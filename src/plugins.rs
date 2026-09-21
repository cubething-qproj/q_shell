//! The primary [`Plugin`] for q_shell.

use crate::prelude::*;

/// Registers shell messages and the [`spawn_process`] system.
#[derive(Debug)]
pub struct ShellPlugin;
impl Plugin for ShellPlugin {
    fn build(&self, app: &mut App) {
        use crate::systems::spawn::*;
        app.add_message::<ShellSpawnMsg>();
        app.register_io_component::<TerminalIoEndpoint>();
        app.add_systems(Update, spawn_process);
    }
}
