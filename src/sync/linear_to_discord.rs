use serenity::all::{ChannelId, Http};
use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::db;
use crate::error::AppError;
use crate::linear::client::LinearClient;

const DISCORD_MAX_MESSAGE_CHARS: usize = 2000;

fn split_for_discord(message: &str) -> Vec<String> {
    if message.chars().count() <= DISCORD_MAX_MESSAGE_CHARS {
        return vec![message.to_string()];
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;

    let push_chunk = |chunks: &mut Vec<String>, current: &mut String, current_len: &mut usize| {
        if !current.is_empty() {
            chunks.push(std::mem::take(current));
            *current_len = 0;
        }
    };

    for line in message.split_inclusive('\n') {
        let line_len = line.chars().count();

        if line_len > DISCORD_MAX_MESSAGE_CHARS {
            push_chunk(&mut chunks, &mut current, &mut current_len);

            let mut buf = String::new();
            let mut buf_len = 0usize;
            for c in line.chars() {
                if buf_len == DISCORD_MAX_MESSAGE_CHARS {
                    chunks.push(std::mem::take(&mut buf));
                    buf_len = 0;
                }
                buf.push(c);
                buf_len += 1;
            }
            current = buf;
            current_len = buf_len;
            continue;
        }

        if current_len + line_len > DISCORD_MAX_MESSAGE_CHARS {
            push_chunk(&mut chunks, &mut current, &mut current_len);
        }
        current.push_str(line);
        current_len += line_len;
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

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

        let chunks = split_for_discord(&message);
        let mut first_message_id: Option<String> = None;
        for chunk in &chunks {
            let sent = channel.say(http, chunk).await?;
            if first_message_id.is_none() {
                first_message_id = Some(sent.id.to_string());
            }
        }

        let discord_message_id = first_message_id
            .ok_or_else(|| AppError::Internal("Comment produced no Discord messages".into()))?;

        db::insert_synced_comment(pool, &comment.id, linear_issue_id, &discord_message_id).await?;

        info!(
            comment_id = %comment.id,
            identifier,
            author = %comment.author_name,
            "Synced Linear comment to Discord"
        );
    }

    Ok(())
}
