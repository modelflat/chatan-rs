#![feature(test)]
#![feature(duration_float)]
#![feature(drain_filter)]
#![feature(fn_traits)]
//#![feature(unboxed_closures)]

mod chatan;
use chatan::overrustle::ChannelLogs;

use chrono::Utc;
use chrono::offset::TimeZone;

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use counter::Counter;
use std::collections::HashMap;
use std::fs::File;


fn most_common(counter: Counter<&str, u64>, threshold: u64) -> Vec<(&str, u64)> {
    let mut items = counter.iter()
        .filter_map(|(key, &count)| {
            if count > threshold {
                Some((key.clone(), count.clone()))
            } else { None }
        })
        .collect::<Vec<_>>();
    items.sort_unstable_by(|l, r| r.1.cmp(&l.1));
    items
}


fn main() {
    let mut logs = ChannelLogs::new(PathBuf::from_str(".").unwrap(), "forsen");
    logs.sync(false, false).expect("Couldn't sync channel logs");
    logs.print_info();

    let start = Utc.ymd(2019, 1, 1).and_hms(0, 0, 0);
    let end = Utc.ymd(2019, 2, 1).and_hms(0, 0, 0);
    let step = Duration::from_secs(86400 / 1);
    let size = Duration::from_secs(86400 * 3);

    let t = std::time::Instant::now();

    let top = 100;
    let mut top_100_words = Vec::new();

    logs.roll_index(step, size, |t0, t1, win| {
        eprintln!("Processing window {:?} -- {:?}", t0, t1);
        let mut cnt = 0;
        let mut counter: Counter<&str, u64> = Counter::new();

        win.for_each(|msg| {
            cnt += 1;
            counter += msg.message.split_whitespace();
        });

        // drop the tokens which are encountered only once
        let most_common = most_common(counter, 1);

        let mut top_words: HashMap<String, u64> = HashMap::with_capacity(top);
        most_common.iter().take(top).for_each(|(s, n)| {
            top_words.insert(s.to_string(), *n);
        });

        top_100_words.push((*t0, *t1, top_words));

        eprintln!("Entries: {} / Top tokens: {}", cnt, most_common.len());
    }).expect("Failed to roll logs");

    let file = File::create("top_words.json").expect("Cannot create output file");
    serde_json::to_writer(file, &top_100_words);

    eprintln!("Rolling through logs took {:.3}s", t.elapsed().as_secs_f64());
}
