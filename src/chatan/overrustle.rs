extern crate test;

use crate::chatan::message::*;

use rayon::prelude::*;
use chrono::{Date, DateTime, Utc, NaiveDate};
use std::time::Duration;
use scraper::{Html, Selector};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use std::io::Result;
use std::fs::File;
use std::path::PathBuf;
use std::iter::Iterator;
use std::cmp::min;


const BASE_URL: &str = "https://overrustlelogs.net";

#[derive(Debug, Clone)]
pub struct LogFileUrl {
    url: String,
    path: Option<PathBuf>,
    date: Date<Utc>
}

impl LogFileUrl {
    pub fn from_overrustle_url(url: &str) -> LogFileUrl {
        let date_str = url.rsplitn(2, '/').next().unwrap();
        let date = Date::<Utc>::from_utc(
            NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                .expect("Overrustle url should end with parsable date, but it doesn't"),
            Utc
        );
        LogFileUrl { url: format!("{}{}.txt", BASE_URL, url), path: None, date }
    }
}

#[derive(Debug)]
pub struct LocalChannelLogs {
    channel: String,
    root_path: PathBuf,
    index: Vec<LogFileUrl>
}

pub trait WindowFn {
    fn window_start(&mut self, t0: &DateTime<Utc>, t1: &DateTime<Utc>) -> ();
    fn window_end(&mut self, t0: &DateTime<Utc>, t1: &DateTime<Utc>) -> ();
    fn call(&mut self, msg: &Message);
}

impl LocalChannelLogs {

    pub fn from_path(client: &Client, channel: String, root_path: PathBuf) -> Result<LocalChannelLogs> {
        let root_path = root_path.canonicalize()?.join(&channel);
        if !root_path.is_dir() {
            std::fs::create_dir_all(&root_path)?
        }
        let mut o = LocalChannelLogs { channel, root_path, index: Vec::new() };
        o.update_index(client);
        Ok(o)
    }

    pub fn update_index(&mut self, client: &Client) {
        let mut index =  get_all_urls_for_channel(client, &self.channel);
        let root_path = &self.root_path;
        index
            .iter_mut()
            .for_each(|l| {
                let filepath = make_file_path(root_path, &l.date);
                if filepath.is_file() {
                    let meta = std::fs::metadata(&filepath).expect("Cannot read file metadata");
                    if meta.len() > 0 { // only count non-empty files
                        l.path = Some(filepath)
                    }
                }
            });
        index.sort_by_key(|l| l.date);
        self.index = index;
    }

    pub fn download(&mut self, client: &Client, missing_only: bool) {
        let missing_count = self.index.iter()
            .filter(|l| l.path.is_none())
            .count();
        let bar = make_progress_bar(missing_count);
        let root_path = &self.root_path;
        let today = Utc::today();
        self.index
            .par_iter_mut()
            .for_each(|l: &mut LogFileUrl| {
                if !missing_only || l.path.is_none() && l.date < today {
                    let path = make_file_path(root_path, &l.date);
                    if let Ok(mut res) = client.get(&l.url).send() {
                        let mut file = File::create(&path)
                            .expect("Unable to create local file");
                        std::io::copy(&mut res, &mut file)
                            .expect("Unable to write to local file");
                    }
                    l.path = Some(path);
                    bar.inc(1);
                }
            })
    }

    pub fn print_info(&self) {
        let (n_local_files, local_files_size) = self.index
            .iter()
            .filter_map(|l| l.path.as_ref())
            .fold((0u64, 0u64), |mut a, e| {
                a.0 += 1;
                a.1 += std::fs::metadata(e)
                    .expect("Index file does not exist or permission error occured")
                    .len();
                a
            });
        println!(
            "LocalChannelLogs channel = {} @ local path = {:?}\n:: URLs in index = {}\n\
            :: Local files in index = {}\n:: Total size on disk = {}",
            self.channel, self.root_path, self.index.len(),
            n_local_files, indicatif::HumanBytes(local_files_size)
        )
    }

    pub fn load_date(&self, date: &Date<Utc>) -> String {
        let path = self.index[
            self.index.binary_search_by_key(date, |l| l.date)
                .expect(format!("Date {:?} is not present in the index", date).as_str())
            ].path.as_ref().expect("Date not loaded into local index");
        std::fs::read_to_string(path)
            .expect("File in index is not a valid UTF8!")
    }

