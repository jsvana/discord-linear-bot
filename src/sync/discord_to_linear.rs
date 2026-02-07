use serenity::all::{ChannelId, GuildChannel, Http};
use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::config::ChannelConfig;
use crate::db;
use crate::error::AppError;
use crate::linear::client::LinearClient;

pub async fn sync_discord_to_linear(
    http: &Http,
    pool: &SqlitePool,
    channel_config: &ChannelConfig,
    linear: &LinearClient,
    thread: &GuildChannel,
) -> Result<(), AppError> {
    let thread_id = thread.id.to_string();

    // Check for existing mapping (deduplication)
    if db::get_mapping_by_discord_thread(pool, &thread_id)
        .await?
        .is_some()
    {
        info!(thread_id, "Thread already synced, skipping");
        return Ok(());
    }

    let parent_id = thread
        .parent_id
        .ok_or_else(|| AppError::Internal("Thread has no parent channel".into()))?;

    // Fetch first message with retry — race condition where message isn't available yet
    let first_message = fetch_first_message_with_retry(http, thread.id).await;

    let message_body = match &first_message {
        Some(msg) => msg.content.clone(),
        None => "(No message content available)".to_string(),
    };

    // Build label list: primary label + any mapped forum tags
    let mut label_ids = vec![channel_config.linear_label_id.clone()];

    for tag_id in &thread.applied_tags {
        let tag_str = tag_id.to_string();
        if let Some(linear_label_id) = channel_config.tag_label_map.get(&tag_str) {
            label_ids.push(linear_label_id.clone());
        }
    }

    // Upload attachments (best-effort)
    let mut attachment_links = Vec::new();
    if let Some(msg) = &first_message {
        for attachment in &msg.attachments {
            match upload_attachment(linear, &attachment.url, &attachment.filename).await {
                Ok(asset_url) => {
                    attachment_links.push(format!("![{}]({})", attachment.filename, asset_url));
                }
                Err(e) => {
                    warn!(
                        filename = %attachment.filename,
                        error = %e,
                        "Failed to upload attachment, skipping"
                    );
                }
            }
        }
    }

    // Build description
    let thread_url = format!(
        "https://discord.com/channels/{}/{}/{}",
        channel_config.guild_id, parent_id, thread.id
    );

    let mut description = format!("{message_body}\n\n---\n[Discord Thread]({thread_url})");
    if !attachment_links.is_empty() {
        description.push_str("\n\n**Attachments:**\n");
        description.push_str(&attachment_links.join("\n"));
    }

    // Create Linear issue in the configured team
    let title = thread.name.clone();
    let issue = linear
        .create_issue(&channel_config.linear_team_id, &title, &description, &label_ids)
        .await?;

    info!(
        thread_id,
        identifier = %issue.identifier,
        team_id = %channel_config.linear_team_id,
        "Created Linear issue from Discord thread"
    );

    // Store mapping
    db::create_mapping(
        pool,
        &thread_id,
        &issue.id,
        &issue.identifier,
        &channel_config.channel_type,
    )
    .await?;

    // Post confirmation in Discord thread
    let reply = format!(
        "Tracked as **[{}]({})** in Linear",
        issue.identifier, issue.url
    );
    thread.id.say(http, &reply).await?;

    Ok(())
}

async fn fetch_first_message_with_retry(
    http: &Http,
    channel_id: ChannelId,
) -> Option<serenity::model::channel::Message> {
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }

        match channel_id
            .messages(http, serenity::builder::GetMessages::new().limit(1))
            .await
        {
            Ok(messages) => {
                if let Some(msg) = messages.into_iter().next() {
                    return Some(msg);
                }
                warn!(attempt, "No messages found in thread yet, retrying");
            }
            Err(e) => {
                warn!(attempt, error = %e, "Failed to fetch messages, retrying");
            }
        }
    }

    warn!("Failed to fetch first message after 3 attempts");
    None
}

async fn upload_attachment(
    linear: &LinearClient,
    url: &str,
    filename: &str,
) -> Result<String, AppError> {
    let (data, content_type) = linear.download_attachment(url).await?;
    let size = data.len() as u64;

    let upload = linear
        .request_file_upload(filename, &content_type, size)
        .await?;

    linear
        .upload_file_to_url(&upload, data, &content_type)
        .await
}
