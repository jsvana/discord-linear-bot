use std::collections::HashMap;

use serenity::all::{Channel, ChannelId, EditThread, Http};
use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::db;
use crate::error::AppError;
use crate::linear::client::LinearClient;

const BATCH_SIZE: usize = 100;

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
