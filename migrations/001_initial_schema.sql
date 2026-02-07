CREATE TABLE IF NOT EXISTS sync_mappings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    discord_thread_id TEXT NOT NULL UNIQUE,
    linear_issue_id TEXT NOT NULL UNIQUE,
    linear_identifier TEXT NOT NULL,
    channel_type TEXT NOT NULL CHECK (channel_type IN ('feature', 'bug')),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS linear_status_cache (
    linear_issue_id TEXT PRIMARY KEY,
    status_name TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS backfill_state (
    channel_id TEXT PRIMARY KEY,
    completed INTEGER NOT NULL DEFAULT 0,
    last_thread_id TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
