extern crate test;

use crate::message::*;
use crate::util::*;

use std::io;
use std::result::Result;
use std::path::PathBuf;
use std::iter::Iterator;
use std::cmp::min;
use std::fs::File;
use std::time::Duration;

use rayon::prelude::*;
use chrono::{Date, DateTime, Utc, NaiveDate};
use scraper::{Html, Selector};
use reqwest::Client;
use log::{info, warn};


const BASE_URL: &str = "https://overrustlelogs.net";
const SECS_PER_DAY: i64 = 24 * 60 * 60;

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

    pub fn from_local_path(channel: &String, path: PathBuf) -> Option<LogFileUrl> {
        let date_str = path.file_name().unwrap_or("".as_ref()).to_str().unwrap();
        let date = Date::<Utc>::from_utc(NaiveDate::parse_from_str(date_str, "%Y-%m-%d.txt").ok()?, Utc);
        let month_name_year = date.format("%B%%20%Y").to_string();
        let url = format!("{}/{}%20chatlog/{}/{}.txt", BASE_URL, capitalized(channel),
                          capitalized(&month_name_year), date_str);
        Some(LogFileUrl { url, path: Some(path), date })
    }

    pub fn detect_local(&mut self, root_path: &PathBuf) -> bool {
        let full_path = root_path.join(self.date.format("%Y-%m-%d.txt").to_string());
        self.path = if full_path.is_file() { Some(full_path) } else { None };
        self.path.is_some()
    }

}

#[derive(Debug)]
pub struct WindowScanError;

pub struct WindowInfo {
    pub dates: Vec<Date<Utc>>,
}

impl WindowInfo {

    pub fn new() -> WindowInfo {
        WindowInfo { dates: Vec::new() }
    }
}

#[derive(Debug)]
pub struct ChannelLogs {
    root_path: PathBuf,
    channel: String,
    client: Client,
    index: Vec<LogFileUrl>,
}

impl ChannelLogs {

    pub fn new(root_path: PathBuf, channel: &str) -> ChannelLogs {
        ChannelLogs {
            root_path, channel: channel.to_string(), client: Client::new(), index: Vec::new()
        }
    }

    fn detect_local_files(&self) -> io::Result<Vec<LogFileUrl>> {
        let root_path = self.root_path.canonicalize()?.join(&self.channel);

        let mut index = if !root_path.is_dir() {
            std::fs::create_dir_all(&root_path)?;
            Vec::new()
        } else {
            std::fs::read_dir(&root_path)?
                .filter_map(|path| {
                    let path = path.expect("Cannot get Path from DirEntry").path();
                    LogFileUrl::from_local_path(&self.channel, path)
                })
                .collect::<Vec<_>>()
        };

        index.sort_unstable_by_key(|l| l.date);

        Ok(index)
    }

    fn detect_remote_files(&self) -> io::Result<Vec<LogFileUrl>> {
        let root_path = self.root_path.join(&self.channel);
        let mut index = get_all_urls_for_channel(&self.client, &self.channel);
        index
            .iter_mut()
            .for_each(|l| {
                let path = make_file_path(&root_path, &l.date);
                if path.is_file() {
                    let meta = std::fs::metadata(&path).expect("Cannot get metadata for file");
                    if meta.len() > 0 { // only count non-empty files
                        l.path = Some(path)
                    }
                }
            });
        index.sort_unstable_by_key(|l| l.date);
        Ok(index)
    }

    fn download_missing_files(&mut self) -> io::Result<()> {
        let missing_count = self.index.iter().filter(|l| l.path.is_none()).count();
        let bar = make_progress_bar(missing_count);
        let root_path = &self.root_path.join(&self.channel);
        if !root_path.is_dir() {
            std::fs::create_dir_all(root_path)?;
        }
        let client = &self.client;
        let today = Utc::today();

        self.index
            .par_iter_mut()
            .for_each(|l: &mut LogFileUrl| {
                if l.path.is_none() && l.date < today {
                    let path = make_file_path(root_path, &l.date);
                    if let Ok(mut res) = client.get(&l.url).send() {
                        let mut file = File::create(&path).expect("Unable to create local file");
                        std::io::copy(&mut res, &mut file).expect("Unable to write to local file");
                    }
                    l.path = Some(path);
                    bar.inc(1);
                }
            });

        Ok(())
    }

