use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use mpris::{Metadata, PlaybackStatus, PlayerFinder};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

use crate::database::{Database, Track};
use crate::session_tracker::{SessionTracker, SessionEvent};

#[derive(Debug, Clone)]
struct PlayerState {
    player_id: i64,
    current_metadata: Option<Metadata>,
    current_status: PlaybackStatus,
    last_update: i64,
}

pub struct MprisMonitor {
    db: Database,
    session_tracker: SessionTracker,
    player_finder: PlayerFinder,
    player_states: HashMap<String, PlayerState>,
}

impl MprisMonitor {
    pub fn new(db: Database) -> Result<Self> {
        let player_finder = PlayerFinder::new()
            .context("Failed to create MPRIS player finder")?;
        
        let session_tracker = SessionTracker::new();

        Ok(MprisMonitor {
            db,
            session_tracker,
            player_finder,
            player_states: HashMap::new(),
        })
    }

    pub async fn start_monitoring(&mut self) -> Result<()> {
        info!("Starting MPRIS monitoring...");

        // Set up event channel for session events
        let (session_tx, mut session_rx) = mpsc::unbounded_channel();
        self.session_tracker.set_event_sender(session_tx);

        // Start the main monitoring loop
        let mut poll_interval = tokio::time::interval(Duration::from_secs(2));
        let mut discovery_interval = tokio::time::interval(Duration::from_secs(5));
        let mut cleanup_interval = tokio::time::interval(Duration::from_secs(60));
        let mut update_interval = tokio::time::interval(Duration::from_secs(30)); // Update active sessions every 30 seconds

        loop {
            tokio::select! {
                // Handle session events
                Some(event) = session_rx.recv() => {
                    if let Err(e) = self.handle_session_event(event).await {
                        error!("Error handling session event: {}", e);
                    }
                }
                
                // Poll existing players for status changes
                _ = poll_interval.tick() => {
                    if let Err(e) = self.poll_players().await {
                        error!("Error polling players: {}", e);
                    }
                }
                
                // Discover new players
                _ = discovery_interval.tick() => {
                    if let Err(e) = self.discover_players().await {
                        error!("Error discovering players: {}", e);
                    }
                }
                
                // Cleanup stale sessions and detect long idle periods
                _ = cleanup_interval.tick() => {
                    let current_time = Self::current_timestamp();
                    
                    // Check for sessions that might have been affected by system sleep/suspend
                    if let Err(e) = self.check_for_sleep_resume(current_time).await {
                        error!("Error checking for sleep/resume: {}", e);
                    }
                    
                    // Regular cleanup of stale sessions
                    if let Err(e) = self.session_tracker.cleanup_stale_sessions(current_time, 300).await {
                        error!("Error cleaning up stale sessions: {}", e);
                    }
                }
                
                // Update active sessions in database for real-time stats
                _ = update_interval.tick() => {
                    if let Err(e) = self.update_active_sessions().await {
                        error!("Error updating active sessions: {}", e);
                    }
                }
            }
        }
    }

