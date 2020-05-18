extern crate test;

use crate::message::*;
use crate::util::*;
use crate::chatlog::DailyChatLog;

use std::io;
use std::result::Result;
use std::path::PathBuf;
use std::iter::Iterator;
use std::fs::File;

use rayon::prelude::*;
use chrono::{Date, Utc, NaiveDate, Duration};
use scraper::{Html, Selector};
use reqwest::Client;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use crate::message::overrustle::parse_string;

const BASE_URL: &str = "https://overrustlelogs.net";


#[derive(Debug, Clone)]
pub struct LogFileUrl {
    url: String,
    path: Option<PathBuf>,
    date: Date<Utc>
}

impl LogFileUrl {

    pub fn from_overrustle_url(url: &str) -> Result<LogFileUrl, chrono::format::ParseError> {
        let date_str = url.rsplitn(2, '/').next().unwrap();
        let parsing_result = NaiveDate::parse_from_str(date_str, "%Y-%m-%d");
        match parsing_result {
            Ok(date) => {
                let date = Date::<Utc>::from_utc(date, Utc);
                Ok(LogFileUrl { url: format!("{}{}.txt", BASE_URL, url), path: None, date })
            },
            Err(err) => Err(err)
        }
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

/// Data load mode for
#[derive(Debug)]
pub enum DataLoadMode {
    /// Data for each index date will be loaded only on demand and won't be cached
    Remote,

    /// Data for each index date will be loaded only on demand and will be cached on disk
    RemoteAndCache,

    /// All data in index will be loaded upon calling `ChannelLogs::sync()`.
    /// If missing data will be encoutered later, an attempt will be made to download it.
    /// No caching occurs upon these downloads.
    Prefetch,

    /// All data in index will be loaded upon calling `ChannelLogs::sync()`.
    /// If missing data will be encoutered later, an attempt will be made to download it.
    /// If downloaded successfully, will also cache new data to disk.
    PrefetchAndCache,

    /// Use local storage only. No connection attempts will be made.
    Local,
}

// TODO replace with macro. But don't depend on the external crates, that seems to be an overkill for now
impl FromStr for DataLoadMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "remote" => Ok(DataLoadMode::Remote),
            "remoteandcache" => Ok(DataLoadMode::RemoteAndCache),
            "prefetch" => Ok(DataLoadMode::Prefetch),
            "prefetchandcache" => Ok(DataLoadMode::PrefetchAndCache),
            "local" => Ok(DataLoadMode::Local),
            _ => Err(s.to_string())
        }
    }
}

// TODO same
impl ToString for DataLoadMode {
    fn to_string(&self) -> String {
        match self {
            DataLoadMode::Local => "Local".to_string(),
            DataLoadMode::Remote => "Remote".to_string(),
            DataLoadMode::RemoteAndCache => "RemoteAndCache".to_string(),
            DataLoadMode::Prefetch => "Prefetch".to_string(),
            DataLoadMode::PrefetchAndCache => "PrefetchAndCache".to_string(),
        }
    }
}

pub enum SlideError {
    InvalidTimeInterval,
    NotEnoughDataInIndex,
}

#[derive(Debug)]
pub struct OverRustleLogs {
    root_path: PathBuf,
    channel: String,
    client: Client,
    index: Vec<LogFileUrl>,
    mode: DataLoadMode,
}

impl OverRustleLogs {

    pub fn new(root_path: PathBuf, channel: String, mode: DataLoadMode) -> OverRustleLogs {
        OverRustleLogs {
            root_path, channel, client: Client::new(), index: Vec::new(), mode
        }
    }

