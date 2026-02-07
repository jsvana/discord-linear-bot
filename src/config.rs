use std::collections::HashMap;
use std::env;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Missing environment variable: {0}")]
    Missing(String),

    #[error("Invalid value for {0}: {1}")]
    Invalid(String, String),
}

#[derive(Debug, Clone)]
pub struct Config {
    pub discord_token: String,
    pub guild_id: u64,
    pub feature_requests_channel_id: u64,
    pub bug_reports_channel_id: u64,
    pub linear_api_key: String,
    pub linear_team_id: String,
    pub linear_feature_label_id: String,
    pub linear_bug_label_id: String,
    pub tag_label_map: HashMap<String, String>,
    pub database_url: String,
    pub poll_interval_secs: u64,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let tag_label_map = match env::var("TAG_LABEL_MAP") {
            Ok(val) => serde_json::from_str(&val)
                .map_err(|e| ConfigError::Invalid("TAG_LABEL_MAP".into(), e.to_string()))?,
            Err(_) => HashMap::new(),
        };

        Ok(Config {
            discord_token: required("DISCORD_TOKEN")?,
            guild_id: required_u64("DISCORD_GUILD_ID")?,
            feature_requests_channel_id: required_u64("FEATURE_REQUESTS_CHANNEL_ID")?,
            bug_reports_channel_id: required_u64("BUG_REPORTS_CHANNEL_ID")?,
            linear_api_key: required("LINEAR_API_KEY")?,
            linear_team_id: required("LINEAR_TEAM_ID")?,
            linear_feature_label_id: required("LINEAR_FEATURE_LABEL_ID")?,
            linear_bug_label_id: required("LINEAR_BUG_LABEL_ID")?,
            tag_label_map,
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:bot.db".into()),
            poll_interval_secs: env::var("POLL_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
        })
    }

    pub fn is_monitored_channel(&self, channel_id: u64) -> bool {
        channel_id == self.feature_requests_channel_id
            || channel_id == self.bug_reports_channel_id
    }

    pub fn channel_type(&self, channel_id: u64) -> Option<&'static str> {
        if channel_id == self.feature_requests_channel_id {
            Some("feature")
        } else if channel_id == self.bug_reports_channel_id {
            Some("bug")
        } else {
            None
        }
    }

    pub fn primary_label_id(&self, channel_type: &str) -> &str {
        match channel_type {
            "feature" => &self.linear_feature_label_id,
            "bug" => &self.linear_bug_label_id,
            _ => &self.linear_feature_label_id,
        }
    }
}

fn required(name: &str) -> Result<String, ConfigError> {
    env::var(name).map_err(|_| ConfigError::Missing(name.into()))
}

fn required_u64(name: &str) -> Result<u64, ConfigError> {
    let val = required(name)?;
    val.parse()
        .map_err(|_| ConfigError::Invalid(name.into(), format!("not a valid u64: {val}")))
}
