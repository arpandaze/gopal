//! Music Listening Tracker Library
//! 
//! A Rust library for tracking music listening sessions via MPRIS on Linux.
//! This library provides components for monitoring media players, tracking sessions,
//! and storing listening data in a SQLite database.

pub mod database;
pub mod mpris_monitor;
pub mod session_tracker;

pub use database::{Database, Track, Player, Session, ListeningStats, DatabaseStats};
pub use mpris_monitor::MprisMonitor;
pub use session_tracker::{SessionTracker, SessionEvent};

/// Current version of the music tracker
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default database path relative to home directory
pub const DEFAULT_DB_PATH: &str = "~/.local/share/musicd/music.db";

/// Default configuration directory
pub const DEFAULT_CONFIG_DIR: &str = "~/.config/musicd";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_exists() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_constants() {
        assert!(DEFAULT_DB_PATH.contains("musicd"));
        assert!(DEFAULT_CONFIG_DIR.contains("musicd"));
    }
}