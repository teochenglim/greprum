//! GrepRUM: product analytics and RUM in one Rust binary, on GreptimeDB.
#![forbid(unsafe_code)]

pub mod collect;
pub mod config;
pub mod geoip;
pub mod health;
pub mod jobs;
pub mod metrics;
pub mod migrate;
pub mod model;
pub mod query;
pub mod server;
pub mod sink;
pub mod soak;
pub mod spike;
pub mod spool;
pub mod synth;

/// Build version, stamped by `GREPRUM_VERSION` at compile time.
pub const VERSION: &str = match option_env!("GREPRUM_VERSION") {
    Some(v) => v,
    None => concat!("v", env!("CARGO_PKG_VERSION")),
};
