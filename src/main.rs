#![feature(test)]
#![feature(duration_float)]

mod chatan;
use chatan::overrustle;
use reqwest::Client;

use crate::chatan::overrustle::LocalChannelLogs;
use std::path::PathBuf;
use std::str::FromStr;
use chrono::Utc;
use chrono::offset::TimeZone;

fn main() {
    let client = Client::new();

    let mut local_logs = LocalChannelLogs::from_path(
        &client, "forsen".to_string(), PathBuf::from_str(".").unwrap()
    ).unwrap();
    local_logs.print_info();
    local_logs.download(&client, true);

    let t = std::time::Instant::now();
    let start_date = &Utc.ymd(2018, 12, 1);
    let end_date = &Utc.ymd(2019, 2, 1);

    let res = local_logs.load_date_range(start_date, end_date).unwrap();
    let spent_loading = t.elapsed().as_secs_f64();

    let messages = overrustle::parse_files(&res);
    let spent_parsing = t.elapsed().as_secs_f64() - spent_loading;

    println!("Loaded {} messages for interval {:?} -- {:?} in {:.3} s [{:.3}s disk read / {:.3}s parsing]",
        messages.len(), start_date, end_date, t.elapsed().as_secs_f64(),
        spent_loading, spent_parsing

    );

    println!("{:#?}", &messages[..1]);
}
