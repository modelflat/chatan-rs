extern crate chatan;
extern crate structopt;
use structopt::StructOpt;

use chatan::overrustle::{OverRustleLogs, DataLoadMode};
use crate::chatan::chatlog::DailyChatLog;
use chrono::{Utc, DateTime};

use std::path::PathBuf;
use counter::Counter;
use std::fs::File;
use serde::Serialize;
use chatan::chatlog::WindowStats;

#[derive(Debug, StructOpt)]
#[structopt(about = "Compute a rolling top of specific tokens from logs")]
struct RollingTop {
    #[structopt(subcommand)]
    mode: Mode,
    #[structopt(name = "channel", long)]
    channel: String,
    #[structopt(name = "start", long)]
    start: Option<DateTime<Utc>>,
    #[structopt(name = "end", long)]
    end: Option<DateTime<Utc>>,
    #[structopt(name = "step", long)]
    step: u32,
    #[structopt(name = "size", long)]
    size: u32,
    #[structopt(name = "top", long)]
    n_top: u64,
    #[structopt(name = "data-load-mode", long)]
    data_load_mode: DataLoadMode,
    #[structopt(name = "frequency-threshold", long)]
    threshold: u64,
    #[structopt(name = "output", long)]
    output_file: PathBuf,
    #[structopt(name = "cache-dir", long)]
    cache_dir: PathBuf
}

#[derive(Debug, StructOpt)]
enum Mode {
    #[structopt(name = "emotes")]
    Emotes {
        #[structopt(name = "index", long)]
        index: PathBuf,
    },
    #[structopt(name = "tokens")]
    Tokens,
    #[structopt(name = "messages")]
    Messages,
}

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

fn convert_counter(t0: &DateTime<Utc>, t1: &DateTime<Utc>, thr: u64, top: u64, counter: Counter<&str, u64>) -> RollingTopWords {
    let most_common = chatan::util::most_common(counter, thr);
    let mut top_tokens: Vec<(String, u64)> = Vec::with_capacity(top as usize);
    most_common.iter().take(top as usize).for_each(|(s, n)| top_tokens.push((s.to_string(), *n)));
    RollingTopWords::new(*t0, *t1, top_tokens)
}

fn main() {
    let opt = RollingTop::from_args();

    let mut logs = OverRustleLogs::make_and_sync(
        opt.cache_dir.clone(), opt.channel.clone(), opt.data_load_mode
    );

    println!("{}", &logs);

    let (logs_first, logs_last) = logs.range().unwrap();

    let start = opt.start.unwrap_or(logs_first.and_hms(0, 0, 0));
    let end = opt.end.unwrap_or(logs_last.and_hms(0, 0, 0));
    let step = opt.step;
    let size = opt.size;
    let top = opt.n_top;
    let threshold = opt.threshold;

    println!(
        "Window params: start={:?} end={:?} step={:?} size={:?} ; will gather top={} token_type='{:?}' per window",
        start, end, step, size, top, opt.mode
    );

    let mut result = Vec::new();
    let t = std::time::Instant::now();

    match opt.mode {
        Mode::Messages => {
            logs.slide(
                start, end, step, size,
                |t0, t1, win| {
                    println!("Window {:?} -- {:?}", t0, t1);
                    let counter: Counter<&str, u64> = win.map(|m| m.message()).collect();
                    result.push(convert_counter(t0, t1, threshold, top, counter))
                }
            )
        },
        mode @ _ => {
            let f = |t0: &DateTime<Utc>, t1: &DateTime<Utc>, win: WindowStats| {
                println!("Window {:?} -- {:?}", t0, t1);
                result.push(convert_counter(t0, t1, threshold, top, win.token_counts))
            };

            match mode {
                Mode::Emotes { index, .. } => {
                    let emote_index = chatan::emote_index::load_index(&index).expect("Could not load emote index");
                    logs.slide_token_counts(start, end, step, size, f, |t| emote_index.contains_key(t))
                },
                Mode::Tokens => logs.slide_token_counts(start, end, step, size, f, |_| true),
                _ => unreachable!()
            }
        }
    }.ok().expect("Failed to slide through the logs");

    let file = File::create(opt.output_file)
        .expect("Could not create output file");

    serde_json::to_writer(file, &result)
        .expect("Could not write output file");

//    let mut wrt = csv::Writer::from_writer(file);
//    for el in &result {
//        wrt.serialize(el).expect("Could not write output file");
//    }

    println!("Successfully rolled through logs in {:.3}s", t.elapsed().as_secs_f64());
}
