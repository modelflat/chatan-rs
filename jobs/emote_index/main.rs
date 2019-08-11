extern crate structopt;
use structopt::StructOpt;

extern crate chatan;
use chatan::overrustle::{DataLoadMode, OverRustleLogs};
use chatan::emote_index;
use chatan::emote_index::{load_index, EmoteProvider, update_index_in_path};
use chatan::chatlog::DailyChatLog;

use std::path::PathBuf;
use chrono::{DateTime, Utc};
use reqwest::Client;

use crate::chatan::emote_index::*;
use crate::chatan::util;

use std::collections::HashSet;


#[derive(Debug, StructOpt)]
#[structopt(about = "Create emote index")]
struct EmoteIndexCLI {
    #[structopt(subcommand)]
    mode: OperationMode,
    #[structopt(name = "channel", long)]
    channel: String,
    #[structopt(name = "output", long)]
    output: PathBuf,
    #[structopt(name = "input", long)]
    input: Option<PathBuf>,
}

#[derive(Debug, StructOpt)]
enum OperationMode {
    #[structopt(name = "fetch")]
    Fetch,
    #[structopt(name = "discover")]
    Discover {
        #[structopt(name = "start", long)]
        start: Option<DateTime<Utc>>,
        #[structopt(name = "end", long)]
        end: Option<DateTime<Utc>>,
        #[structopt(name = "top", long)]
        top: u32,
        #[structopt(name = "storage", long)]
        storage: PathBuf,
        #[structopt(name = "storage-policy", long)]
        storage_policy: DataLoadMode,
    },
}

/// Uses channel logs to discover emotes being used in the channel. Useful when you
/// want to unearth emotes of the past, which won't appear in `EmoteProvider.fetch()`
/// result anymore because APIs do not emit them.
///
/// TODO parameters are mess, cleanup/split into several functions
pub fn discover_lost_emotes(
    base_index: EmoteIndex, logs: &mut OverRustleLogs, start: DateTime<Utc>, end: DateTime<Utc>, top: u32,
) -> EmoteIndex {
    let client = Client::new();

    // only providers which do not store historical emotes
    let providers: Vec<Box<dyn EmoteProvider>> = vec![
        Box::new(FrankerFaceZ::new()),
        Box::new(BetterTTV::new())
    ];

    let tokens_to_check = {
        let mut result = HashSet::new();
        let size = 86400; // 1d

        println!("Processing logs...");

        logs.slide_token_counts(
            start, end, size, size,
            |_, _, win| {
                result.extend(
                    util::most_common(win.token_counts, 1).iter()
                        .take(top as usize)
                        .map(|(s, _)| s.to_string())
                        .collect::<HashSet<String>>()
                );
            },
            |tok| 2 <= tok.len()
                && tok.len() <= 32
                && tok.chars().all(|c| c.is_ascii_alphanumeric())
                && !base_index.contains_key(tok)
        ).ok().expect("Failed to iterate through the logs");

        println!("{} popular tokens found", result.len());

        result.into_iter().collect::<Vec<String>>()
    };

    let mut emotes = EmoteIndex::new();

    for provider in &providers {
        println!("Searching provider: {}", provider.name());
        emotes.extend(
            provider.find_emotes(&client, &tokens_to_check)
        );
    }

    merge_indexes(vec![base_index, emotes])
}

fn main() {
    let opt: EmoteIndexCLI = EmoteIndexCLI::from_args();
    let client = Client::new();

    let twitch_client_id = std::env::var("TWITCH_CLIENT_ID")
        .expect("Set TWITCH_CLIENT_ID env var to your client id");

    let providers: Vec<Box<dyn EmoteProvider>> = vec![
        Box::new(emote_index::TwitchMetrics::new(twitch_client_id)),
        Box::new(emote_index::BetterTTV::new()),
        Box::new(emote_index::FrankerFaceZ::new()),
    ];

    match opt.mode {
        OperationMode::Fetch => {
            let input = opt.input.as_ref().map(|p| p.as_path());
            update_index_in_path(&client, vec![opt.channel], providers, opt.output.as_path(), input)
                .expect("Could not update index in path");
        },
        OperationMode::Discover
        { start, end, top, storage, storage_policy, .. } => {
            let mut logs = OverRustleLogs::make_and_sync(storage, opt.channel.clone(), storage_policy);
            let base_index = match opt.input {
                Some(input) => load_index(&input).expect("Could not load input index"),
                None => EmoteIndex::new()
            };
            let (log_start, log_end) = logs.range().expect("Logs are empty, can't discover anything");
            let start = start.unwrap_or(log_start.and_hms(0, 0, 0));
            let end = end.unwrap_or(log_end.and_hms(0, 0, 0));
            emote_index::save_index(
                &opt.output,
                &discover_lost_emotes(base_index, &mut logs, start, end, top)
            ).expect("Could not save index to output file");
        }
    };
}