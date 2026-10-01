#![cfg_attr(bevy_lint, feature(register_tool), register_tool(bevy))]
#![feature(trivial_bounds)]

#[macro_use]
extern crate derive_more;
#[macro_use]
extern crate lazy_regex;
#[macro_use]
extern crate serde_with;
#[macro_use]
extern crate str_macro;
#[macro_use]
extern crate strum;
#[macro_use]
extern crate tracing;

pub mod prelude;

pub mod commands;
pub mod modules;
pub mod parsers;
pub mod settings;
pub mod trackers;

use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::Result;
use azalea::{
    DefaultPlugins,
    app::{PluginGroup, PluginGroupBuilder},
    bot::DefaultBotPlugins,
    ecs::prelude::*,
    pong::PongPlugin,
    prelude::*,
    swarm::{DefaultSwarmPlugins, prelude::*},
};
#[cfg(feature = "via")]
use azalea_viaversion::ViaVersionPlugin;
use bevy_discord::DiscordBotPlugin;
#[cfg(feature = "bot")]
use bevy_discord::config::DiscordBotConfig;
use parking_lot::RwLock;
#[cfg(feature = "bot")]
use serenity::prelude::*;
use smart_default::SmartDefault;

use crate::prelude::*;

/// # Create and start the Minecraft bot client
///
/// # Errors
/// Will return `Err` if `ClientBuilder::start` fails.
#[allow(clippy::future_not_send)]
pub async fn start() -> Result<()> {
    let global_settings = GlobalSettings::load()?;
    global_settings.save()?; /* Save settings on first-run */

    #[allow(unused_mut)]
    let mut client = SwarmBuilder::new_without_plugins()
        .set_swarm_handler(swarm_handler)
        .add_plugins((
            PluginGroupBuilder::disable::<PongPlugin>(DefaultPlugins.build()),
            DefaultBotPlugins,
            DefaultSwarmPlugins,
            CommandsPluginGroup,
            MinecraftParserPlugin,
            ModulesPluginGroup,
            SettingsPluginGroup,
            TrackersPluginGroup,
        ));

    #[cfg(feature = "api")]
    if global_settings.http_api.enabled {
        client = client.add_plugins(HttpApiParserPlugin);
    }

    #[cfg(feature = "bot")]
    if !global_settings.discord_token.is_empty() {
        let gateway_intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;
        let configuration = DiscordBotConfig::default()
            .gateway_intents(gateway_intents)
            .token(global_settings.discord_token.clone());

        client = client.add_plugins((DiscordBotPlugin::new(configuration), DiscordParserPlugin));
    }

    /* Logger for distributed rate limits via webhooks */
    if !global_settings.logger.webhooks.is_empty() {
        client = client.add_plugins(LoggerPlugin);
    }

    /* ViaProxy for multi-version compatibility */
    #[cfg(feature = "via")]
    if !global_settings.server_version.is_empty() {
        client = client.add_plugins(ViaVersionPlugin::start(&global_settings.server_version).await);
    }

    client
        .start(global_settings.server_address.to_string())
        .await;
    Ok(())
}

#[derive(Clone, Resource, SmartDefault)]
pub struct SwarmState {
    auto_reconnect: Arc<RwLock<HashMap<String, (bool, u64)>>>,
}

/// # Errors
/// Will return `Err` if `Swarm::add_with_opts` fails.
pub async fn swarm_handler(swarm: Swarm, event: SwarmEvent, state: SwarmState) -> Result<()> {
    match event {
        SwarmEvent::Init => swarm.ecs.write().insert_resource(state),
        SwarmEvent::Chat(chat_packet) => {
            let message = chat_packet.message();
            if message.to_string().contains("Position in queue: ") {
                return Ok(()); /* 2B2T Queue */
            }

            println!("{}", message.to_ansi());
        }
        SwarmEvent::Disconnect(ref account, ref join_opts) => loop {
            let bot_name = account.username().to_lowercase();
            let Some((rejoin, secs)) = state.auto_reconnect.read().get(&bot_name).copied() else {
                state
                    .auto_reconnect
                    .write()
                    .insert(bot_name.to_lowercase(), (false, 5));

                continue; /* AutoReconnect: Missing */
            };

            tokio::time::sleep(Duration::from_secs(secs)).await;

            if !rejoin {
                continue; /* AutoReconnect: Disabled */
            }

            info!("AutoReconnecting on {}", account.username());
            swarm.add_with_opts(account, state.clone(), join_opts).await;

            break;
        },
        _ => {}
    }

    Ok(())
}
