use anyhow::Result;
use log::{debug, warn};
use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::database::Track;

#[derive(Debug, Clone)]
pub enum SessionEvent {
    SessionStarted {
        session_id: i64,
        track: Track,
        player_id: i64,
        start_time: i64,
    },
    SessionPaused {
        session_id: i64,
        pause_duration: i64,
    },
    SessionFinalized {
        session_id: i64,
        end_time: i64,
        status: String,
    },
}

#[derive(Debug, Clone)]
pub struct ActiveSession {
    pub session_id: i64,
    pub track: Track,
    pub player_id: i64,
    pub start_time: i64,
    pub pause_start_time: Option<i64>,
    pub total_pause_time: i64,
    pub is_paused: bool,
}

#[derive(Clone)]
pub struct SessionTracker {
    active_sessions: HashMap<i64, ActiveSession>, // player_id -> session
    event_sender: Option<mpsc::UnboundedSender<SessionEvent>>,
    next_session_id: i64,
}

impl SessionTracker {
    pub fn new() -> Self {
        SessionTracker {
            active_sessions: HashMap::new(),
            event_sender: None,
            next_session_id: 1,
        }
    }

    pub fn set_event_sender(&mut self, sender: mpsc::UnboundedSender<SessionEvent>) {
        self.event_sender = Some(sender);
    }

    pub async fn handle_play_event(
        &mut self,
        player_id: i64,
        track: Track,
        timestamp: i64,
    ) -> Result<()> {
        debug!("Handling play event for player {} at {}", player_id, timestamp);

        // If there's an active session for this player, finalize it first
        if let Some(_existing_session) = self.active_sessions.get(&player_id) {
            self.finalize_session(player_id, timestamp, "interrupted").await?;
        }

        // Create new session
        let session_id = self.next_session_id;
        self.next_session_id += 1;

        let session = ActiveSession {
            session_id,
            track: track.clone(),
            player_id,
            start_time: timestamp,
            pause_start_time: None,
            total_pause_time: 0,
            is_paused: false,
        };

        self.active_sessions.insert(player_id, session);

        // Send session started event
        if let Some(ref sender) = self.event_sender {
            let _ = sender.send(SessionEvent::SessionStarted {
                session_id,
                track,
                player_id,
                start_time: timestamp,
            });
        }

        Ok(())
    }

    pub async fn handle_pause_event(&mut self, player_id: i64, timestamp: i64) -> Result<()> {
        debug!("Handling pause event for player {} at {}", player_id, timestamp);

        if let Some(session) = self.active_sessions.get_mut(&player_id) {
            if !session.is_paused {
                session.pause_start_time = Some(timestamp);
                session.is_paused = true;
                debug!("Session {} paused at {}", session.session_id, timestamp);
            } else {
                warn!("Received pause event for already paused session {}", session.session_id);
            }
        } else {
            debug!("Received pause event for player {} with no active session - ignoring", player_id);
        }

        Ok(())
    }

    pub async fn handle_resume_event(&mut self, player_id: i64, timestamp: i64) -> Result<()> {
        debug!("Handling resume event for player {} at {}", player_id, timestamp);

        if let Some(session) = self.active_sessions.get_mut(&player_id) {
            if session.is_paused {
                if let Some(pause_start) = session.pause_start_time {
                    let pause_duration = timestamp - pause_start;
                    session.total_pause_time += pause_duration;
                    session.pause_start_time = None;
                    session.is_paused = false;

                    debug!(
                        "Session {} resumed after {} seconds of pause",
                        session.session_id, pause_duration
                    );

                    // Send pause duration event
                    if let Some(ref sender) = self.event_sender {
                        let _ = sender.send(SessionEvent::SessionPaused {
                            session_id: session.session_id,
                            pause_duration,
                        });
                    }
                } else {
                    warn!("Session {} marked as paused but no pause start time", session.session_id);
                    session.is_paused = false;
                }
            } else {
                warn!("Received resume event for non-paused session {}", session.session_id);
            }
        } else {
            debug!("Received resume event for player {} with no active session - ignoring", player_id);
        }

        Ok(())
    }

