use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Database configuration
    pub database: DatabaseConfig,
    
    /// Monitoring configuration
    pub monitoring: MonitoringConfig,
    
    /// Logging configuration
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Path to the SQLite database file
    pub path: String,
    
    /// Database connection pool size (for future use)
    pub pool_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    /// How often to check for new players (in seconds)
    pub player_discovery_interval: u64,
    
    /// How long to wait before considering a session stale (in seconds)
    pub session_timeout: u64,
    
    /// How often to run cleanup tasks (in seconds)
    pub cleanup_interval: u64,
    
    /// Minimum session duration to record (in seconds)
    pub min_session_duration: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (error, warn, info, debug, trace)
    pub level: String,
    
    /// Log file path (optional, logs to stderr if not specified)
    pub file: Option<String>,
    
    /// Whether to include timestamps in logs
    pub timestamps: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            database: DatabaseConfig {
                path: "~/.local/share/gopal/music.db".to_string(),
                pool_size: None,
            },
            monitoring: MonitoringConfig {
                player_discovery_interval: 5,
                session_timeout: 300, // 5 minutes
                cleanup_interval: 300, // 5 minutes
                min_session_duration: 10, // 10 seconds
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                file: None,
                timestamps: true,
            },
        }
    }
}

impl Config {
    /// Load configuration from file, falling back to defaults
    pub fn load(config_path: Option<&Path>) -> Result<Self> {
        if let Some(path) = config_path {
            if path.exists() {
                let content = std::fs::read_to_string(path)
                    .context("Failed to read configuration file")?;
                
                let config: Config = toml::from_str(&content)
                    .context("Failed to parse configuration file")?;
                
                Ok(config)
            } else {
                // Create default config file
                let default_config = Config::default();
                let toml_content = toml::to_string_pretty(&default_config)
                    .context("Failed to serialize default configuration")?;
                
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .context("Failed to create config directory")?;
                }
                
                std::fs::write(path, toml_content)
                    .context("Failed to write default configuration file")?;
                
                Ok(default_config)
            }
        } else {
            // No config file specified, use defaults
            Ok(Config::default())
        }
    }
    
    /// Save configuration to file
    #[allow(dead_code)]
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let toml_content = toml::to_string_pretty(self)
            .context("Failed to serialize configuration")?;
        
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create config directory")?;
        }
        
        std::fs::write(path, toml_content)
            .context("Failed to write configuration file")?;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.monitoring.player_discovery_interval, 5);
        assert_eq!(config.monitoring.session_timeout, 300);
        assert_eq!(config.logging.level, "info");
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let toml_str = toml::to_string(&config).unwrap();
        let parsed_config: Config = toml::from_str(&toml_str).unwrap();
        
        assert_eq!(config.monitoring.player_discovery_interval, parsed_config.monitoring.player_discovery_interval);
        assert_eq!(config.database.path, parsed_config.database.path);
    }

    #[test]
    fn test_config_load_nonexistent() {
        let temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path();
        
        // Remove the file so it doesn't exist
        std::fs::remove_file(temp_path).unwrap();
        
        let config = Config::load(Some(temp_path)).unwrap();
        
        // Should create default config and file should now exist
        assert!(temp_path.exists());
        assert_eq!(config.monitoring.player_discovery_interval, 5);
    }

    #[test]
    fn test_config_save_and_load() {
        let temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path();
        
        let mut config = Config::default();
        config.monitoring.player_discovery_interval = 10;
        config.logging.level = "debug".to_string();
        
        // Save config
        config.save(temp_path).unwrap();
        
        // Load config
        let loaded_config = Config::load(Some(temp_path)).unwrap();
        
        assert_eq!(loaded_config.monitoring.player_discovery_interval, 10);
        assert_eq!(loaded_config.logging.level, "debug");
    }
}