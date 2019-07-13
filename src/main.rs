#![feature(test)]
#![feature(duration_float)]

mod chatan;
use chatan::overrustle;
use reqwest::Client;

use std::time;

fn main() {
    let client = Client::new();
    println!("Getting urls...");
    let urls = overrustle::get_all_urls_for_channel(&client, "forsen".to_string());
    println!("Found urls: {}. Getting files...", urls.len());
    let files = overrustle::fetch_files(&client, &urls[..1]);
    println!("Downloaded files. Parsing messages...");

    let t = time::Instant::now();
    let messages = overrustle::parse_files(&files);
    println!("Messages parsed: {} / {:.3}s spent", messages.len(), t.elapsed().as_secs_f64());

    println!("First message: {:?}", messages.first().unwrap());
    println!("Last message: {:?}", messages.last().unwrap());
}
