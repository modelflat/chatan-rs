#![feature(duration_float)]

extern crate chatan;
extern crate structopt;
use structopt::StructOpt;

use chatan::overrustle::{OverRustleLogs, DataLoadMode};
use crate::chatan::chatlog::DailyChatLog;
use chrono::{Utc, DateTime};

use std::path::PathBuf;
use std::fs::File;
use std::collections::HashSet;
use chatan::util;

#[derive(Debug, StructOpt)]
#[structopt(about = "Discover popular emotes being used in the channel's history")]
struct DiscoverEmotes {
    #[structopt(name = "channel", long)]
    channel: String,
    #[structopt(name = "start", long)]
    start: Option<DateTime<Utc>>,
    #[structopt(name = "end", long)]
    end: Option<DateTime<Utc>>,
    #[structopt(name = "size", long)]
    size: Option<u32>,
    #[structopt(name = "top", long)]
    top: u64,
    #[structopt(name = "threshold", long)]
    threshold: Option<u64>,
    #[structopt(name = "data-load-mode", long)]
    data_load_mode: DataLoadMode,
    #[structopt(name = "output", long)]
    output_file: PathBuf,
    #[structopt(name = "cache-dir", long)]
    cache_dir: PathBuf
}

fn main() {
    let opt = DiscoverEmotes::from_args();

    let mut logs = OverRustleLogs::make_and_sync(
        opt.cache_dir.clone(), opt.channel.clone(), opt.data_load_mode
    );

    println!("{}", &logs);

    let (logs_first, logs_last) = logs.range().unwrap();

    let start = opt.start.unwrap_or(logs_first.and_hms(0, 0, 0));
    let end = opt.end.unwrap_or(logs_last.and_hms(0, 0, 0));
    let size = opt.size.unwrap_or(86400);
    let top = opt.top as usize;
    let thr = opt.threshold.unwrap_or(0);

    let mut result = HashSet::new();
    let t = std::time::Instant::now();

    println!("Processing logs...");

    logs.slide_token_counts(
        start, end, size, size,
        |t0, t1, win| {
            result.extend(
                util::most_common(win.token_counts, thr).iter()
                    .take(top)
                    .map(|(s, _)| s.to_string())
                    .collect::<HashSet<String>>()
            );
        },
        |tok| 2 <= tok.len() && tok.len() <= 32 && tok.chars().all(|c| c.is_ascii_alphanumeric())
    ).ok().expect("Failed to iterate through the logs");

    println!("{} popular tokens found", result.len());

    let file = File::create(opt.output_file).expect("Could not create output file");
    serde_json::to_writer(file, &result).expect("Could not write output file");

    println!("Successfully rolled through logs in {:.3}s", t.elapsed().as_secs_f64());
}
