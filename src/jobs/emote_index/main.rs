extern crate chatan;

use chatan::emote_index;
use chatan::emote_index::{EmoteProvider, update_index_in_path};
use reqwest::Client;
use std::path::Path;

fn main() {
    let client = Client::new();

    let channels = vec![
        "forsen".to_string(),
        "nymn".to_string(),
    ];

    let twitch_client_id = std::env::var("TWITCH_CLIENT_ID")
        .expect("Set TWITCH_CLIENT_ID env var to your client id");

    let providers: Vec<Box<dyn EmoteProvider>> = vec![
        Box::new(emote_index::TwitchMetrics::new(twitch_client_id)),
        Box::new(emote_index::BetterTTV::new()),
        Box::new(emote_index::FrankerFaceZ::new()),
    ];

    let output_path = Path::new("index.json");

    update_index_in_path(&client, &output_path, channels, providers)
        .expect("Could not update index in path");
}