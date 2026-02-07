use serenity::all::{ChannelId, Http};
use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::db;
use crate::error::AppError;
use crate::linear::client::LinearClient;

pub async fn sync_linear_to_discord(
    http: &Http,
    pool: &SqlitePool,
    linear_issue_id: &str,
    identifier: &str,
    new_status: &str,
) -> Result<(), AppError> {
    // Look up Discord thread from mapping
    let mapping = db::get_mapping_by_linear_issue(pool, linear_issue_id)
        .await?
        .ok_or_else(|| AppError::Internal(format!("No mapping for issue {identifier}")))?;

    // Post status update in Discord thread
    let thread_id: u64 = mapping
        .discord_thread_id
        .parse()
        .map_err(|_| AppError::Internal("Invalid discord thread id".into()))?;

    let channel = ChannelId::new(thread_id);
    let message = format!("**{identifier}** status changed to **{new_status}**");

    channel.say(http, &message).await?;

    // Update status cache
    db::upsert_cached_status(pool, linear_issue_id, new_status).await?;

    info!(
        linear_issue_id,
        identifier,
        status = new_status,
        "Posted status update to Discord"
    );

    Ok(())
}

pub async fn sync_linear_comments_to_discord(
    http: &Http,
    pool: &SqlitePool,
    linear: &LinearClient,
    linear_issue_id: &str,
    identifier: &str,
) -> Result<(), AppError> {
    let mapping = match db::get_mapping_by_linear_issue(pool, linear_issue_id).await? {
        Some(m) => m,
        None => return Ok(()),
    };

    let thread_id: u64 = mapping
        .discord_thread_id
        .parse()
        .map_err(|_| AppError::Internal("Invalid discord thread id".into()))?;
    let channel = ChannelId::new(thread_id);

    let comments = linear.get_issue_comments(linear_issue_id).await?;

    for comment in &comments {
        match db::is_comment_synced(pool, &comment.id).await {
            Ok(true) => continue,
            Ok(false) => {}
            Err(e) => {
                warn!(
                    comment_id = %comment.id,
                    error = %e,
                    "Failed to check comment sync status"
                );
                continue;
            }
        }

        let message = format!(
            "**{}** commented on **{}**:\n> {}",
            comment.author_name,
            identifier,
            comment.body.replace('\n', "\n> ")
        );

        let sent = channel.say(http, &message).await?;

        db::insert_synced_comment(
            pool,
            &comment.id,
            linear_issue_id,
            &sent.id.to_string(),
        )
        .await?;

        info!(
            comment_id = %comment.id,
            identifier,
            author = %comment.author_name,
            "Synced Linear comment to Discord"
        );
    }

    Ok(())
}
