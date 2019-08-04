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

fn main() {
    let mut logs = ChannelLogs::new(PathBuf::from_str(".").unwrap(), "forsen", DataLoadMode::Local);
    logs.sync().expect("Couldn't sync channel logs");

    let start = Utc.ymd(2015, 5, 1).and_hms(0, 0, 0);
    let end = Utc.ymd(2019, 5, 1).and_hms(0, 0, 0);
    let step = Duration::from_secs(86400 / 1);
    let size = Duration::from_secs(86400 * 3);

    let t = std::time::Instant::now();

    let mut global_counter: Counter<String, u64> = Counter::new();

    logs.slide(start, end, step, size, |t0, t1, win| {
        eprintln!("Processing window {:?} -- {:?}", t0, t1);
        let mut cnt = 0;
        let mut counter: Counter<&str, u64> = Counter::new();

        win.for_each(|msg| {
            cnt += 1;
            counter += msg.message.split_whitespace().filter(|s| s.len() > 1);
        });

        global_counter += counter.iter().map(|(s, n)| (s.to_string(), *n)).collect::<Counter<String, u64>>();

        eprintln!("Entries: {} / Tokens so far: {}", cnt, global_counter.len());
    });

//    let file = File::create("top_words.json").expect("Cannot create output file");
//    serde_json::to_writer(file, &result).expect("Could not write output file");

    eprintln!("Rolling through logs took {:.3}s", t.elapsed().as_secs_f64());
}
