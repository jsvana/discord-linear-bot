CREATE TABLE IF NOT EXISTS synced_comments (
    linear_comment_id TEXT PRIMARY KEY,
    linear_issue_id TEXT NOT NULL,
    discord_message_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
