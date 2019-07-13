use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Message<'a> {
    pub time: DateTime<Utc>,
    pub user: &'a str,
    pub message: &'a str
}
//
//impl Message<'_> {
//    pub fn pre_serialize(self) -> SerializableMessage {
//        SerializableMessage {
//            time: self.time.timestamp(),
//            user: self.user.to_string(),
//            message: self.message.to_string()
//        }
//    }
//}
//
//#[derive(Debug)]
//pub struct SerializableMessage {
//    pub time: i64,
//    pub user: String,
//    pub message: String
//}
