#![forbid(unsafe_code)]
pub mod config;
pub mod connection;
pub mod country;
pub mod monitor;
pub mod wire;
pub type Result<T> = std::result::Result<T, &'static str>;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
