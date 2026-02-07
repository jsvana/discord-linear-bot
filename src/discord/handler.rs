use serenity::all::{Context, EventHandler, GuildChannel, Ready};
use serenity::async_trait;
use sqlx::SqlitePool;
use tracing::{error, info};

use crate::config::Config;
use crate::linear::client::LinearClient;
use crate::sync::discord_to_linear::sync_discord_to_linear;

pub struct AppState {
    pub config: Config,
    pub pool: SqlitePool,
    pub linear_client: LinearClient,
}

pub struct Handler;

impl Handler {
    async fn get_state(ctx: &Context) -> Option<std::sync::Arc<AppState>> {
        let data = ctx.data.read().await;
        data.get::<AppStateKey>().cloned()
    }
}

pub struct AppStateKey;

impl serenity::prelude::TypeMapKey for AppStateKey {
    type Value = std::sync::Arc<AppState>;
}

#[async_trait]
impl EventHandler for Handler {
    async fn thread_create(&self, ctx: Context, thread: GuildChannel) {
        let state = match Self::get_state(&ctx).await {
            Some(s) => s,
            None => {
                error!("AppState not found in TypeMap");
                return;
            }
        };

        // Check if thread is in a monitored forum channel
        let parent_id = match thread.parent_id {
            Some(id) => id.get(),
            None => return,
        };

        if !state.config.is_monitored_channel(parent_id) {
            return;
        }

        info!(
            thread_id = %thread.id,
            thread_name = %thread.name,
            parent_id,
            "New forum post detected"
        );

        if let Err(e) = sync_discord_to_linear(
            &ctx.http,
            &state.pool,
            &state.config,
            &state.linear_client,
            &thread,
        )
        .await
        {
            error!(
                thread_id = %thread.id,
                error = %e,
                "Failed to sync thread to Linear"
            );
        }
    }

    async fn ready(&self, _ctx: Context, ready: Ready) {
        info!(user = %ready.user.name, "Discord bot connected");
    }
}
