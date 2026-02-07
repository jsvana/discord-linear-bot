use serenity::all::{ChannelId, Http};
use sqlx::SqlitePool;
use tracing::info;

use crate::db;
use crate::error::AppError;

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
