#![feature(test)]
#![feature(duration_float)]
#![feature(drain_filter)]
#![feature(fn_traits)]
#![feature(unboxed_closures)]

mod chatan;
use chatan::overrustle;
use chatan::message::Message;
use chrono::{DateTime, Utc};
use reqwest::Client;

use crate::chatan::overrustle::LocalChannelLogs;
use std::path::PathBuf;
use std::str::FromStr;
use chrono::offset::TimeZone;
use std::time::Duration;

struct Window {
    first: bool
}

impl Window {
    fn new() -> Window {
        Window {
            first: true
        }
    }
}

impl overrustle::WindowFn for Window {
    fn window_start(&mut self, t0: &DateTime<Utc>, t1: &DateTime<Utc>) -> () {
//        eprintln!("Started window {:?} -- {:?}", t0, t1);
        self.first = true;
    }

    fn window_end(&mut self, t0: &DateTime<Utc>, t1: &DateTime<Utc>) -> () {
//        eprintln!("Finished window {:?} -- {:?}", t0, t1);
    }

    fn call(&mut self, msg: &Message) {
        if self.first {
//            eprintln!("{:?}", msg);
            self.first = false;
        }
    }
}

fn main() {
    let client = Client::new();

    let mut local_logs = LocalChannelLogs::from_path(
        &client, "forsen".to_string(), PathBuf::from_str(".").unwrap()
    ).unwrap();

    local_logs.print_info();
    local_logs.download(&client, true);
    local_logs.print_info();

    let start_date = &Utc.ymd(2019, 1, 1);
    let end_date = &Utc.ymd(2019, 1, 30);

    let start = start_date.and_hms(0, 0, 0);
    let end = end_date.and_hms(0, 0, 0);
    let step = Duration::from_secs((2.3 * 86400f64) as u64);
    let size = Duration::from_secs((5.4234 * 86400f64) as u64);

    let t = std::time::Instant::now();

    let mut window = Window::new();

    local_logs.roll(start, end, step, size, &mut window);

    eprintln!("Rolling through logs {:?} -- {:?} took {:.3}s", start, end, t.elapsed().as_secs_f64());
}
