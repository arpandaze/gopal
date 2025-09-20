use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local, TimeZone, Utc};
use clap::{Parser, Subcommand};
use serde_json;
use std::path::PathBuf;

use gopal::database::{Database, ListeningStats};

#[derive(Parser)]
#[command(name = "gopal-cli")]
#[command(about = "Query music listening statistics")]
#[command(version = "0.1.0")]
struct Args {
    /// Path to the SQLite database file
    #[arg(short, long, default_value = "~/.local/share/gopal/music.db")]
    database: String,

    /// Output format
    #[arg(short, long, default_value = "human")]
    format: OutputFormat,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Debug, clap::ValueEnum)]
enum OutputFormat {
    Human,
    Json,
    Csv,
}

#[derive(Subcommand)]
enum Commands {
    /// Show listening statistics for a time period
    Stats {
        /// Time period to analyze
        #[arg(short, long, default_value = "week")]
        period: TimePeriod,

        /// Custom start date (YYYY-MM-DD format, used with 'custom' period)
        #[arg(long)]
        start_date: Option<String>,

        /// Custom end date (YYYY-MM-DD format, used with 'custom' period)
        #[arg(long)]
        end_date: Option<String>,

        /// Limit number of results for top lists
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },

    /// Show top tracks
    TopTracks {
        /// Time period to analyze
        #[arg(short, long, default_value = "week")]
        period: TimePeriod,

        /// Number of tracks to show
        #[arg(short, long, default_value = "20")]
        limit: usize,

        /// Sort by listening time or play count
        #[arg(short, long, default_value = "time")]
        sort_by: SortBy,
    },

    /// Show top artists
    TopArtists {
        /// Time period to analyze
        #[arg(short, long, default_value = "week")]
        period: TimePeriod,

        /// Number of artists to show
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Show listening history
    History {
        /// Time period to analyze
        #[arg(short, long, default_value = "today")]
        period: TimePeriod,

        /// Number of sessions to show
        #[arg(short, long, default_value = "50")]
        limit: usize,
    },

    /// Show current database status
    Status,
}

#[derive(Clone, Debug, clap::ValueEnum)]
enum TimePeriod {
    Today,
    Week,
    Month,
    Year,
    AllTime,
    Custom,
}

#[derive(Clone, Debug, clap::ValueEnum)]
enum SortBy {
    Time,
    Count,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Resolve database path
    let db_path = expand_path(&args.database)?;

    // Check if database exists
    if !db_path.exists() {
        eprintln!("Database not found at: {}", db_path.display());
        eprintln!("Make sure the gopald daemon has been running to collect data.");
        std::process::exit(1);
    }

    // Initialize database
    let database = Database::new(&db_path)
        .context("Failed to open database")?;

    match args.command {
        Commands::Stats { period, start_date, end_date, limit } => {
            let (start_time, end_time) = parse_time_period(period, start_date, end_date)?;
            let stats = database.get_listening_stats(start_time, end_time)?;
            
            match args.format {
                OutputFormat::Human => print_stats_human(&stats, limit),
                OutputFormat::Json => print_stats_json(&stats)?,
                OutputFormat::Csv => print_stats_csv(&stats)?,
            }
        }

        Commands::TopTracks { period, limit, sort_by } => {
            let (start_time, end_time) = parse_time_period(period, None, None)?;
            let stats = database.get_listening_stats(start_time, end_time)?;
            
            let mut tracks = stats.top_tracks;
            if matches!(sort_by, SortBy::Count) {
                tracks.sort_by(|a, b| b.play_count.cmp(&a.play_count));
            }
            tracks.truncate(limit);

            match args.format {
                OutputFormat::Human => print_top_tracks_human(&tracks, &sort_by),
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&tracks)?),
                OutputFormat::Csv => print_top_tracks_csv(&tracks)?,
            }
        }

        Commands::TopArtists { period, limit } => {
            let (start_time, end_time) = parse_time_period(period, None, None)?;
            let stats = database.get_listening_stats(start_time, end_time)?;
            
            let mut artists = stats.top_artists;
            artists.truncate(limit);

            match args.format {
                OutputFormat::Human => print_top_artists_human(&artists),
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&artists)?),
                OutputFormat::Csv => print_top_artists_csv(&artists)?,
            }
        }

        Commands::History { period, limit } => {
            let (start_time, end_time) = parse_time_period(period, None, None)?;
            let stats = database.get_listening_stats(start_time, end_time)?;
            
            let mut history = stats.listening_history;
            history.truncate(limit);

            match args.format {
                OutputFormat::Human => print_history_human(&history),
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&history)?),
                OutputFormat::Csv => print_history_csv(&history)?,
            }
        }

        Commands::Status => {
            print_status(&database)?;
        }
    }

    Ok(())
}

