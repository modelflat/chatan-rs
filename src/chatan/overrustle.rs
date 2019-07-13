extern crate test;

use super::message::Message;

use rayon::prelude::*;
use chrono::{DateTime, Date, Utc, NaiveDate};
use scraper::{Html, Selector};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;

const BASE_URL: &str = "https://overrustlelogs.net";

#[derive(Debug)]
pub struct LogFileUrl {
    url: String,
    date: Date<Utc>
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

pub fn fetch_files(client: &Client, urls: &[LogFileUrl]) -> Vec<String> {
    let bar = make_progress_bar(urls.len());
    let files = urls.par_iter().map(|url| {
        let contents = match get_text(client, &url.url) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("! Error processing text response from {:?}: {:?}", url, e);
                String::new()
            }
        };
        bar.inc(1);
        contents
    }).collect();
    bar.finish();
    files
}

pub fn parse_files(files: &[String]) -> Vec<Message> {
    files
        .par_iter()
        .flat_map(|file| {
            file.par_split_terminator('\n').filter_map(|line| parse_line(line))
        })
        .collect()
}

fn get_text(client: &Client, url: &String) -> reqwest::Result<String> {
    client.get(url).send()?.text()
}

// wtf why doesn't Rust have this built-in?
fn capitalized(s: String) -> String {
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

pub fn get_all_urls_for_channel(client: &Client, channel: String) -> Vec<LogFileUrl> {
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
            let date_str = s.rsplitn(2, '/').next().unwrap();
            let date = Date::<Utc>::from_utc(
                NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                    .expect("Overrustle url should end with parsable date, but it doesn't"),
                Utc
            );
            bar.inc(1);
            LogFileUrl { url: format!("{}{}.txt", BASE_URL, s), date }
        })
        .collect();
    day_urls.sort_by(|l, r| l.date.cmp(&r.date));
    bar.finish();
    day_urls
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

