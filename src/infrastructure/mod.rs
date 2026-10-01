use std::time::Duration;

pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(15);
pub const IO_TIMEOUT: Duration = Duration::from_secs(7);

pub mod crypto;
pub mod logging;
pub mod lua;
pub mod script_catalog;
pub mod session_repository;
pub mod telegram_auth;
