use anyhow::{Context, Result};
use clap::Parser;
use log::{error, info};
use std::path::PathBuf;
use tokio::signal;

mod config;
use config::Config;

// Import modules from the parent src directory
use gopal::database::Database;
use gopal::mpris_monitor::MprisMonitor;

#[derive(Parser)]
#[command(name = "gopald")]
#[command(about = "Music listening tracker daemon")]
#[command(version = "0.1.0")]
struct Args {
    /// Path to the SQLite database file
    #[arg(short, long, default_value = "~/.local/share/musicd/music.db")]
    database: String,

    /// Configuration file path
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Run in foreground (don't daemonize)
    #[arg(short, long)]
    foreground: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let log_level = if args.verbose {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    env_logger::Builder::from_default_env()
        .filter_level(log_level)
        .init();

    info!("Starting gopald v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration
    let _config = Config::load(args.config.as_deref())?;
    
    // Resolve database path (handle ~ expansion)
    let db_path = expand_path(&args.database)?;
    
    // Ensure database directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .context("Failed to create database directory")?;
    }

    // Initialize database
    let database = Database::new(&db_path)
        .context("Failed to initialize database")?;

    info!("Database initialized at: {}", db_path.display());

    // Clean up orphaned sessions from previous runs
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    
    let max_session_duration = 24 * 3600; // 24 hours - generous limit for long listening sessions
    let orphaned_count = database.cleanup_orphaned_sessions(current_time, max_session_duration)
        .context("Failed to cleanup orphaned sessions")?;
    
    if orphaned_count > 0 {
        info!("Cleaned up {} orphaned sessions from previous runs", orphaned_count);
    }

    // Initialize MPRIS monitor
    let mut monitor = MprisMonitor::new(database)
        .context("Failed to initialize MPRIS monitor")?;

    // Set up graceful shutdown
    let shutdown_signal = setup_shutdown_handler();

    info!("Music daemon started successfully");

    // Wait for shutdown signal or run monitoring
    tokio::select! {
        _ = shutdown_signal => {
            info!("Received shutdown signal, stopping daemon...");
        }
        result = monitor.start_monitoring() => {
            if let Err(e) = result {
                error!("MPRIS monitoring failed: {}", e);
            }
        }
    }

    info!("Music daemon stopped");
    Ok(())
}

async fn setup_shutdown_handler() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

fn expand_path(path: &str) -> Result<PathBuf> {
    if path.starts_with('~') {
        let home = std::env::var("HOME")
            .context("HOME environment variable not set")?;
        Ok(PathBuf::from(path.replacen('~', &home, 1)))
    } else {
        Ok(PathBuf::from(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_path() {
        // Test regular path
        let path = expand_path("/tmp/test.db").unwrap();
        assert_eq!(path, PathBuf::from("/tmp/test.db"));

        // Test relative path
        let path = expand_path("./test.db").unwrap();
        assert_eq!(path, PathBuf::from("./test.db"));
    }

    #[test]
    fn test_expand_home_path() {
        std::env::set_var("HOME", "/home/testuser");
        let path = expand_path("~/.local/share/test.db").unwrap();
        assert_eq!(path, PathBuf::from("/home/testuser/.local/share/test.db"));
    }
}