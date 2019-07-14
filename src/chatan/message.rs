use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Message<'a> {
    pub time: DateTime<Utc>,
    pub user: &'a str,
    pub message: &'a str
}
