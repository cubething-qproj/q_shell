//! Spawns a [`Process`] in response to a [`ShellSpawnMsg`].
//!
//! The terminal entity used for stdio is taken directly from
//! `Shell { term }`; this system does not query `q_term` types.

use crate::prelude::*;
use bevy::platform::collections::HashMap;

pub fn spawn_process(
    mut commands: Commands,
    q_shell: Query<(Entity, &Shell)>,
    mut reader: MessageReader<ShellSpawnMsg>,
) {
    for msg in reader.read() {
        let (shell_id, shell) = c!(q_shell.get(msg.shell));
        let term_id = shell.term;
        let mut entt = commands.spawn_empty();
        let val = (
            Process {
                prog: msg.prog,
                argv: msg.argv.clone(),
                environ: msg.environ.clone(),
                fd0: term_id,
                fd1: term_id,
                fd2: term_id,
                signal_overrides: HashMap::new(), // TODO: How??
            },
            ShellJob(shell_id),
            ForegroundProcess::new(shell_id),
        );
        debug!("Spawned process {val:?}");
        entt.insert(val);
    }
}
