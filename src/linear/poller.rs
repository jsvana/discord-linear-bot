use std::sync::Arc;

use serenity::http::Http;
use sqlx::SqlitePool;
use tracing::{error, info, warn};

use crate::db;
use crate::linear::client::LinearClient;
use crate::sync::linear_to_discord::{sync_linear_comments_to_discord, sync_linear_to_discord};

pub async fn run_poller(
    http: Arc<Http>,
    pool: SqlitePool,
    linear: LinearClient,
    team_ids: Vec<String>,
    interval_secs: u64,
) {
    let mut last_poll = chrono::Utc::now().to_rfc3339();

    info!(
        interval_secs,
        teams = team_ids.len(),
        "Starting Linear status poller"
    );

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;

        let now = chrono::Utc::now().to_rfc3339();
        let mut any_success = false;

        for team_id in &team_ids {
            match linear.get_updated_issues(team_id, &last_poll).await {
                Ok(issues) => {
                    any_success = true;

                    if !issues.is_empty() {
                        info!(
                            count = issues.len(),
                            team_id,
                            "Polled updated issues from Linear"
                        );
                    }

                    for issue in &issues {
                        // Only process issues we're tracking
                        match db::get_mapping_by_linear_issue(&pool, &issue.id).await {
                            Ok(Some(_)) => {}
                            Ok(None) => continue,
                            Err(e) => {
                                warn!(issue_id = %issue.id, error = %e, "DB lookup failed");
                                continue;
                            }
                        };

                        // Check if status actually changed from what we last posted
                        let status_changed = match db::get_cached_status(&pool, &issue.id).await {
                            Ok(Some(cached)) if cached == issue.status_name => false,
                            Ok(_) => true,
                            Err(e) => {
                                warn!(
                                    issue_id = %issue.id,
                                    error = %e,
                                    "Failed to check status cache"
                                );
                                false
                            }
                        };

                        if status_changed {
                            info!(
                                identifier = %issue.identifier,
                                status = %issue.status_name,
                                "Status change detected"
                            );

                            if let Err(e) = sync_linear_to_discord(
                                &http,
                                &pool,
                                &issue.id,
                                &issue.identifier,
                                &issue.status_name,
                            )
                            .await
                            {
                                error!(
                                    identifier = %issue.identifier,
                                    error = %e,
                                    "Failed to sync status to Discord"
                                );
                            }
                        }

                        // Sync any new comments for this issue
                        if let Err(e) = sync_linear_comments_to_discord(
                            &http,
                            &pool,
                            &linear,
                            &issue.id,
                            &issue.identifier,
                        )
                        .await
                        {
                            error!(
                                identifier = %issue.identifier,
                                error = %e,
                                "Failed to sync comments to Discord"
                            );
                        }
                    }
                }
                Err(e) => {
                    error!(team_id, error = %e, "Failed to poll Linear for updates");
                }
            }
        }

        // Only advance the cursor if at least one team succeeded
        if any_success {
            last_poll = now;
        }
    }
}
