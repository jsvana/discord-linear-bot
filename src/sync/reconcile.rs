use std::collections::HashMap;

use serenity::all::{Channel, ChannelId, EditThread, GuildId, Http};
use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::config::Config;
use crate::db;
use crate::error::AppError;
use crate::linear::client::LinearClient;
use crate::sync::discord_to_linear::sync_discord_to_linear;

const BATCH_SIZE: usize = 100;

/// Scan every monitored forum channel for active threads that have no Linear mapping and
/// create the missing issues. This is the safety net for `thread_create` events and backfill
/// creates that failed (transient errors, rate limits, the Free-plan issue cap, or a
/// gateway-missed event): a one-shot failure no longer orphans a post permanently — the next
/// reconcile pass picks it up. Deduplication lives in `sync_discord_to_linear`, so already-mapped
/// threads cost only a DB lookup.
///
/// Only active (non-archived) Discord threads are scanned, matching backfill. A post that is
/// archived in Discord before its issue is created won't be picked up; the reconcile interval is
/// expected to be well under Discord's forum auto-archive duration.
pub async fn reconcile_discord_to_linear(
    http: &Http,
    pool: &SqlitePool,
    config: &Config,
    linear: &LinearClient,
) -> Result<(), AppError> {
    let mut created = 0usize;
    let mut failed = 0usize;

    for guild_id in config.unique_guild_ids() {
        let guild = GuildId::new(guild_id);
        let active = match guild.get_active_threads(http).await {
            Ok(a) => a,
            Err(e) => {
                warn!(guild_id, error = %e, "Failed to fetch active threads for reconcile");
                continue;
            }
        };

        for thread in &active.threads {
            // Only threads in a monitored forum channel.
            let parent_id = match thread.parent_id {
                Some(p) => p.get(),
                None => continue,
            };
            let channel_config = match config.channel_config(parent_id) {
                Some(c) => c,
                None => continue,
            };

            // Skip threads already mapped (cheap DB lookup, no Linear call).
            match db::get_mapping_by_discord_thread(pool, &thread.id.to_string()).await {
                Ok(Some(_)) => continue,
                Ok(None) => {}
                Err(e) => {
                    warn!(thread_id = %thread.id, error = %e, "DB lookup failed during reconcile");
                    continue;
                }
            }

            match sync_discord_to_linear(http, pool, channel_config, linear, thread).await {
                Ok(()) => {
                    created += 1;
                    info!(
                        thread_id = %thread.id,
                        thread_name = %thread.name,
                        "Reconcile created missing Linear issue"
                    );
                }
                Err(e) => {
                    failed += 1;
                    warn!(
                        thread_id = %thread.id,
                        thread_name = %thread.name,
                        error = %e,
                        "Reconcile failed to create Linear issue, will retry next pass"
                    );
                }
            }

            // Rate limit between creates to avoid Discord/Linear bursts.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    if created > 0 || failed > 0 {
        info!(created, failed, "Discord→Linear reconcile pass complete");
    }

    Ok(())
}

/// Reconcile Discord thread archive state with current Linear issue status for every
/// tracked issue. Runs silently (no status messages posted); intended for startup so
/// past completions don't require manual cleanup. Also primes the status cache so the
/// poller doesn't fire spurious transitions immediately after.
pub async fn reconcile_archive_state(
    http: &Http,
    pool: &SqlitePool,
    linear: &LinearClient,
) -> Result<(), AppError> {
    let mappings = db::get_all_tracked_issues(pool).await?;
    if mappings.is_empty() {
        info!("No tracked issues; skipping reconcile pass");
        return Ok(());
    }

    info!(count = mappings.len(), "Reconciling archive state");

    // Fetch current Linear state for all tracked issues in batches.
    let mut status_by_id: HashMap<String, (String, String)> = HashMap::new();
    for chunk in mappings.chunks(BATCH_SIZE) {
        let ids: Vec<String> = chunk.iter().map(|m| m.linear_issue_id.clone()).collect();
        match linear.get_issues_by_ids(&ids).await {
            Ok(issues) => {
                for issue in issues {
                    status_by_id.insert(issue.id, (issue.status_name, issue.status_type));
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to fetch issue batch from Linear");
            }
        }
    }

    let mut archived_count = 0usize;
    let mut unarchived_count = 0usize;
    let mut already_correct = 0usize;
    let mut missing_in_linear = 0usize;

    for mapping in &mappings {
        let (status_name, status_type) = match status_by_id.get(&mapping.linear_issue_id) {
            Some(s) => s.clone(),
            None => {
                missing_in_linear += 1;
                continue;
            }
        };

        let desired_archived = status_type == "completed";

        let thread_id: u64 = match mapping.discord_thread_id.parse() {
            Ok(id) => id,
            Err(_) => {
                warn!(
                    thread_id = %mapping.discord_thread_id,
                    identifier = %mapping.linear_identifier,
                    "Invalid Discord thread id in mapping"
                );
                continue;
            }
        };
        let channel = ChannelId::new(thread_id);

        // Read the thread's current archive state so we only write when it differs.
        let current_archived = match channel.to_channel(http).await {
            Ok(Channel::Guild(gc)) => gc.thread_metadata.map(|m| m.archived).unwrap_or(false),
            Ok(_) => {
                warn!(
                    identifier = %mapping.linear_identifier,
                    thread_id = %mapping.discord_thread_id,
                    "Mapped channel is not a guild channel"
                );
                continue;
            }
            Err(e) => {
                warn!(
                    identifier = %mapping.linear_identifier,
                    thread_id = %mapping.discord_thread_id,
                    error = %e,
                    "Failed to fetch Discord channel"
                );
                continue;
            }
        };

        // Prime the status cache regardless of archive action.
        if let Err(e) = db::upsert_cached_status(pool, &mapping.linear_issue_id, &status_name).await
        {
            warn!(
                identifier = %mapping.linear_identifier,
                error = %e,
                "Failed to prime status cache"
            );
        }

        if current_archived == desired_archived {
            already_correct += 1;
            continue;
        }

        if let Err(e) = channel
            .edit_thread(http, EditThread::new().archived(desired_archived))
            .await
        {
            warn!(
                identifier = %mapping.linear_identifier,
                thread_id = %mapping.discord_thread_id,
                archived = desired_archived,
                error = %e,
                "Failed to update Discord thread archive state"
            );
            continue;
        }

        if desired_archived {
            archived_count += 1;
        } else {
            unarchived_count += 1;
        }
        info!(
            identifier = %mapping.linear_identifier,
            status = %status_name,
            archived = desired_archived,
            "Reconciled Discord thread archive state"
        );
    }

    info!(
        archived = archived_count,
        unarchived = unarchived_count,
        already_correct,
        missing_in_linear,
        total = mappings.len(),
        "Reconcile pass complete"
    );

    Ok(())
}