    pub fn sync(&mut self, download_files: bool, offline: bool) -> io::Result<()> {
        self.index = if offline {
            self.detect_local_files()?
        } else {
            self.detect_remote_files()?
        };
        if !offline && download_files {
            self.download_missing_files()?;
        }
        Ok(())
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

    pub fn load_date(&self, date: &Date<Utc>) -> Result<String, ()> {
        let idx = self.index.binary_search_by_key(date, |l| l.date).map_err(|_| ())?;
//            .expect(format!("Date {:?} is not present in the index", date).as_str());

        let entry = &self.index[idx];
        match entry.path.as_ref() {
            Some(path) => Ok(std::fs::read_to_string(path).expect("File in index is not a valid UTF8!")),
            None => get_text(&self.client, &entry.url).map_err(|_| ())
        }
    }

    pub fn roll_index<F>(
        &self, step: Duration, size: Duration, mut f: F
    ) -> Result<(), WindowScanError>
        where F: FnMut(&DateTime<Utc>, &DateTime<Utc>, &mut dyn Iterator<Item=&Message>) -> ()
    {
        let start_date = self.index.first().ok_or(WindowScanError {})?.date.and_hms(0, 0, 0);
        self.roll_update(start_date, step, size, f)
    }

    pub fn roll_update<F>(
        &self, start: DateTime<Utc>, step: Duration, size: Duration, mut f: F
    ) -> Result<(), WindowScanError>
        where F: FnMut(&DateTime<Utc>, &DateTime<Utc>, &mut dyn Iterator<Item=&Message>) -> ()
    {
        let end_date = day_after(self.index.last().ok_or(WindowScanError {})?.date).and_hms(0, 0, 0);
        self.roll(start, end_date, step, size, f)
    }

    pub fn roll<F>(
        &self, start: DateTime<Utc>, end: DateTime<Utc>, step: Duration, size: Duration, mut f: F
    ) -> Result<(), WindowScanError>
        where F: FnMut(&DateTime<Utc>, &DateTime<Utc>, &mut dyn Iterator<Item=&Message>) -> ()
    {
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

        let mut cur = start;

        let size = chrono::Duration::from_std(size)
            .expect("??? cannot convert std::time::Duration to chrono::Duration");
        let step = chrono::Duration::from_std(step)
            .expect("??? cannot convert std::time::Duration to chrono::Duration");

        // could be made more generic, but we have one file per day
        let num_units_in_window =
            size.num_seconds() / SECS_PER_DAY + if size.num_seconds() % SECS_PER_DAY == 0 { 0 } else { 1 };

        let _num_units_in_step = min(
            step.num_seconds() / SECS_PER_DAY + if step.num_seconds() % SECS_PER_DAY == 0 { 0 } else { 1 }, 1
        );

        let mut loaded_files: Vec<(Date<Utc>, Vec<Message>)> =
            Vec::with_capacity((num_units_in_window * 2) as usize);

        while cur + size <= end {
            let cur_start = cur;
            let cur_end = cur + size;
            let cur_date = cur.date();

            // unload files that are no longer needed
            loaded_files.drain_filter(|e| e.0 < cur_date);

            let mut date = match loaded_files.last() {
                Some(v) => {
                    // files up to this date were loaded.
                    day_after(v.0)
                },
                None => {
                    // no files loaded yet
                    cur_date
                }
            };

            while date < (cur + size + step).date() {
                // worst error handling ever, todo improve
                let file = self.load_date(&date);
                let messages = match file {
                    Ok(file) => parse_string(&file),
                    Err(_) => {
                        eprintln!("Could not load data for date {:?}!", &date);
                        Vec::new()
                    }
                };
                loaded_files.push((date, messages));
                date = day_after(date);
            }

            let mut window = loaded_files
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
                });

            f(&cur_start, &cur_end, &mut window);

            cur = cur + step;
        }
        Ok(())
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

fn day_after(date: chrono::Date<Utc>) -> chrono::Date<Utc> {
    let day = chrono::Duration::from_std(Duration::from_secs(SECS_PER_DAY as u64))
        .expect("Cannot convert from std Duration to chrono Duration");
    date + day
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

fn make_file_path(root_path: &PathBuf, date: &Date<Utc>) -> PathBuf {
    root_path.join(date.format("%Y-%m-%d.txt").to_string())
}

fn get_all_urls_for_channel(client: &Client, channel: &String) -> Vec<LogFileUrl> {
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
