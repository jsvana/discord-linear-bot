use std::collections::HashMap;
use std::env;

use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Missing environment variable: {0}")]
    Missing(String),

    #[error("Invalid value for {0}: {1}")]
    Invalid(String, String),

    #[error("No channels configured")]
    NoChannels,
}

/// Per-channel configuration mapping a Discord forum channel to a Linear team + label.
#[derive(Debug, Clone, Deserialize)]
pub struct ChannelConfig {
    /// Discord channel ID (forum channel)
    pub discord_channel_id: u64,
    /// Discord guild ID this channel belongs to
    pub guild_id: u64,
    /// "feature" or "bug"
    pub channel_type: String,
    /// Linear team ID to create issues in
    pub linear_team_id: String,
    /// Primary Linear label ID for this channel type
    pub linear_label_id: String,
    /// Optional: map Discord forum tag IDs to additional Linear label IDs
    #[serde(default)]
    pub tag_label_map: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub discord_token: String,
    pub linear_api_key: String,
    pub channels: Vec<ChannelConfig>,
    pub database_url: String,
    pub poll_interval_secs: u64,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let channels_json = required("CHANNELS")?;
        let channels: Vec<ChannelConfig> = serde_json::from_str(&channels_json)
            .map_err(|e| ConfigError::Invalid("CHANNELS".into(), e.to_string()))?;

        if channels.is_empty() {
            return Err(ConfigError::NoChannels);
        }

        Ok(Config {
            discord_token: required("DISCORD_TOKEN")?,
            linear_api_key: required("LINEAR_API_KEY")?,
            channels,
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:bot.db".into()),
            poll_interval_secs: env::var("POLL_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
        })
    }

    /// Look up channel config by Discord channel ID.
    pub fn channel_config(&self, discord_channel_id: u64) -> Option<&ChannelConfig> {
        self.channels
            .iter()
            .find(|c| c.discord_channel_id == discord_channel_id)
    }

    /// Whether a channel ID is monitored.
    pub fn is_monitored_channel(&self, channel_id: u64) -> bool {
        self.channel_config(channel_id).is_some()
    }

    /// All unique Linear team IDs across all channels.
    pub fn unique_team_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .channels
            .iter()
            .map(|c| c.linear_team_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// All unique guild IDs across all channels.
    pub fn unique_guild_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.channels.iter().map(|c| c.guild_id).collect();
        ids.sort();
        ids.dedup();
        ids
    }
}

fn required(name: &str) -> Result<String, ConfigError> {
    env::var(name).map_err(|_| ConfigError::Missing(name.into()))
}
