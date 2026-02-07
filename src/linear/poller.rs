use std::sync::Arc;

use serenity::http::Http;
use sqlx::SqlitePool;
use tracing::{error, info, warn};

use crate::db;
use crate::linear::client::LinearClient;
use crate::sync::linear_to_discord::sync_linear_to_discord;

pub async fn run_poller(
    http: Arc<Http>,
    pool: SqlitePool,
    linear: LinearClient,
    interval_secs: u64,
) {
    // Start polling from now
    let mut last_poll = chrono::Utc::now().to_rfc3339();

    info!(interval_secs, "Starting Linear status poller");

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;

        let now = chrono::Utc::now().to_rfc3339();

        match linear.get_updated_issues(&last_poll).await {
            Ok(issues) => {
                if !issues.is_empty() {
                    info!(count = issues.len(), "Polled updated issues from Linear");
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
                    match db::get_cached_status(&pool, &issue.id).await {
                        Ok(Some(cached)) if cached == issue.status_name => continue,
                        Ok(_) => {}
                        Err(e) => {
                            warn!(
                                issue_id = %issue.id,
                                error = %e,
                                "Failed to check status cache"
                            );
                            continue;
                        }
                    }

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

                last_poll = now;
            }
            Err(e) => {
                error!(error = %e, "Failed to poll Linear for updates");
                // Don't advance last_poll on failure — retry same window next tick
            }
        }
    }
}
