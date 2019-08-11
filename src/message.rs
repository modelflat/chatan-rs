use chrono::{DateTime, Utc};

#[derive(Debug)]
pub struct Message {
    timestamp: DateTime<Utc>,
    user: *const str,
    message: *const str,
}

impl Message {
    /// This method should not be public to prevent constructor misuse
    fn new(timestamp: DateTime<Utc>, user: &str, message: &str) -> Message {
        Message { timestamp, user: user as *const str, message: message as *const str }
    }

    #[inline(always)]
    pub fn message(&self) -> &str {
        // This is safe because `Message` can only be constructed inside of `Messages` struct
        unsafe { &*self.message }
    }

    #[inline(always)]
    pub fn user(&self) -> &str {
        // This is safe because `Message` can only be constructed inside of `Messages` struct
        unsafe { &*self.user }
    }

    #[inline(always)]
    pub fn timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        write!(
            f,
            "[{ts:?}] {user}: {message}",
            ts = self.timestamp(),
            user = self.user(),
            message = self.message(),
        )
    }
}

pub struct Messages {
    /// This data is a base for all the messages in this struct, i.e. any `Message`
    /// in `self.messages` will have pointers to this
    data: String,
    messages: Vec<Message>,
}

impl Messages {

    pub fn from_string<Parser: Fn(&str) -> Option<(DateTime<Utc>, &str, &str)>>(
        data: String, parser: Parser, sort_messages: bool
    ) -> Self {
        let mut res = Messages { data, messages: Vec::new() };
        res.messages = res.data.split_terminator('\n').filter_map(
            |line| {
                let (ts, us, ms) = parser(line)?;
                Some(Message::new(ts, us, ms))
            }
        ).collect();
        if sort_messages {
            res.messages.sort_unstable_by_key(|m| m.timestamp);
        }
        res
    }

    pub fn empty() -> Self {
        Messages {
            data: String::new(),
            messages: Vec::new()
        }
    }

    pub fn vec(&self) -> &Vec<Message> {
        &self.messages
    }

    /// Retrieves a slice of messages falling into specified time interval.
    pub fn temporal_slice(&self, t0: &DateTime<Utc>, t1: &DateTime<Utc>) -> &[Message] {
        if self.messages.is_empty() {
            &self.messages
        } else {
            let first = self.messages.first().unwrap();
            let last = self.messages.last().unwrap();

            let start_idx = {
                if *t0 <= first.timestamp() {
                    0
                } else {
                    match self.messages.binary_search_by_key(t0, |m| m.timestamp) {
                        Ok(x) => x, Err(x) => x
                    }
                }
            };

            let end_idx = {
                if *t1 >= last.timestamp() {
                    self.messages.len()
                } else {
                    match self.messages.binary_search_by_key(t1, |m| m.timestamp) {
                        Ok(x) => x, Err(x) => x
                    }
                }
            };

            &self.messages[start_idx..end_idx]
        }
    }

}

pub mod overrustle {
    use super::*;
    use humantime::parse_rfc3339_weak;

    pub(crate) fn parse_line(line: &str) -> Option<(DateTime<Utc>, &str, &str)> {
        // According to OverRustle log structure:
        //$[2019-07-01 00:00:42 UTC] someuser: ...
        // ^                   ^     ^
        // 1                  20    26
        // Lets hard-code this to avoid searching for the first ']'. This helps to save
        // ~18-20% of time per call (~15ns per message on my laptop in particular)
        const TS_START: usize = 1;
        const TS_END: usize = 20;
        const USER_START: usize = 26;

        // check 0
        if line.len() > USER_START {
            // Unsafe calls to save several ns.

            // 1. Safe due to check 0
            let user_end = TS_END + unsafe {
                line.get_unchecked(TS_END..line.len() - 1).find(':')?
            };

            // 2. Safe due to check 0
            let ts = unsafe {
                line.get_unchecked(TS_START..TS_END)
            };

            // 3. Safe due to check 0 and stmt 1
            let user = unsafe {
                line.get_unchecked(USER_START..user_end)
            };

            // 4. Safe due to stmt 1
            let message = unsafe {
                line.get_unchecked(user_end + 2..line.len())
            };

            if let Ok(time) = {
                parse_rfc3339_weak(ts).map(|t| DateTime::<Utc>::from(t))
            } {
                Some((time, user, message))
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn parse_string(s: String) -> Messages {
        // messages on overrustle are already sorted by timestamp, so
        // no need to ensure sorting (artifacts should be negligible)
        Messages::from_string(s, parse_line, false)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use test::Bencher;

        #[test]
        fn test_parse_line() {
            let line = "[2019-07-01 00:00:42 UTC] someuser: FeelsGoodMan WE FeelsGoodMan ARE FeelsGoodMan READY FeelsGoodMan";
            match parse_line(line) {
                Some((ts, usr, msg)) => {
                    assert_eq!(usr, "someuser");
                    assert_eq!(msg, "FeelsGoodMan WE FeelsGoodMan ARE FeelsGoodMan READY FeelsGoodMan");
                    assert_eq!(
                        ts,
                        chrono::DateTime::<Utc>::from(
                            humantime::parse_rfc3339_weak("2019-07-01 00:00:42").unwrap()
                        )
                    );
                },
                None => assert!(false, "Message should parse correctly")
            }
        }

        #[bench]
        fn bench_parse_line(b: &mut Bencher) {
            let line = "[2019-07-01 00:00:42 UTC] someuser: message";
            b.iter(|| parse_line(line))
        }
    }
}