    async fn discover_players(&mut self) -> Result<()> {
        let players = self.player_finder.find_all()
            .context("Failed to find MPRIS players")?;

        for player in players {
            let bus_name = player.bus_name().to_string();
            
            if !self.player_states.contains_key(&bus_name) {
                info!("Discovered new player: {}", bus_name);
                
                // Register player in database
                let identity = player.identity().to_string();
                
                let player_id = self.db.insert_or_update_player(&bus_name, &identity)
                    .context("Failed to register player in database")?;

                // Initialize player state
                let current_metadata = player.get_metadata().ok();
                let current_status = player.get_playback_status().unwrap_or(PlaybackStatus::Stopped);
                let current_time = Self::current_timestamp();

                let player_state = PlayerState {
                    player_id,
                    current_metadata: current_metadata.clone(),
                    current_status,
                    last_update: current_time,
                };

                self.player_states.insert(bus_name, player_state);

                // If currently playing, start a session
                if current_status == PlaybackStatus::Playing {
                    if let Some(metadata) = current_metadata {
                        let track = Self::metadata_to_track(&metadata);
                        self.session_tracker.handle_play_event(player_id, track, current_time).await?;
                    } else {
                        // Try to get metadata again for playing tracks
                        if let Ok(metadata) = player.get_metadata() {
                            let track = Self::metadata_to_track(&metadata);
                            self.session_tracker.handle_play_event(player_id, track, current_time).await?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn poll_players(&mut self) -> Result<()> {
        let players = self.player_finder.find_all()
            .context("Failed to find MPRIS players")?;
        
        let mut active_players = HashMap::new();
        
        for player in players {
            let bus_name = player.bus_name().to_string();
            active_players.insert(bus_name.clone(), player);
        }

        // Check each tracked player
        let mut players_to_remove = Vec::new();
        let mut state_updates = Vec::new();
        
        for (bus_name, player_state) in &self.player_states {
            if let Some(player) = active_players.get(bus_name) {
                // Player still exists, check for changes
                state_updates.push((bus_name.clone(), player));
            } else {
                // Player no longer exists
                info!("Player {} disappeared", bus_name);
                
                // Finalize any active session
                if player_state.current_status == PlaybackStatus::Playing {
                    let current_time = Self::current_timestamp();
                    self.session_tracker.handle_stop_event(player_state.player_id, current_time).await?;
                }
                
                players_to_remove.push(bus_name.clone());
            }
        }

        // Process state updates
        for (bus_name, player) in state_updates {
            let current_time = Self::current_timestamp();
            let new_status = player.get_playback_status().unwrap_or(PlaybackStatus::Stopped);
            let new_metadata = player.get_metadata().ok();
            
            if let Some(ref metadata) = new_metadata {
                debug!("Polling player {}: status={:?}, track='{}'",
                       bus_name, new_status, metadata.title().unwrap_or("Unknown"));
            } else {
                debug!("Polling player {}: status={:?}, no metadata",
                       bus_name, new_status);
            }
            
            // Extract the player state data we need
            let (player_id, old_status, old_metadata) = {
                if let Some(state) = self.player_states.get(&bus_name) {
                    (state.player_id, state.current_status, state.current_metadata.clone())
                } else {
                    continue;
                }
            };
            
            // Debug: Show what we're comparing
            if let (Some(ref old_meta), Some(ref new_meta)) = (&old_metadata, &new_metadata) {
                debug!("Comparing metadata: old='{}' vs new='{}'",
                       old_meta.title().unwrap_or("Unknown"),
                       new_meta.title().unwrap_or("Unknown"));
            }
            
            // Handle state changes
            if let Err(e) = self.handle_state_changes(
                player_id,
                old_status,
                new_status,
                old_metadata,
                new_metadata.clone(),
                current_time
            ).await {
                warn!("Error handling player state change for {}: {}", bus_name, e);
            }
            
            // Update the state
            if let Some(player_state) = self.player_states.get_mut(&bus_name) {
                player_state.current_status = new_status;
                player_state.current_metadata = new_metadata;
                player_state.last_update = current_time;
            }
        }

        // Remove disappeared players
        for bus_name in players_to_remove {
            self.player_states.remove(&bus_name);
        }

        Ok(())
    }

    async fn handle_state_changes(
        &mut self,
        player_id: i64,
        old_status: PlaybackStatus,
        new_status: PlaybackStatus,
        old_metadata: Option<Metadata>,
        new_metadata: Option<Metadata>,
        current_time: i64,
    ) -> Result<()> {
        // Check for status changes
        if new_status != old_status {
            debug!("Player status changed: {:?} -> {:?}", old_status, new_status);

            match (old_status, new_status) {
                (PlaybackStatus::Playing, PlaybackStatus::Paused) => {
                    self.session_tracker.handle_pause_event(player_id, current_time).await?;
                }
                (PlaybackStatus::Paused, PlaybackStatus::Playing) => {
                    // Check if we have an active session, if not create one
                    if !self.session_tracker.has_active_session(player_id) {
                        debug!("No active session for resume, creating new session");
                        if let Some(ref metadata) = new_metadata {
                            let track = Self::metadata_to_track(metadata);
                            self.session_tracker.handle_play_event(player_id, track, current_time).await?;
                        } else if let Some(ref metadata) = old_metadata {
                            let track = Self::metadata_to_track(metadata);
                            self.session_tracker.handle_play_event(player_id, track, current_time).await?;
                        }
                    } else {
                        self.session_tracker.handle_resume_event(player_id, current_time).await?;
                    }
                }
                (PlaybackStatus::Playing, PlaybackStatus::Stopped) => {
                    self.session_tracker.handle_stop_event(player_id, current_time).await?;
                }
                (_, PlaybackStatus::Playing) => {
                    // Started playing from stopped state
                    if let Some(ref metadata) = new_metadata {
                        let track = Self::metadata_to_track(metadata);
                        self.session_tracker.handle_play_event(player_id, track, current_time).await?;
                    } else if let Some(ref metadata) = old_metadata {
                        // Use old metadata if new metadata is not available
                        let track = Self::metadata_to_track(metadata);
                        self.session_tracker.handle_play_event(player_id, track, current_time).await?;
                    }
                }
                _ => {}
            }
        }

        // Check for metadata changes (new track)
        let metadata_changed = match (&old_metadata, &new_metadata) {
            (Some(old), Some(new)) => {
                // Always compare title and artist, even if track IDs are available
                // Some players (like Chromium) reuse track IDs for different songs
                let title_changed = old.title() != new.title();
                let artist_changed = old.artists() != new.artists();
                let content_changed = title_changed || artist_changed;
                
                // Also check track ID if available
                let id_changed = if let (Some(old_id), Some(new_id)) = (old.track_id(), new.track_id()) {
                    old_id != new_id
                } else {
                    false
                };
                
                let changed = content_changed || id_changed;
                
                debug!("Comparing titles: '{:?}' vs '{:?}' = {}",
                       old.title(), new.title(), title_changed);
                debug!("Comparing artists: '{:?}' vs '{:?}' = {}",
                       old.artists(), new.artists(), artist_changed);
                if let (Some(old_id), Some(new_id)) = (old.track_id(), new.track_id()) {
                    debug!("Comparing track IDs: '{}' vs '{}' = {}", old_id, new_id, id_changed);
                }
                debug!("Content changed: {}, ID changed: {}, Overall changed: {}",
                       content_changed, id_changed, changed);
                
                changed
            }
            (None, Some(_)) => {
                debug!("Metadata appeared (was None, now Some)");
                true
            }
            (Some(_), None) => {
                debug!("Metadata disappeared (was Some, now None)");
                true
            }
            (None, None) => {
                debug!("No metadata in either old or new");
                false
            }
        };

        debug!("Final metadata_changed result: {}", metadata_changed);

        if metadata_changed {
            debug!("Processing metadata change - stopping old session and starting new one");

            // If we were playing something else, stop the previous session
            if old_status == PlaybackStatus::Playing {
                debug!("Stopping previous session for player {}", player_id);
                self.session_tracker.handle_stop_event(player_id, current_time).await?;
            }

            // If currently playing, start new session for the new track
            if new_status == PlaybackStatus::Playing {
                if let Some(ref metadata) = new_metadata {
                    debug!("Starting new session for player {}", player_id);
                    let track = Self::metadata_to_track(metadata);
                    self.session_tracker.handle_play_event(player_id, track, current_time).await?;
                } else {
                    debug!("No metadata available for new session");
                }
            }
        }

        Ok(())
    }

    async fn handle_session_event(&mut self, event: SessionEvent) -> Result<()> {
        match event {
            SessionEvent::SessionStarted { session_id, track, player_id, start_time } => {
                debug!("Session started: {} for track: {}", session_id, track.title);
                self.db.insert_or_update_track(&track)?;
                self.db.start_session(&track.id, player_id, start_time)?;
            }
            
            SessionEvent::SessionPaused { session_id, pause_duration } => {
                debug!("Session paused: {} for {} seconds", session_id, pause_duration);
                self.db.update_session_pause_time(session_id, pause_duration)?;
            }
            
            SessionEvent::SessionFinalized { session_id, end_time, status } => {
                debug!("Session finalized: {} with status: {}", session_id, status);
                self.db.finalize_session(session_id, end_time, &status)?;
            }
        }
        Ok(())
    }

    fn metadata_to_track(metadata: &Metadata) -> Track {
        // Always generate a unique ID based on content to avoid issues with
        // players that reuse MPRIS track IDs for different songs
        let title = metadata.title().unwrap_or("Unknown");
        let artist = metadata.artists()
            .map(|artists| artists.join(", "))
            .unwrap_or_else(|| "Unknown".to_string());
        let album = metadata.album_name().unwrap_or("Unknown");
        
        // Create a content-based unique ID
        let track_id = format!("{}::{}::{}", title, artist, album);

        let track = Track {
            id: track_id,
            title: title.to_string(),
            artist: artist,
            album: album.to_string(),
            length: metadata.length().map(|d| d.as_micros() as i64),
            art_url: metadata.art_url().map(|url| url.to_string()),
        };

        debug!("Created track: {} - {} ({}) [ID: {}]", track.title, track.artist, track.album, track.id);
        track
    }

    fn current_timestamp() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    async fn check_for_sleep_resume(&mut self, current_time: i64) -> Result<()> {
        let max_reasonable_gap = 300; // 5 minutes - if we haven't polled for longer, system might have slept
        
        for (bus_name, player_state) in &mut self.player_states {
            let time_since_last_update = current_time - player_state.last_update;
            
            if time_since_last_update > max_reasonable_gap {
                info!("Detected long gap ({} seconds) for player {} - treating as pause period",
                      time_since_last_update, bus_name);
                
                // If there was an active session, add the gap as pause time instead of discarding
                if player_state.current_status == PlaybackStatus::Playing {
                    info!("Adding {} seconds as pause time for player {} due to system sleep/suspend",
                          time_since_last_update, player_state.player_id);
                    
                    // Add the entire gap as pause time
                    self.session_tracker.handle_sleep_gap(
                        player_state.player_id,
                        time_since_last_update
                    ).await?;
                }
                
                // Update the last update time to current time
                player_state.last_update = current_time;
            }
        }
        
        Ok(())
    }

    async fn update_active_sessions(&mut self) -> Result<()> {
        let current_time = Self::current_timestamp();
        
        // Get all active sessions and update their progress in the database
        let active_sessions = self.session_tracker.get_active_sessions();
        
        for (player_id, session) in active_sessions {
            debug!("Updating progress for active session {} (player {})", session.session_id, player_id);
            
            if let Err(e) = self.db.update_active_session_progress(session.session_id, current_time) {
                warn!("Failed to update progress for session {}: {}", session.session_id, e);
            }
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_mpris_monitor_creation() {
        let temp_db = NamedTempFile::new().unwrap();
        let db = Database::new(temp_db.path()).unwrap();
        
        let monitor = MprisMonitor::new(db);
        assert!(monitor.is_ok());
    }

    #[test]
    fn test_current_timestamp() {
        let timestamp = MprisMonitor::current_timestamp();
        assert!(timestamp > 0);
    }
}