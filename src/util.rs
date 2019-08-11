use std::time::Duration;
use chrono::Utc;
use counter::Counter;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::atomic::{AtomicBool, Ordering};

static PROGRESS_BAR_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn progress_bar(enabled: bool) {
    PROGRESS_BAR_ENABLED.store(enabled, Ordering::SeqCst)
}

pub fn most_common(counter: Counter<&str, u64>, threshold: u64) -> Vec<(&str, u64)> {
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

pub(crate) fn make_progress_bar(count: usize) -> ProgressBar {
    if !PROGRESS_BAR_ENABLED.load(Ordering::SeqCst) {
        return ProgressBar::hidden();
    }
    let bar = ProgressBar::new(count as u64);
    bar.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] [{wide_bar}] {percent}% {pos}/{len}")
            .progress_chars("=> ")
    );
    bar
}

// why doesn't Rust have this built-in?
pub(crate) fn capitalized(s: &String) -> String {
    let ss = s.clone();
    ss.get(0..1).unwrap().to_uppercase() + ss.get(1..ss.len()).unwrap()
}

pub(crate) fn day_after(date: chrono::Date<Utc>) -> chrono::Date<Utc> {
    const SECS_PER_DAY: i64 = 24 * 60 * 60;
    let day = chrono::Duration::from_std(Duration::from_secs(SECS_PER_DAY as u64))
        .expect("Cannot convert from std Duration to chrono Duration");
    date + day
}
