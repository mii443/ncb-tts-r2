//! Interaction handlers module
//!
//! This module provides centralized routing for all Discord interaction types:
//! - Commands (slash commands)
//! - Modals (form submissions)
//! - Buttons (button clicks)
//! - Select menus (dropdown selections)

pub mod buttons;
pub mod commands;
pub mod modals;
pub mod select_menus;
pub mod utils;

use crate::errors::Result;
use serenity::{all::Interaction, prelude::Context};

/// Main interaction router
/// Routes all interaction types to their respective handlers
pub async fn handle_interaction(ctx: &Context, interaction: &Interaction) -> Result<()> {
    match interaction {
        Interaction::Command(command) => {
            commands::handle_command(ctx, command).await
        }
        Interaction::Modal(modal) => modals::handle_modal(ctx, modal).await,
        Interaction::Component(component) => {
            // Route based on component type
            use serenity::all::ComponentInteractionDataKind;
            match &component.data.kind {
                ComponentInteractionDataKind::Button => {
                    buttons::handle_button(ctx, component).await
                }
                ComponentInteractionDataKind::StringSelect { .. } => {
                    select_menus::handle_select_menu(ctx, component).await
                }
                _ => Ok(()),
            }
        }
        _ => Ok(()),
    }
}
