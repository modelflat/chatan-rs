#![feature(test)]
#![feature(duration_float)]
#![feature(drain_filter)]
#![feature(fn_traits)]
#![feature(unboxed_closures)]

mod chatan;
use chatan::overrustle::ChannelLogs;

use chrono::Utc;
use chrono::offset::TimeZone;

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use counter::Counter;


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
    logs.sync().expect("Couldn't sync channel logs");
    logs.print_info();

    let start = Utc.ymd(2019, 1, 1).and_hms(0, 0, 0);
    let end = Utc.ymd(2019, 4, 1).and_hms(0, 0, 0);
    let step = Duration::from_secs(86400 / 1);
    let size = Duration::from_secs(86400 * 3);

    let t = std::time::Instant::now();

    logs.roll(start, end, step, size, |t0, t1, win| {
        eprintln!("Processing window {:?} -- {:?}", t0, t1);
        let mut cnt = 0;
        let mut counter: Counter<&str, u64> = Counter::new();

        win.for_each(|msg| {
            cnt += 1;
            counter += msg.message.split_whitespace();
        });

        // drop the tokens which are encountered only once
        let most_common = most_common(counter, 1);

        let top: Vec<&(&str, u64)> = most_common.iter().take(5).collect();

        eprintln!("Entries: {} / Top tokens: {} / Top word: {:?}", cnt, most_common.len(), top);
    });

    eprintln!("Rolling through logs {:?} -- {:?} took {:.3}s", start, end, t.elapsed().as_secs_f64());
}
