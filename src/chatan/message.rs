use chrono::{DateTime, Utc};
use humantime::parse_rfc3339_weak;

#[derive(Debug)]
pub struct Message {
    pub time: DateTime<Utc>,
    // We could avoid allocations whatsoever by using &'a str here but this conflicts with
    // the rolling code and I don't know yet how to do this without diving into unsafe Rust
    pub user: String, // &'a str
    pub message: String, // &'a str
}

impl Message {
    pub fn from_triple(time: &str, user: &str, message: &str) -> Option<Message> {
        if let Ok(time) = {
            parse_rfc3339_weak(time).map(|t| DateTime::<Utc>::from(t))
        } {
            Some(Message { time, user: user.to_owned(), message: message.to_owned() })
        } else {
            None
        }
    }
}

fn parse_line(line: &str) -> Option<Message> {
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

        Message::from_triple(ts, user, message)
    } else {
        None
    }
}

pub fn parse_string(s: &String) -> Vec<Message> {
    s.split_terminator('\n').filter_map(|line| parse_line(line)).collect()
}

pub mod tokenizer {

    pub fn normal(text: &str) -> std::str::SplitWhitespace {
        text.split_whitespace()
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use test::Bencher;

    #[test]
    fn test_parse_line() {
        let line = "[2019-07-01 00:00:42 UTC] someuser: FeelsGoodMan WE FeelsGoodMan ARE FeelsGoodMan READY FeelsGoodMan";
        match parse_line(line) {
            Some(m) => {
                assert_eq!(m.user, "someuser");
                assert_eq!(m.message, "FeelsGoodMan WE FeelsGoodMan ARE FeelsGoodMan READY FeelsGoodMan");
                assert_eq!(
                    m.time,
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
