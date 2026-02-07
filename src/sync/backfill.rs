use serenity::all::{GuildId, Http};
use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::config::Config;
use crate::db;
use crate::error::AppError;
use crate::linear::client::LinearClient;
use crate::sync::discord_to_linear::sync_discord_to_linear;

pub async fn run_backfill(
    http: &Http,
    pool: &SqlitePool,
    config: &Config,
    linear: &LinearClient,
) -> Result<(), AppError> {
    let channels = [
        (
            config.feature_requests_channel_id,
            "feature_requests",
        ),
        (
            config.bug_reports_channel_id,
            "bug_reports",
        ),
    ];

    for (channel_id, name) in &channels {
        let channel_str = channel_id.to_string();

        // Check if backfill already completed for this channel
        if let Some(state) = db::get_backfill_state(pool, &channel_str).await? {
            if state.completed {
                info!(channel = name, "Backfill already completed, skipping");
                continue;
            }
        }

        info!(channel = name, "Starting backfill");

        match backfill_channel(http, pool, config, linear, *channel_id).await {
            Ok(count) => {
                db::upsert_backfill_state(pool, &channel_str, true, None).await?;
                info!(channel = name, count, "Backfill completed");
            }
            Err(e) => {
                warn!(channel = name, error = %e, "Backfill failed");
            }
        }
    }

    Ok(())
}

async fn backfill_channel(
    http: &Http,
    pool: &SqlitePool,
    config: &Config,
    linear: &LinearClient,
    channel_id: u64,
) -> Result<usize, AppError> {
    let guild_id = GuildId::new(config.guild_id);
    let channel_str = channel_id.to_string();

    // Get resume cursor if we crashed mid-backfill
    let resume_after = if let Some(state) = db::get_backfill_state(pool, &channel_str).await? {
        state.last_thread_id
    } else {
        None
    };

    // Fetch active threads in the guild
    let active_threads = guild_id.get_active_threads(http).await?;

    // Filter to threads in the target forum channel
    let mut threads: Vec<_> = active_threads
        .threads
        .into_iter()
        .filter(|t| {
            t.parent_id
                .map(|p| p.get() == channel_id)
                .unwrap_or(false)
        })
        .collect();

    // Sort by ID (chronological order)
    threads.sort_by_key(|t| t.id);

    // Skip past resume cursor
    if let Some(ref cursor) = resume_after {
        let cursor_id: u64 = cursor.parse().unwrap_or(0);
        threads.retain(|t| t.id.get() > cursor_id);
    }

    let mut synced = 0;

    for thread in &threads {
        let thread_id = thread.id.to_string();

        // Skip already-synced threads
        if db::get_mapping_by_discord_thread(pool, &thread_id)
            .await?
            .is_some()
        {
            continue;
        }

        match sync_discord_to_linear(http, pool, config, linear, thread).await {
            Ok(()) => {
                synced += 1;
                // Persist cursor for crash resilience
                db::upsert_backfill_state(pool, &channel_str, false, Some(&thread_id)).await?;
            }
            Err(e) => {
                warn!(
                    thread_id,
                    thread_name = %thread.name,
                    error = %e,
                    "Failed to backfill thread, continuing"
                );
            }
        }

        // Rate limit: wait between syncs to avoid Discord rate limits
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    Ok(synced)
}
