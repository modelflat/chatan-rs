use super::message::{Message, Messages};
use super::util::day_after;
use chrono::{Utc, DateTime, Date};
use counter::Counter;

const SECS_PER_DAY: i64 = 24 * 60 * 60;

/// Represents an error occured when sliding through `DailyChatLog`
pub enum SlideError {
    InvalidTimeInterval,
    NotEnoughData,
}

/// Represents statistics computed over window of tokenized messages
pub struct WindowStats<'a> {
    pub n_messages: u64,
    pub n_tokens: u64,
    pub n_tokens_filtered: u64,
    pub token_counts: Counter<&'a str, u64>
}

/// Represents daily chat log, i.e. chat log where data is stored in per-day files.
/// TODO generalize to any time interval as a unit,
/// also fetching one day at a time is probably the most effective approach for step <= 1d
pub trait DailyChatLog {

    /// Range of dates in this chatlog, or None if it is empty
    fn range(&self) -> Option<(Date<Utc>, Date<Utc>)>;

    /// Load data for a given date.
    fn load(&mut self, date: &Date<Utc>) -> Option<Messages>;

    /// Iterate over the time interval within the index, using given step, sliding
    /// window of given size and window function F.
    ///
    /// Returns an `SlideStatus::InvalidTimeInterval` if start or end time is outside of the index,
    /// if start > end or if window size > index size.
    ///
    /// Returns `SlideStatus::NotEnoughDataInIndex` if index is empty
    fn slide<F>(
        &mut self, start: DateTime<Utc>, end: DateTime<Utc>, step: u32, size: u32,
        window_fn: F
    ) -> Result<(), SlideError>
        where
            F: FnMut(&DateTime<Utc>, &DateTime<Utc>, &mut dyn Iterator<Item=&Message>) -> ()
    {
        // avoid putting mut into function signature
        let mut f = window_fn;

        let index_size = match self.range() {
            Some((t0, t1)) => day_after(t1) - t0,
            None => {
                return Err(SlideError::NotEnoughData);
            }
        };
        let size = chrono::Duration::seconds(size as i64);
        let step = chrono::Duration::seconds(step as i64);

        if start > end || (end - start) > index_size {
            return Err(SlideError::InvalidTimeInterval);
        }

        if size > index_size {
            return Err(SlideError::NotEnoughData);
        }

        let mut cur = start;

        // can be made more generic
        let n_units = size.num_seconds() / SECS_PER_DAY + if size.num_seconds() % SECS_PER_DAY == 0 { 0 } else { 1 };
        let mut loaded_files: Vec<(Date<Utc>, Messages)> = Vec::with_capacity((n_units * 2) as usize);

        while cur + size <= end {
            let cur_start = cur;
            let cur_end = cur + size;
            let cur_date = cur.date();

            // unload files that are no longer needed
            loaded_files.drain_filter(|e| e.0 < cur_date);

            let mut date = match loaded_files.last() {
                Some(v) => {
                    // files up to this date were loaded.
                    day_after(v.0)
                },
                None => {
                    // no files loaded yet
                    cur_date
                }
            };

            while date < (cur + size + step).date() {
                loaded_files.push((date, self.load(&date).unwrap_or_else(|| Messages::empty())));
                date = day_after(date);
            }

            let mut window = loaded_files
                .iter()
                .flat_map(|(_, msgs)| msgs.temporal_slice(&cur_start, &cur_end).iter());

            f(&cur_start, &cur_end, &mut window);

            cur = cur + step;
        }

        Ok(())
    }

    fn slide_token_counts<F, Filter>(
        &mut self, start: DateTime<Utc>, end: DateTime<Utc>, step: u32, size: u32,
        f: F, filter: Filter
    ) -> Result<(), SlideError>
        where
            F: FnMut(&DateTime<Utc>, &DateTime<Utc>, WindowStats) -> (),
            Filter: Fn(&str) -> bool
    {
        let mut f = f;
        self.slide(start, end, step, size, |t0, t1, win| {
            let mut total: u64 = 0;
            let mut total_filtered: u64 = 0;
            let mut total_msgs: u64 = 0;

            let counter: Counter<&str, u64> = win
                .flat_map(|msg| {
                    total_msgs += 1;
                    msg.message().split_ascii_whitespace()
                })
                .filter(|tok| {
                    total += 1;
                    if filter(tok) {
                        total_filtered += 1;
                        true
                    } else {
                        false
                    }
                })
                .collect();

            f(t0, t1, WindowStats {
                n_messages: total_msgs,
                n_tokens: total,
                n_tokens_filtered: total_filtered,
                token_counts: counter
            });
        })
    }

}