    pub async fn handle_stop_event(&mut self, player_id: i64, timestamp: i64) -> Result<()> {
        debug!("Handling stop event for player {} at {}", player_id, timestamp);
        self.finalize_session(player_id, timestamp, "completed").await
    }

    pub async fn handle_sleep_gap(&mut self, player_id: i64, gap_duration: i64) -> Result<()> {
        debug!("Handling sleep gap of {} seconds for player {}", gap_duration, player_id);
        
        if let Some(session) = self.active_sessions.get_mut(&player_id) {
            // Add the entire gap as pause time
            session.total_pause_time += gap_duration;
            
            debug!("Added {} seconds of pause time to session {} (total pause: {}s)",
                   gap_duration, session.session_id, session.total_pause_time);
            
            // Send pause duration event
            if let Some(ref sender) = self.event_sender {
                let _ = sender.send(SessionEvent::SessionPaused {
                    session_id: session.session_id,
                    pause_duration: gap_duration,
                });
            }
        } else {
            debug!("No active session found for player {} when handling sleep gap", player_id);
        }
        
        Ok(())
    }

    async fn finalize_session(
        &mut self,
        player_id: i64,
        end_time: i64,
        status: &str,
    ) -> Result<()> {
        if let Some(mut session) = self.active_sessions.remove(&player_id) {
            // If the session was paused when it ended, calculate the final pause duration
            if session.is_paused {
                if let Some(pause_start) = session.pause_start_time {
                    let final_pause_duration = end_time - pause_start;
                    session.total_pause_time += final_pause_duration;

                    // Send the final pause duration event
                    if let Some(ref sender) = self.event_sender {
                        let _ = sender.send(SessionEvent::SessionPaused {
                            session_id: session.session_id,
                            pause_duration: final_pause_duration,
                        });
                    }
                }
            }

            let duration = end_time - session.start_time;
            let max_reasonable_duration = 24 * 3600; // 24 hours - very generous for long listening sessions
            
            // Only cap extremely long sessions (likely from system issues)
            let capped_end_time = if duration > max_reasonable_duration {
                warn!("Session {} duration ({} hours) exceeds 24 hours, capping to prevent system sleep inflation",
                      session.session_id, duration / 3600);
                session.start_time + max_reasonable_duration
            } else {
                end_time
            };

            let final_duration = capped_end_time - session.start_time;
            debug!(
                "Finalizing session {} for track '{}' - Duration: {}s, Paused: {}s",
                session.session_id,
                session.track.title,
                final_duration,
                session.total_pause_time
            );

            // Only finalize sessions that have a reasonable duration (at least 1 second)
            if final_duration >= 1 {
                // Send session finalized event with capped end time
                if let Some(ref sender) = self.event_sender {
                    let _ = sender.send(SessionEvent::SessionFinalized {
                        session_id: session.session_id,
                        end_time: capped_end_time,
                        status: status.to_string(),
                    });
                }
            } else {
                debug!("Skipping finalization of very short session {} ({}s)", session.session_id, final_duration);
            }
        } else {
            debug!("Attempted to finalize session for player {} with no active session - ignoring", player_id);
        }

        Ok(())
    }

    pub fn get_active_session_count(&self) -> usize {
        self.active_sessions.len()
    }

    pub fn has_active_session(&self, player_id: i64) -> bool {
        self.active_sessions.contains_key(&player_id)
    }

    pub fn get_active_sessions(&self) -> Vec<(i64, &ActiveSession)> {
        self.active_sessions.iter().map(|(&k, v)| (k, v)).collect()
    }

    pub async fn cleanup_stale_sessions(&mut self, current_time: i64, max_idle_time: i64) -> Result<()> {
        let mut stale_players = Vec::new();

        for (&player_id, session) in &self.active_sessions {
            let last_activity_time = session.pause_start_time.unwrap_or(session.start_time);
            if current_time - last_activity_time > max_idle_time {
                debug!(
                    "Session {} for player {} is stale (idle for {}s), cleaning up",
                    session.session_id,
                    player_id,
                    current_time - last_activity_time
                );
                stale_players.push(player_id);
            }
        }

        for player_id in stale_players {
            self.finalize_session(player_id, current_time, "timeout").await?;
        }

        Ok(())
    }
}