fn parse_time_period(
    period: TimePeriod,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<(Option<i64>, Option<i64>)> {
    let now = Local::now();

    match period {
        TimePeriod::Today => {
            let start_of_day = now.date_naive().and_hms_opt(0, 0, 0).unwrap();
            let start_timestamp = Local.from_local_datetime(&start_of_day).unwrap().timestamp();
            Ok((Some(start_timestamp), None))
        }

        TimePeriod::Week => {
            let start_of_week = now - Duration::days(7);
            Ok((Some(start_of_week.timestamp()), None))
        }

        TimePeriod::Month => {
            let start_of_month = now - Duration::days(30);
            Ok((Some(start_of_month.timestamp()), None))
        }

        TimePeriod::Year => {
            let start_of_year = now - Duration::days(365);
            Ok((Some(start_of_year.timestamp()), None))
        }

        TimePeriod::AllTime => Ok((None, None)),

        TimePeriod::Custom => {
            let start_timestamp = if let Some(start_str) = start_date {
                let start_date = chrono::NaiveDate::parse_from_str(&start_str, "%Y-%m-%d")
                    .context("Invalid start date format. Use YYYY-MM-DD")?;
                let start_datetime = start_date.and_hms_opt(0, 0, 0).unwrap();
                Some(Local.from_local_datetime(&start_datetime).unwrap().timestamp())
            } else {
                None
            };

            let end_timestamp = if let Some(end_str) = end_date {
                let end_date = chrono::NaiveDate::parse_from_str(&end_str, "%Y-%m-%d")
                    .context("Invalid end date format. Use YYYY-MM-DD")?;
                let end_datetime = end_date.and_hms_opt(23, 59, 59).unwrap();
                Some(Local.from_local_datetime(&end_datetime).unwrap().timestamp())
            } else {
                None
            };

            Ok((start_timestamp, end_timestamp))
        }
    }
}

fn print_stats_human(stats: &ListeningStats, limit: usize) {
    println!("🎵 Music Listening Statistics");
    println!("═══════════════════════════════");
    println!();

    // Total listening time
    let total_hours = stats.total_listening_time as f64 / 3600.0;
    println!("📊 Total Listening Time: {:.1} hours ({} minutes)", 
             total_hours, stats.total_listening_time / 60);
    println!();

    // Top tracks
    if !stats.top_tracks.is_empty() {
        println!("🎵 Top Tracks (by listening time):");
        for (i, track_stat) in stats.top_tracks.iter().take(limit).enumerate() {
            let time_str = format_duration(track_stat.total_listened_time);
            println!("  {}. {} - {} ({}, {} plays)",
                     i + 1,
                     track_stat.track.title,
                     track_stat.track.artist,
                     time_str,
                     track_stat.play_count);
        }
        println!();
    }

    // Top artists
    if !stats.top_artists.is_empty() {
        println!("🎤 Top Artists (by listening time):");
        for (i, artist_stat) in stats.top_artists.iter().take(limit).enumerate() {
            let time_str = format_duration(artist_stat.total_listened_time);
            println!("  {}. {} ({}, {} tracks)",
                     i + 1,
                     artist_stat.artist,
                     time_str,
                     artist_stat.track_count);
        }
        println!();
    }

    // Recent listening
    if !stats.listening_history.is_empty() {
        println!("🕒 Recent Listening:");
        for session in stats.listening_history.iter().take(5) {
            let datetime = DateTime::<Local>::from(
                DateTime::<Utc>::from_timestamp(session.session.start_time, 0).unwrap()
            );
            let time_str = format_duration(session.session.listened_time.unwrap_or(0));
            println!("  {} - {} ({}) [{}]",
                     session.track.title,
                     session.track.artist,
                     time_str,
                     datetime.format("%Y-%m-%d %H:%M"));
        }
    }
}

fn print_stats_json(stats: &ListeningStats) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(stats)?);
    Ok(())
}

fn print_stats_csv(stats: &ListeningStats) -> Result<()> {
    println!("type,name,value");
    println!("total_time,Total Listening Time,{}", stats.total_listening_time);
    
    for track_stat in &stats.top_tracks {
        println!("track,\"{} - {}\",{}", 
                 track_stat.track.title, 
                 track_stat.track.artist, 
                 track_stat.total_listened_time);
    }
    
    for artist_stat in &stats.top_artists {
        println!("artist,\"{}\",{}", 
                 artist_stat.artist, 
                 artist_stat.total_listened_time);
    }
    
    Ok(())
}

fn print_top_tracks_human(tracks: &[gopal::database::TrackStats], sort_by: &SortBy) {
    let sort_desc = match sort_by {
        SortBy::Time => "listening time",
        SortBy::Count => "play count",
    };
    
    println!("🎵 Top Tracks (by {}):", sort_desc);
    println!("═══════════════════════════");
    
    for (i, track_stat) in tracks.iter().enumerate() {
        let time_str = format_duration(track_stat.total_listened_time);
        println!("{}. {} - {}", i + 1, track_stat.track.title, track_stat.track.artist);
        println!("   {} listened, {} plays", time_str, track_stat.play_count);
        println!();
    }
}

