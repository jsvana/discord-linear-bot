use sqlx::sqlite::SqlitePool;
use sqlx::FromRow;

#[derive(Debug, FromRow)]
pub struct SyncMapping {
    pub id: i64,
    pub discord_thread_id: String,
    pub linear_issue_id: String,
    pub linear_identifier: String,
    pub channel_type: String,
    pub created_at: String,
}

#[derive(Debug, FromRow)]
pub struct LinearStatusCache {
    pub linear_issue_id: String,
    pub status_name: String,
    pub updated_at: String,
}

#[derive(Debug, FromRow)]
pub struct BackfillState {
    pub channel_id: String,
    pub completed: bool,
    pub last_thread_id: Option<String>,
    pub updated_at: String,
}

pub async fn get_mapping_by_discord_thread(
    pool: &SqlitePool,
    discord_thread_id: &str,
) -> Result<Option<SyncMapping>, sqlx::Error> {
    sqlx::query_as::<_, SyncMapping>(
        "SELECT id, discord_thread_id, linear_issue_id, linear_identifier, channel_type, created_at
         FROM sync_mappings WHERE discord_thread_id = ?",
    )
    .bind(discord_thread_id)
    .fetch_optional(pool)
    .await
}

pub async fn get_mapping_by_linear_issue(
    pool: &SqlitePool,
    linear_issue_id: &str,
) -> Result<Option<SyncMapping>, sqlx::Error> {
    sqlx::query_as::<_, SyncMapping>(
        "SELECT id, discord_thread_id, linear_issue_id, linear_identifier, channel_type, created_at
         FROM sync_mappings WHERE linear_issue_id = ?",
    )
    .bind(linear_issue_id)
    .fetch_optional(pool)
    .await
}

pub async fn create_mapping(
    pool: &SqlitePool,
    discord_thread_id: &str,
    linear_issue_id: &str,
    linear_identifier: &str,
    channel_type: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO sync_mappings (discord_thread_id, linear_issue_id, linear_identifier, channel_type)
         VALUES (?, ?, ?, ?)",
    )
    .bind(discord_thread_id)
    .bind(linear_issue_id)
    .bind(linear_identifier)
    .bind(channel_type)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_cached_status(
    pool: &SqlitePool,
    linear_issue_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT status_name FROM linear_status_cache WHERE linear_issue_id = ?",
    )
    .bind(linear_issue_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.0))
}

pub async fn upsert_cached_status(
    pool: &SqlitePool,
    linear_issue_id: &str,
    status_name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO linear_status_cache (linear_issue_id, status_name, updated_at)
         VALUES (?, ?, datetime('now'))
         ON CONFLICT(linear_issue_id) DO UPDATE SET status_name = excluded.status_name, updated_at = excluded.updated_at",
    )
    .bind(linear_issue_id)
    .bind(status_name)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn is_comment_synced(
    pool: &SqlitePool,
    linear_comment_id: &str,
) -> Result<bool, sqlx::Error> {
    let row: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM synced_comments WHERE linear_comment_id = ?",
    )
    .bind(linear_comment_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

pub async fn insert_synced_comment(
    pool: &SqlitePool,
    linear_comment_id: &str,
    linear_issue_id: &str,
    discord_message_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO synced_comments (linear_comment_id, linear_issue_id, discord_message_id)
         VALUES (?, ?, ?)",
    )
    .bind(linear_comment_id)
    .bind(linear_issue_id)
    .bind(discord_message_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_backfill_state(
    pool: &SqlitePool,
    channel_id: &str,
) -> Result<Option<BackfillState>, sqlx::Error> {
    sqlx::query_as::<_, BackfillState>(
        "SELECT channel_id, completed, last_thread_id, updated_at
         FROM backfill_state WHERE channel_id = ?",
    )
    .bind(channel_id)
    .fetch_optional(pool)
    .await
}

pub async fn upsert_backfill_state(
    pool: &SqlitePool,
    channel_id: &str,
    completed: bool,
    last_thread_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO backfill_state (channel_id, completed, last_thread_id, updated_at)
         VALUES (?, ?, ?, datetime('now'))
         ON CONFLICT(channel_id) DO UPDATE SET
           completed = excluded.completed,
           last_thread_id = excluded.last_thread_id,
           updated_at = excluded.updated_at",
    )
    .bind(channel_id)
    .bind(completed)
    .bind(last_thread_id)
    .execute(pool)
    .await?;
    Ok(())
}

