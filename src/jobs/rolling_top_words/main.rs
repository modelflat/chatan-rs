#![feature(duration_float)]

extern crate chatan;

use chatan::overrustle::{ChannelLogs, DataLoadMode};
use chrono::{Utc, DateTime};
use chrono::offset::TimeZone;

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use counter::Counter;
use std::fs::File;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct RollingTopWords {
    t0: DateTime<Utc>,
    t1: DateTime<Utc>,
    data: Vec<(String, u64)>,
}

impl RollingTopWords {
    fn new(t0: DateTime<Utc>, t1: DateTime<Utc>, data: Vec<(String, u64)>) -> RollingTopWords {
        RollingTopWords { t0, t1, data }
    }
}

fn main() {
    let mut logs = ChannelLogs::new(PathBuf::from_str(".").unwrap(), "forsen", DataLoadMode::Local);
    logs.sync().expect("Couldn't sync channel logs");

    let start = Utc.ymd(2015, 5, 1).and_hms(0, 0, 0);
    let end = Utc.ymd(2019, 5, 1).and_hms(0, 0, 0);
    let step = Duration::from_secs(86400 / 1);
    let size = Duration::from_secs(86400 * 3);

    let t = std::time::Instant::now();

    let top = 100;
    let mut result = Vec::new();

    logs.slide(start, end, step, size, |t0, t1, win| {
        eprintln!("Processing window {:?} -- {:?}", t0, t1);
        let mut cnt = 0;
        let mut counter: Counter<&str, u64> = Counter::new();

        win.for_each(|msg| {
            cnt += 1;
            counter += msg.message.split_whitespace().filter(|s| s.len() > 1);
        });

        let tokens = counter.len();
        // drop the tokens which are encountered only once
        let most_common = chatan::util::most_common(counter, 1);

        let mut top_words: Vec<(String, u64)> = Vec::with_capacity(top);
        most_common.iter().take(top).for_each(|(s, n)| {
            top_words.push((s.to_string(), *n));
        });

        result.push(RollingTopWords::new(*t0, *t1, top_words));

        eprintln!("Entries: {} / Tokens: {}", cnt, tokens);
    });

    let file = File::create("top_words.json").expect("Cannot create output file");
    serde_json::to_writer(file, &result).expect("Could not write output file");

    eprintln!("Rolling through logs took {:.3}s", t.elapsed().as_secs_f64());
}
