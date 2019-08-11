#![feature(test)]
#![feature(drain_filter)]
#![feature(fn_traits)]
#![feature(unboxed_closures)]
#![feature(result_map_or_else)]

#[macro_use]
extern crate runtime_fmt;
extern crate test;

pub mod message;
pub mod chatlog;
pub mod emote_index;
pub mod util;

pub mod overrustle;
