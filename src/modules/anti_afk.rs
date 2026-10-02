use azalea::{
    app::{App, Plugin, Startup},
    ecs::prelude::*,
    interact::SwingArmEvent,
    mining::continue_mining_block,
    packet::game::SendGamePacketEvent,
    prelude::*,
    protocol::packets::game::{ServerboundClientInformation, ServerboundGamePacket},
    ClientInformation, InGameState,
};

use crate::prelude::*;

/// Automatically swing arm to avoid being kicked.
pub struct AntiAfkPlugin;

impl Plugin for AntiAfkPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, Self::change_view_distance)
            .add_systems(
                GameTick,
                Self::handle_anti_afk
                    .after(continue_mining_block)
                    .after(GameTickPlugin::handle_game_ticks),
            );
    }
}

impl AntiAfkPlugin {
    pub fn change_view_distance(mut query: Query<(&mut ClientInformation, &LocalSettings)>) {
        for (mut client_information, local_settings) in &mut query {
            client_information.view_distance = local_settings.anti_afk.view_distance;
        }
    }

    pub fn handle_anti_afk(
        mut query: Query<(
            Entity,
            &LocalSettings,
            &GameTicks,
            &mut ClientInformation,
            Option<&InGameState>,
        )>,
        mut commands: Commands,
    ) {
        for (entity, local_settings, game_ticks, mut client_information, in_game_state) in
            &mut query
        {
            // 仍在 config 阶段（未插入 InGameState）时跳过，
            // 避免 "Tried to send a game packet ... while not in game state"
            if in_game_state.is_none() {
                continue;
            }

            if !local_settings.anti_afk.enabled {
                continue;
            }

            if game_ticks.0 % local_settings.anti_afk.delay_ticks != 0 {
                continue;
            }

            client_information.view_distance = local_settings.anti_afk.view_distance;
            commands.trigger(SendGamePacketEvent {
                sent_by: entity,
                packet:  ServerboundGamePacket::ClientInformation(ServerboundClientInformation {
                    client_information: client_information.clone(),
                }),
            });

            trace!("Anti-Afk Swing Arm Event");
            commands.trigger(SwingArmEvent { entity });
        }
    }
}
