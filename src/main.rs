mod config;
mod db;
mod discord;
mod error;
mod linear;
mod sync;

use std::sync::Arc;

use serenity::all::GatewayIntents;
use serenity::Client;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;
use tracing::{error, info};

use crate::config::Config;
use crate::discord::handler::{AppState, AppStateKey, Handler};
use crate::linear::client::LinearClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "discord_linear_bot=info".into()),
        )
        .init();

    dotenvy::dotenv().ok();

    let config = Config::from_env()?;
    info!(
        channels = config.channels.len(),
        teams = config.unique_team_ids().len(),
        guilds = config.unique_guild_ids().len(),
        "Configuration loaded"
    );

    // SQLite pool + migrations
    let connect_options = SqliteConnectOptions::from_str(&config.database_url)?
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(connect_options)
        .await?;

    sqlx::raw_sql(include_str!("../migrations/001_initial_schema.sql"))
        .execute(&pool)
        .await?;
    sqlx::raw_sql(include_str!("../migrations/002_comment_sync.sql"))
        .execute(&pool)
        .await?;

    info!("Database initialized");

    let linear_client = LinearClient::new(config.linear_api_key.clone());

    let app_state = Arc::new(AppState {
        config: config.clone(),
        pool: pool.clone(),
        linear_client: linear_client.clone(),
    });

    // Build Discord client
    let intents = GatewayIntents::GUILDS | GatewayIntents::MESSAGE_CONTENT;
    let mut discord_client = Client::builder(&config.discord_token, intents)
        .event_handler(Handler)
        .await?;

    // Store app state in serenity TypeMap
    {
        let mut data = discord_client.data.write().await;
        data.insert::<AppStateKey>(app_state.clone());
    }

    let discord_http = discord_client.http.clone();

    // Run backfill before starting live sync
    info!("Running backfill...");
    if let Err(e) =
        sync::backfill::run_backfill(&discord_http, &pool, &config, &linear_client).await
    {
        error!(error = %e, "Backfill failed, continuing with live sync");
    }

    // Spawn Linear status poller for all teams
    let team_ids = config.unique_team_ids();
    let poller_handle = tokio::spawn(linear::poller::run_poller(
        discord_http,
        pool,
        linear_client,
        team_ids,
        config.poll_interval_secs,
    ));

    // Run Discord gateway + poller concurrently
    tokio::select! {
        result = discord_client.start() => {
            if let Err(e) = result {
                error!(error = %e, "Discord client error");
            }
        }
        _ = poller_handle => {
            error!("Linear poller unexpectedly ended");
        }
    }

    Ok(())
}
