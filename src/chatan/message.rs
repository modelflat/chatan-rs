use chrono::{DateTime, Utc};

#[derive(Debug)]
pub struct Message<'a> {
    pub time: DateTime<Utc>,
    pub user: &'a str,
    pub message: &'a str
}