    pub fn make_and_sync(root_path: PathBuf, channel: String, mode: DataLoadMode) -> OverRustleLogs {
        let mut o = OverRustleLogs::new(root_path, channel, mode);
        o.sync().expect("Could not sync logs");
        o
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
                // TODO 14 days should be parameterized
                if l.path.is_none() || l.date >= today - Duration::days(2) {
                    let path = make_file_path(root_path, &l.date);

                    // NOTE this doesn't work for overrustle as they don't send content-length
                    // // check file sizes
                    // if path.exists() {
                    //     let local_size = std::fs::metadata(&path).expect("Cannot get metadata for file").len();
                    //     match client.head(&l.url).send() {  
                    //         Ok(resp) => {
                    //             let remote_size = resp.content_length().unwrap_or(0);
                    //             if local_size == remote_size {
                    //                 // we don't need to download files if they are identical
                    //                 bar.inc(1);
                    //                 return;
                    //             }
                    //         }
                    //         Err(err) => {
                    //             // we silently discard path if unable to do HEAD against resource
                    //             // TODO log error properly?
                    //             eprintln!("ERR: cannot fetch metadata for remote file {:?}", err);
                    //             bar.inc(1);
                    //             return;
                    //         }
                    //     }
                    // }
                    // eprintln!("INF: downloading remote file {:?}", path);
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

    pub fn sync(&mut self) -> io::Result<()> {
        self.index.clear();
        match self.mode {
            DataLoadMode::Remote | DataLoadMode::RemoteAndCache => {
                self.index.extend(self.detect_remote_files()?);
            },
            DataLoadMode::Local => {
                self.index.extend(self.detect_local_files()?);
            },
            DataLoadMode::Prefetch | DataLoadMode::PrefetchAndCache => {
                self.index.extend(self.detect_remote_files()?);
                self.download_missing_files()?;
            }
        }
        Ok(())
    }

}

impl Display for OverRustleLogs {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        let (n_local_files, local_files_size) = self.index
            .iter()
            .filter_map(|l| l.path.as_ref())
            .fold((0u64, 0u64), |mut a, e| {
                a.0 += 1;
                a.1 += std::fs::metadata(e).map_or_else(|_| 0, |m| m.len());
                a
            });

        write!(
            f,
            "ChannelLogs {{ {channel} @ data_load_mode = {mode:?} ; local_path = {path:?} ; \
             URLs in index = {index_size} ; Local files in index = {local_count} ; Total size on disk = {size} }}",
            channel = self.channel,
            mode = self.mode,
            path = self.root_path,
            index_size = self.index.len(),
            local_count = n_local_files,
            size = indicatif::HumanBytes(local_files_size)
        )
    }
}

impl DailyChatLog for OverRustleLogs {

    fn range(&self) -> Option<(Date<Utc>, Date<Utc>)> {
        if self.index.is_empty() {
            None
        } else {
            Some((self.index.first().unwrap().date, self.index.last().unwrap().date))
        }
    }

    fn load(&mut self, date: &Date<Utc>) -> Option<Messages> {
        // TODO proper error handling
        let idx = self.index.binary_search_by_key(date, |l| l.date).map_err(|_| ()).ok()?;
        let entry = &mut self.index[idx];

        let read_path = |path: &PathBuf|
            std::fs::read_to_string(path).map_err(|_| ());

        let res = match self.mode {
            DataLoadMode::Remote => {
                // simply get data from network
                get_text(&self.client, &entry.url).map_err(|_| ())
            },
            DataLoadMode::RemoteAndCache | DataLoadMode::PrefetchAndCache => {
                match entry.path.as_ref() {
                    // cache hit, read from path
                    Some(path) => read_path(&path),
                    // cache miss, need to download data and save into fs
                    None => {
                        let data = get_text(&self.client, &entry.url).map_err(|_| ()).ok()?;
                        let path = make_file_path(&self.root_path, &date);
                        entry.path = Some(path.clone());
                        std::fs::write(&path, &data).map_err(|_| ()).ok()?;
                        Ok(data)
                    }
                }
            },
            DataLoadMode::Local | DataLoadMode::Prefetch => match entry.path.as_ref() {
                // read data from path
                Some(path) => read_path(&path),
                None => Err(())
            }
        };

        Some(res.map_or_else(|_| Messages::empty(), |s| parse_string(s)))
    }
}

fn get_text(client: &Client, url: &String) -> reqwest::Result<String> {
    client.get(url).send()?.text()
}

fn select_urls(client: &Client, url: &String) -> Vec<String> {
    let selector = Selector::parse(".list-group-item").unwrap();
    let urls = get_text(client, url)
        .map(|text| {
            let document = Html::parse_document(text.as_str());
            let mut urls = Vec::new();
            document
                .select(&selector)
                .for_each(|s| {
                    urls.push(s.value().attr("href").unwrap().to_string())
                });
            urls
        });
    match urls {
        Ok(urls) => urls,
        Err(err) => {
            eprintln!("Failed to load overrustle urls from {}: {:?}", url, err);
            Vec::new()
        }
    }
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
                && !s.ends_with("bans")
        })
        .filter_map(|s| {
            bar.inc(1);
            let parsed_url = LogFileUrl::from_overrustle_url(&s);
            match parsed_url {
                Ok(url) => Some(url),
                Err(err) => {
                    eprintln!("Error parsing url ({}): {:?}", s, err);
                    None
                }
            }
        })
        .collect();
    day_urls.sort_by(|l, r| l.date.cmp(&r.date));
    bar.finish();
    day_urls
}