impl Default for SessionTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn create_test_track() -> Track {
        Track {
            id: "test_track_1".to_string(),
            title: "Test Song".to_string(),
            artist: "Test Artist".to_string(),
            album: "Test Album".to_string(),
            length: Some(180_000_000), // 3 minutes in microseconds
            art_url: None,
        }
    }

    #[tokio::test]
    async fn test_session_lifecycle() {
        let mut tracker = SessionTracker::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        tracker.set_event_sender(tx);

        let player_id = 1;
        let track = create_test_track();
        let start_time = 1000;

        // Start session
        tracker.handle_play_event(player_id, track.clone(), start_time).await.unwrap();
        assert_eq!(tracker.get_active_session_count(), 1);

        // Check session started event
        if let Some(SessionEvent::SessionStarted { session_id, .. }) = rx.recv().await {
            assert_eq!(session_id, 1);
        } else {
            panic!("Expected SessionStarted event");
        }

        // Pause session
        tracker.handle_pause_event(player_id, start_time + 60).await.unwrap();

        // Resume session
        tracker.handle_resume_event(player_id, start_time + 90).await.unwrap();

        // Check pause event
        if let Some(SessionEvent::SessionPaused { pause_duration, .. }) = rx.recv().await {
            assert_eq!(pause_duration, 30); // 30 seconds pause
        } else {
            panic!("Expected SessionPaused event");
        }

        // Stop session
        tracker.handle_stop_event(player_id, start_time + 180).await.unwrap();
        assert_eq!(tracker.get_active_session_count(), 0);

        // Check session finalized event
        if let Some(SessionEvent::SessionFinalized { status, .. }) = rx.recv().await {
            assert_eq!(status, "completed");
        } else {
            panic!("Expected SessionFinalized event");
        }
    }

    #[tokio::test]
    async fn test_multiple_players() {
        let mut tracker = SessionTracker::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        tracker.set_event_sender(tx);

        let track1 = create_test_track();
        let mut track2 = create_test_track();
        track2.id = "test_track_2".to_string();
        track2.title = "Test Song 2".to_string();

        // Start sessions for two different players
        tracker.handle_play_event(1, track1, 1000).await.unwrap();
        tracker.handle_play_event(2, track2, 1010).await.unwrap();

        assert_eq!(tracker.get_active_session_count(), 2);

        // Stop one session
        tracker.handle_stop_event(1, 1100).await.unwrap();
        assert_eq!(tracker.get_active_session_count(), 1);

        // Stop the other session
        tracker.handle_stop_event(2, 1200).await.unwrap();
        assert_eq!(tracker.get_active_session_count(), 0);
    }

    #[tokio::test]
    async fn test_session_interruption() {
        let mut tracker = SessionTracker::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        tracker.set_event_sender(tx);

        let track1 = create_test_track();
        let mut track2 = create_test_track();
        track2.id = "test_track_2".to_string();

        let player_id = 1;

        // Start first session
        tracker.handle_play_event(player_id, track1, 1000).await.unwrap();
        assert_eq!(tracker.get_active_session_count(), 1);

        // Start second session (should interrupt the first)
        tracker.handle_play_event(player_id, track2, 1100).await.unwrap();
        assert_eq!(tracker.get_active_session_count(), 1);

        // The active session should be for track2
        let sessions = tracker.get_active_sessions();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].1.track.id, "test_track_2");
    }

    #[tokio::test]
    async fn test_cleanup_stale_sessions() {
        let mut tracker = SessionTracker::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        tracker.set_event_sender(tx);

        let track = create_test_track();
        let start_time = 1000;
        let current_time = 2000;
        let max_idle_time = 500;

        // Start session
        tracker.handle_play_event(1, track, start_time).await.unwrap();
        assert_eq!(tracker.get_active_session_count(), 1);

        // Cleanup stale sessions (session should be considered stale)
        tracker.cleanup_stale_sessions(current_time, max_idle_time).await.unwrap();
        assert_eq!(tracker.get_active_session_count(), 0);
    }
}