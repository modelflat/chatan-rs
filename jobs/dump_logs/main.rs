use chatan::overrustle::{DataLoadMode, OverRustleLogs};

use chatan::util::*;

fn main() {
    let path = "D:\\overrustle-dump";

    let channels: Vec<String> = std::env::args().skip(1).collect();

    progress_bar(true);

    for channel in channels.into_iter() {
        let logs = OverRustleLogs::make_and_sync(path.into(), channel, DataLoadMode::PrefetchAndCache);
        println!("{}", logs);
    }
}