fn print_top_tracks_csv(tracks: &[gopal::database::TrackStats]) -> Result<()> {
    println!("rank,title,artist,album,listened_time,play_count");
    for (i, track_stat) in tracks.iter().enumerate() {
        println!("{},\"{}\",\"{}\",\"{}\",{},{}", 
                 i + 1,
                 track_stat.track.title,
                 track_stat.track.artist,
                 track_stat.track.album,
                 track_stat.total_listened_time,
                 track_stat.play_count);
    }
    Ok(())
}

fn print_top_artists_human(artists: &[gopal::database::ArtistStats]) {
    println!("🎤 Top Artists:");
    println!("═══════════════");
    
    for (i, artist_stat) in artists.iter().enumerate() {
        let time_str = format_duration(artist_stat.total_listened_time);
        println!("{}. {}", i + 1, artist_stat.artist);
        println!("   {} listened, {} tracks", time_str, artist_stat.track_count);
        println!();
    }
}

fn print_top_artists_csv(artists: &[gopal::database::ArtistStats]) -> Result<()> {
    println!("rank,artist,listened_time,track_count");
    for (i, artist_stat) in artists.iter().enumerate() {
        println!("{},\"{}\",{},{}", 
                 i + 1,
                 artist_stat.artist,
                 artist_stat.total_listened_time,
                 artist_stat.track_count);
    }
    Ok(())
}

fn print_history_human(history: &[gopal::database::SessionWithMetadata]) {
    println!("🕒 Listening History:");
    println!("═══════════════════");
    
    for session in history {
        let datetime = DateTime::<Local>::from(
            DateTime::<Utc>::from_timestamp(session.session.start_time, 0).unwrap()
        );
        let time_str = format_duration(session.session.listened_time.unwrap_or(0));
        
        println!("{} - {}", session.track.title, session.track.artist);
        println!("   {} on {} [{}]",
                 time_str,
                 datetime.format("%Y-%m-%d %H:%M"),
                 session.player.name);
        println!();
    }
}

fn print_history_csv(history: &[gopal::database::SessionWithMetadata]) -> Result<()> {
    println!("timestamp,title,artist,album,listened_time,player");
    for session in history {
        println!("{},\"{}\",\"{}\",\"{}\",{},\"{}\"", 
                 session.session.start_time,
                 session.track.title,
                 session.track.artist,
                 session.track.album,
                 session.session.listened_time.unwrap_or(0),
                 session.player.name);
    }
    Ok(())
}

fn print_status(database: &Database) -> Result<()> {
    println!("📊 Database Status:");
    println!("═══════════════════");
    
    match database.get_database_stats() {
        Ok(stats) => {
            println!("Database file: Available");
            println!("Total sessions: {}", stats.total_sessions);
            println!("Active sessions: {}", stats.active_sessions);
            println!("Total tracks: {}", stats.total_tracks);
            println!("Total players: {}", stats.total_players);
            
            if stats.active_sessions > 0 {
                println!();
                println!("⚠️  Warning: {} active sessions found.", stats.active_sessions);
                println!("   This may indicate the daemon was not properly shut down.");
                println!("   These will be cleaned up on next daemon start.");
            }
        }
        Err(e) => {
            println!("Error reading database stats: {}", e);
        }
    }
    
    println!();
    println!("Use 'music-cli stats' to view listening statistics.");
    Ok(())
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
        let path = expand_path("/tmp/test.db").unwrap();
        assert_eq!(path, PathBuf::from("/tmp/test.db"));
    }

    #[test]
    fn test_parse_today_period() {
        let (start, end) = parse_time_period(TimePeriod::Today, None, None).unwrap();
        assert!(start.is_some());
        assert!(end.is_none());
    }

    #[test]
    fn test_parse_all_time_period() {
        let (start, end) = parse_time_period(TimePeriod::AllTime, None, None).unwrap();
        assert!(start.is_none());
        assert!(end.is_none());
    }

    #[test]
    fn test_parse_custom_period() {
        let (start, end) = parse_time_period(
            TimePeriod::Custom, 
            Some("2023-01-01".to_string()), 
            Some("2023-12-31".to_string())
        ).unwrap();
        assert!(start.is_some());
        assert!(end.is_some());
    }
}

fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{} sec", seconds)
    } else {
        let minutes = seconds / 60;
        let remaining_seconds = seconds % 60;
        if remaining_seconds == 0 {
            format!("{} min", minutes)
        } else {
            format!("{} min {} sec", minutes, remaining_seconds)
        }
    }
}