use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: i64,
    pub name: String,
    pub identity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub length: Option<i64>, // in microseconds
    pub art_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: i64,
    pub track_id: String,
    pub player_id: i64,
    pub start_time: i64,
    pub end_time: Option<i64>,
    pub paused_time: i64,
    pub listened_time: Option<i64>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListeningStats {
    pub total_listening_time: i64,
    pub top_tracks: Vec<TrackStats>,
    pub top_artists: Vec<ArtistStats>,
    pub listening_history: Vec<SessionWithMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackStats {
    pub track: Track,
    pub total_listened_time: i64,
    pub play_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistStats {
    pub artist: String,
    pub total_listened_time: i64,
    pub track_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionWithMetadata {
    pub session: Session,
    pub track: Track,
    pub player: Player,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path)
            .context("Failed to open database connection")?;
        
        let db = Database { conn };
        db.initialize_schema()?;
        Ok(db)
    }

    fn initialize_schema(&self) -> Result<()> {
        // Create players table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS players (
                id INTEGER PRIMARY KEY,
                name TEXT UNIQUE NOT NULL,
                identity TEXT NOT NULL
            )",
            [],
        ).context("Failed to create players table")?;

        // Create tracks table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS tracks (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                artist TEXT NOT NULL,
                album TEXT NOT NULL,
                length INTEGER,
                art_url TEXT
            )",
            [],
        ).context("Failed to create tracks table")?;

        // Create sessions table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS sessions (
                id INTEGER PRIMARY KEY,
                track_id TEXT NOT NULL,
                player_id INTEGER NOT NULL,
                start_time INTEGER NOT NULL,
                end_time INTEGER,
                paused_time INTEGER NOT NULL DEFAULT 0,
                listened_time INTEGER,
                status TEXT NOT NULL DEFAULT 'active',
                FOREIGN KEY (track_id) REFERENCES tracks (id),
                FOREIGN KEY (player_id) REFERENCES players (id)
            )",
            [],
        ).context("Failed to create sessions table")?;

        // Create indexes for better query performance
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_start_time ON sessions (start_time)",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_track_id ON sessions (track_id)",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_player_id ON sessions (player_id)",
            [],
        )?;

        Ok(())
    }

    pub fn insert_or_update_player(&self, name: &str, identity: &str) -> Result<i64> {
        // Try to insert, if it fails due to unique constraint, update and get the ID
        match self.conn.execute(
            "INSERT INTO players (name, identity) VALUES (?1, ?2)",
            params![name, identity],
        ) {
            Ok(_) => {
                Ok(self.conn.last_insert_rowid())
            }
            Err(_) => {
                // Update existing player
                self.conn.execute(
                    "UPDATE players SET identity = ?2 WHERE name = ?1",
                    params![name, identity],
                )?;
                
                // Get the player ID
                let mut stmt = self.conn.prepare("SELECT id FROM players WHERE name = ?1")?;
                let player_id: i64 = stmt.query_row(params![name], |row| row.get(0))?;
                Ok(player_id)
            }
        }
    }

    pub fn insert_or_update_track(&self, track: &Track) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO tracks (id, title, artist, album, length, art_url) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                track.id,
                track.title,
                track.artist,
                track.album,
                track.length,
                track.art_url
            ],
        )?;
        Ok(())
    }

    pub fn start_session(&self, track_id: &str, player_id: i64, start_time: i64) -> Result<i64> {
        // First check if there's already an active session for this player
        let existing_active = self.conn.query_row(
            "SELECT id FROM sessions WHERE player_id = ?1 AND status = 'active'",
            params![player_id],
            |row| row.get::<_, i64>(0)
        );
        
        if let Ok(existing_id) = existing_active {
            // Finalize the existing session first
            self.finalize_session(existing_id, start_time, "interrupted")?;
        }
        
        self.conn.execute(
            "INSERT INTO sessions (track_id, player_id, start_time, status)
             VALUES (?1, ?2, ?3, 'active')",
            params![track_id, player_id, start_time],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_session_pause_time(&self, session_id: i64, additional_pause_time: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET paused_time = paused_time + ?1 WHERE id = ?2",
            params![additional_pause_time, session_id],
        )?;
        Ok(())
    }

    pub fn finalize_session(&self, session_id: i64, end_time: i64, status: &str) -> Result<()> {
        // Calculate listened_time = (end_time - start_time - paused_time)
        self.conn.execute(
            "UPDATE sessions
             SET end_time = ?1,
                 listened_time = ?1 - start_time - paused_time,
                 status = ?2
             WHERE id = ?3",
            params![end_time, status, session_id],
        )?;
        Ok(())
    }

    pub fn update_active_session_progress(&self, session_id: i64, current_time: i64) -> Result<()> {
        // Update the progress of an active session without finalizing it
        // This allows real-time viewing of current listening progress
        self.conn.execute(
            "UPDATE sessions
             SET listened_time = ?1 - start_time - paused_time
             WHERE id = ?2 AND status = 'active'",
            params![current_time, session_id],
        )?;
        Ok(())
    }

    pub fn get_active_session_for_player(&self, player_id: i64) -> Result<Option<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, track_id, player_id, start_time, end_time, paused_time, listened_time, status
             FROM sessions 
             WHERE player_id = ?1 AND status = 'active'
             ORDER BY start_time DESC 
             LIMIT 1"
        )?;

        let session = stmt.query_row(params![player_id], |row| {
            Ok(Session {
                id: row.get(0)?,
                track_id: row.get(1)?,
                player_id: row.get(2)?,
                start_time: row.get(3)?,
                end_time: row.get(4)?,
                paused_time: row.get(5)?,
                listened_time: row.get(6)?,
                status: row.get(7)?,
            })
        });

        match session {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_listening_stats(&self, start_time: Option<i64>, end_time: Option<i64>) -> Result<ListeningStats> {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let time_filter = match (start_time, end_time) {
            (Some(start), Some(end)) => format!("AND s.start_time >= {} AND s.start_time <= {}", start, end),
            (Some(start), None) => format!("AND s.start_time >= {}", start),
            (None, Some(end)) => format!("AND s.start_time <= {}", end),
            (None, None) => String::new(),
        };

        // Get total listening time including active sessions
        let total_listening_time: i64 = self.conn.query_row(
            &format!(
                "SELECT COALESCE(SUM(
                    CASE
                        WHEN listened_time IS NOT NULL THEN listened_time
                        WHEN status = 'active' THEN {} - start_time - paused_time
                        ELSE 0
                    END
                ), 0) FROM sessions s WHERE (listened_time IS NOT NULL OR status = 'active') {}",
                current_time, time_filter
            ),
            [],
            |row| row.get(0),
        )?;

        // Get top tracks including active sessions
        let mut stmt = self.conn.prepare(&format!(
            "SELECT t.id, t.title, t.artist, t.album, t.length, t.art_url,
                    COALESCE(SUM(
                        CASE
                            WHEN s.listened_time IS NOT NULL THEN s.listened_time
                            WHEN s.status = 'active' THEN {} - s.start_time - s.paused_time
                            ELSE 0
                        END
                    ), 0) as total_time,
                    COUNT(s.id) as play_count
             FROM tracks t
             JOIN sessions s ON t.id = s.track_id
             WHERE (s.listened_time IS NOT NULL OR s.status = 'active') {}
             GROUP BY t.id
             ORDER BY total_time DESC
             LIMIT 20",
            current_time, time_filter
        ))?;

        let top_tracks: Vec<TrackStats> = stmt.query_map([], |row| {
            Ok(TrackStats {
                track: Track {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    artist: row.get(2)?,
                    album: row.get(3)?,
                    length: row.get(4)?,
                    art_url: row.get(5)?,
                },
                total_listened_time: row.get(6)?,
                play_count: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        // Get top artists including active sessions
        let mut stmt = self.conn.prepare(&format!(
            "SELECT t.artist,
                    COALESCE(SUM(
                        CASE
                            WHEN s.listened_time IS NOT NULL THEN s.listened_time
                            WHEN s.status = 'active' THEN {} - s.start_time - s.paused_time
                            ELSE 0
                        END
                    ), 0) as total_time,
                    COUNT(DISTINCT t.id) as track_count
             FROM tracks t
             JOIN sessions s ON t.id = s.track_id
             WHERE (s.listened_time IS NOT NULL OR s.status = 'active') {}
             GROUP BY t.artist
             ORDER BY total_time DESC
             LIMIT 20",
            current_time, time_filter
        ))?;

        let top_artists: Vec<ArtistStats> = stmt.query_map([], |row| {
            Ok(ArtistStats {
                artist: row.get(0)?,
                total_listened_time: row.get(1)?,
                track_count: row.get(2)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        // Get listening history including active sessions, excluding very short sessions
        let mut stmt = self.conn.prepare(&format!(
            "SELECT s.id, s.track_id, s.player_id, s.start_time, s.end_time,
                    s.paused_time,
                    CASE
                        WHEN s.listened_time IS NOT NULL THEN s.listened_time
                        WHEN s.status = 'active' THEN {} - s.start_time - s.paused_time
                        ELSE 0
                    END as calculated_listened_time,
                    s.status,
                    t.title, t.artist, t.album, t.length, t.art_url,
                    p.name, p.identity
             FROM sessions s
             JOIN tracks t ON s.track_id = t.id
             JOIN players p ON s.player_id = p.id
             WHERE (s.listened_time IS NOT NULL OR s.status = 'active')
               AND (
                   s.status = 'active' OR
                   s.listened_time > 0
               ) {}
             ORDER BY s.start_time DESC
             LIMIT 100",
            current_time, time_filter
        ))?;

        let listening_history: Vec<SessionWithMetadata> = stmt.query_map([], |row| {
            Ok(SessionWithMetadata {
                session: Session {
                    id: row.get(0)?,
                    track_id: row.get(1)?,
                    player_id: row.get(2)?,
                    start_time: row.get(3)?,
                    end_time: row.get(4)?,
                    paused_time: row.get(5)?,
                    listened_time: Some(row.get(6)?), // Use calculated listened time
                    status: row.get(7)?,
                },
                track: Track {
                    id: row.get(1)?,
                    title: row.get(8)?,
                    artist: row.get(9)?,
                    album: row.get(10)?,
                    length: row.get(11)?,
                    art_url: row.get(12)?,
                },
                player: Player {
                    id: row.get(2)?,
                    name: row.get(13)?,
                    identity: row.get(14)?,
                },
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        Ok(ListeningStats {
            total_listening_time,
            top_tracks,
            top_artists,
            listening_history,
        })
    }

    /// Clean up orphaned sessions (active sessions from previous runs)
    pub fn cleanup_orphaned_sessions(&self, current_time: i64, max_session_duration: i64) -> Result<usize> {
        // Find active sessions that are too old (likely from previous daemon runs)
        let mut stmt = self.conn.prepare(
            "SELECT id, start_time FROM sessions WHERE status = 'active' AND ?1 - start_time > ?2"
        )?;

        let orphaned_sessions: Vec<(i64, i64)> = stmt.query_map(
            params![current_time, max_session_duration],
            |row| Ok((row.get(0)?, row.get(1)?))
        )?.collect::<Result<Vec<_>, _>>()?;

        let count = orphaned_sessions.len();
        
        for (session_id, start_time) in orphaned_sessions {
            // Calculate a reasonable end time (start_time + max_session_duration)
            let estimated_end_time = start_time + max_session_duration;
            self.finalize_session(session_id, estimated_end_time, "orphaned")?;
        }

        Ok(count)
    }

    /// Get database statistics
    pub fn get_database_stats(&self) -> Result<DatabaseStats> {
        let total_sessions: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sessions",
            [],
            |row| row.get(0),
        )?;

        let active_sessions: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE status = 'active'",
            [],
            |row| row.get(0),
        )?;

        let total_tracks: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM tracks",
            [],
            |row| row.get(0),
        )?;

        let total_players: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM players",
            [],
            |row| row.get(0),
        )?;

        Ok(DatabaseStats {
            total_sessions,
            active_sessions,
            total_tracks,
            total_players,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseStats {
    pub total_sessions: i64,
    pub active_sessions: i64,
    pub total_tracks: i64,
    pub total_players: i64,
}