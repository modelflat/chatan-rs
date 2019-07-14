extern crate test;

use super::message::Message;

use rayon::prelude::*;
use chrono::{DateTime, Date, Utc, NaiveDate};
use scraper::{Html, Selector};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use std::io::Result;
use std::fs::File;
use std::path::PathBuf;

const BASE_URL: &str = "https://overrustlelogs.net";

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

fn parse_line(line: &str) -> Option<Message> {
    use humantime::parse_rfc3339_weak;

    // According to OverRustle log structure:
    //$[2019-07-01 00:00:42 UTC] someuser: ...
    // ^                   ^     ^
    // 1                  20    26
    // Lets hard-code this to avoid searching for the first ']'. This helps to save
    // ~18-20% of time per call (~15ns per message on my laptop in particular)
    const TS_START: usize = 1;
    const TS_END: usize = 20;
    const USER_START: usize = 26;

    // check 0
    if line.len() > USER_START {
        // Unsafe calls to save several ns.

        // 1. Safe due to check 0
        let user_end = TS_END + unsafe {
            line.get_unchecked(TS_END..line.len() - 1).find(':')?
        };

        // 2. Safe due to check 0
        let ts = unsafe {
            line.get_unchecked(TS_START..TS_END)
        };

        // 3. Safe due to check 0 and stmt 1
        let user = unsafe {
            line.get_unchecked(USER_START..user_end)
        };

        // 4. Safe due to stmt 1
        let message = unsafe {
            line.get_unchecked(user_end + 2..line.len())
        };

        if let Ok(time) = {
            parse_rfc3339_weak(ts).map(|t| DateTime::<Utc>::from(t))
        } {
            Some(Message { time, user, message })
        } else {
            None
        }
    } else {
        None
    }
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

pub fn parse_files(files: &[String]) -> Vec<Message> {
    files
        .par_iter()
        .flat_map(|file| {
            file.par_split_terminator('\n').filter_map(|line| parse_line(line))
        })
        .collect()
}

pub struct LocalChannelLogs {
    channel: String,
    root_path: PathBuf,
    index: Vec<LogFileUrl>
}

fn make_file_path(root_path: &PathBuf, date: &Date<Utc>) -> PathBuf {
    root_path.join(date.format("%Y-%m-%d.txt").to_string())
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
        self.index
            .par_iter_mut()
            .for_each(|l: &mut LogFileUrl| {
                if !missing_only || l.path.is_none() {
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

    pub fn load_date_range(&self, start_date: &Date<Utc>, end_date: &Date<Utc>) -> Option<Vec<String>> {
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

        let res: Vec<String> = dates
            .par_iter()
            .map(|l| {
                l.path.as_ref().map_or(String::new(), |path| {
                    String::from_utf8(std::fs::read(path).expect("Index contains non-existing files!"))
                        .expect("File in index is not a valid UTF8!")
                })
            })
            .collect();
        Some(res)
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use test::Bencher;

    #[test]
    fn test_parse_line() {
        let line = "[2019-07-01 00:00:42 UTC] someuser: message message message message FeelsGoodMan 123";
        match parse_line(line) {
            Some(m) => {
                assert_eq!(m.user, "someuser");
                assert_eq!(m.message, "message message message message FeelsGoodMan 123");
                assert_eq!(
                    m.time,
                    DateTime::<Utc>::from(humantime::parse_rfc3339_weak("2019-07-01 00:00:42").unwrap())
                );
            },
            None => assert!(false, "Message should parse correctly")
        }
    }

    #[bench]
    fn bench_parse_line(b: &mut Bencher) {
        let line = "[2019-07-01 00:00:42 UTC] someuser: message";
        b.iter(|| parse_line(line))
    }
}