    pub fn roll<F: WindowFn>(
        &self, start: DateTime<Utc>, end: DateTime<Utc>, step: Duration, size: Duration, f: &mut F
    ) {
        // window: size=5, step=1
        // buffer_size = size*2 = 10
        //
        //  part 1  ' part 2  ' part 1  '
        // --------- --------- ---------
        // * * * * * * * * * * * * * * * *
        //         .         .         .
        // |       |         .         .
        //   |       |       .         .
        //     |       |     .         .
        //       |       |   .         .
        //         |       | .         .
        //           |       |         .
        //           ^ next disjoint window
        //             |       |       .
        //               |       |     .
        //                 |       |   .
        //                   |       | .
        //                     |       |
        //                     ^ next disjoint window
        //                       |       |

        let mut cur = start;

        let size = chrono::Duration::from_std(size)
            .expect("??? cannot convert std::time::Duration to chrono::Duration");
        let step = chrono::Duration::from_std(step)
            .expect("??? cannot convert std::time::Duration to chrono::Duration");

        const SECS_PER_DAY: i64 = 24 * 60 * 60; // could be made more generic, but we have file per day
        let day: chrono::Duration = chrono::Duration::from_std(Duration::from_secs(SECS_PER_DAY as u64)).unwrap();

        let num_units_in_window =
            size.num_seconds() / SECS_PER_DAY + if size.num_seconds() % SECS_PER_DAY == 0 { 0 } else { 1 };

        let _num_units_in_step = min(
            step.num_seconds() / SECS_PER_DAY + if step.num_seconds() % SECS_PER_DAY == 0 { 0 } else { 1 }, 1
        );

        let mut loaded_files: Vec<(Date<Utc>, Vec<Message>)> =
            Vec::with_capacity((num_units_in_window * 2) as usize);

        while cur + size < end {
            let cur_start = cur;
            let cur_end = cur + size;
            let cur_date = cur.date();

            // unload files that are no longer needed
            loaded_files.drain_filter(|e| e.0 < cur_date);

            let mut date = match loaded_files.last() {
                Some(v) => {
                    // files up to this date were loaded.
                    v.0 + day
                },
                None => {
                    // no files loaded yet
                    cur_date
                }
            };

            while date < (cur + size + step).date() {
                let file = self.load_date(&date);
                let messages = parse_string(&file);
                loaded_files.push((date, messages));
                date = date + day;
            }

            f.window_start(&cur_start, &cur_end);

            loaded_files
                .iter()
                .flat_map(|(_, file)| {
                    if file.is_empty() {
                        return file.iter();
                    }

                    let start_idx = match file.binary_search_by_key(&cur_start, |m| m.time) {
                        Ok(x) => x, Err(x) => x
                    };
                    let end_idx = match file.binary_search_by_key(&cur_end, |m| m.time) {
                        Ok(x) => x, Err(x) => x
                    };

                    file[start_idx..end_idx].iter()
                })
                .for_each(|msg| f.call(msg));

            f.window_end(&cur_start, &cur_end);

            cur = cur + step;
        }
    }

    pub fn load_date_range(&self, start_date: &Date<Utc>, end_date: &Date<Utc>) -> Option<Vec<(Date<Utc>, String)>> {
        if self.index.is_empty() || start_date > end_date {
            return None
        }

        let start_idx = match self.index.binary_search_by_key(start_date, |l| l.date) {
            Ok(i) => i, Err(i) => i
        };
        let end_idx = match self.index.binary_search_by_key(end_date, |l| l.date) {
            Ok(i) => i, Err(i) => i
        };

        let dates = &self.index[start_idx..end_idx + 1];
        if dates.iter().any(|l| l.path.is_none()) {
            eprintln!("[load_date_range] some dates are not present in local index!"); // todo logging
        }
        if dates.len() as i64 != (end_date.naive_utc() - start_date.naive_utc()).num_days() + 1 {
            eprintln!("[load_date_range] remote index is missing dates!"); // todo logging
        }

        let res: Vec<(Date<Utc>, String)> = dates
            .par_iter()
            .map(|l| {
                (l.date, l.path.as_ref().map_or(String::new(), |path| {
                    String::from_utf8(std::fs::read(path).expect("Index contains non-existing files!"))
                        .expect("File in index is not a valid UTF8!")
                }))
            })
            .collect();

        Some(res)
    }
}

fn get_text(client: &Client, url: &String) -> reqwest::Result<String> {
    client.get(url).send()?.text()
}

// wtf why doesn't Rust have this built-in?
fn capitalized(s: &String) -> String {
    let ss = s.clone();
    ss.get(0..1).unwrap().to_uppercase() + ss.get(1..ss.len()).unwrap()
}

fn select_urls(client: &Client, url: &String) -> Vec<String> {
    let selector = Selector::parse(".list-group-item").unwrap();
    get_text(client, url)
        .map(|text| {
            let document = Html::parse_document(text.as_str());
            let mut urls = Vec::new();
            document
                .select(&selector)
                .for_each(|s| {
                    urls.push(s.value().attr("href").unwrap().to_string())
                });
            urls
        })
        .expect(format!("Failed to load overrustle urls from {}", url).as_str())
}

fn make_progress_bar(count: usize) -> ProgressBar {
    let bar = ProgressBar::new(count as u64);
    bar.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] [{wide_bar}] {percent}% {pos}/{len}")
            .progress_chars("=> ")
    );
    bar
}

fn make_file_path(root_path: &PathBuf, date: &Date<Utc>) -> PathBuf {
    root_path.join(date.format("%Y-%m-%d.txt").to_string())
}

pub fn get_all_urls_for_channel(client: &Client, channel: &String) -> Vec<LogFileUrl> {
    let channel_url = format!("{}/{}%20chatlog/", BASE_URL, capitalized(channel));
    let month_urls = select_urls(&client, &channel_url);

    let bar = make_progress_bar(month_urls.len() * 31);

    let mut day_urls: Vec<LogFileUrl> = month_urls
        .par_iter()
        .flat_map(|url| {
            let url = format!("{}{}", BASE_URL, url);
            select_urls(&client, &url)
        })
        .filter(|s| {
            !s.ends_with("userlogs")
                && !s.ends_with("broadcaster")
                && !s.ends_with("subscribers")
        })
        .map(|s| {
            bar.inc(1);
            LogFileUrl::from_overrustle_url(&s)
        })
        .collect();
    day_urls.sort_by(|l, r| l.date.cmp(&r.date));
    bar.finish();
    day_urls
}
