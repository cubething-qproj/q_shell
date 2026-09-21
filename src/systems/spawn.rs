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
        let terminal_entity = shell.term;
        let prog = msg.prog;
        let argv = msg.argv.clone();
        let environ = msg.environ.clone();

        commands.queue(move |world: &mut World| {
            let terminal = {
                let mut endpoint_query = world.query_filtered::<(), With<TerminalIoEndpoint>>();
                let endpoints = endpoint_query.query(world);
                world
                    .resource::<IoComponentCache>()
                    .handle::<TerminalIoEndpoint>(terminal_entity, &endpoints)
            };
            let Some(terminal) = terminal else {
                warn!(
                    "Cannot spawn process for shell {shell_id:?}: terminal endpoint is unavailable"
                );
                return;
            };

            let mut descriptors = ProcessFdTable::default();
            descriptors.set(FileDescriptor::STDIN, terminal);
            descriptors.set(FileDescriptor::STDOUT, terminal);
            descriptors.set(FileDescriptor::STDERR, terminal);

            let process = (
                Process {
                    prog,
                    argv,
                    environ,
                    signal_overrides: HashMap::new(), // TODO: How??
                },
                descriptors,
                ShellJob(shell_id),
                ForegroundProcess::new(shell_id),
            );
            debug!("Spawned process {process:?}");
            world.spawn(process);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    struct TestProgram;

    q_proc::impl_program_label!(TestProgram, "test");

    fn process_count(app: &mut App) -> usize {
        let world = app.world_mut();
        let mut processes = world.query_filtered::<Entity, With<Process>>();
        processes.iter(world).count()
    }

    fn remove_terminal_endpoint_once(
        mut commands: Commands,
        endpoints: Query<Entity, With<TerminalIoEndpoint>>,
        mut removed: Local<bool>,
    ) {
        if *removed {
            return;
        }
        *removed = true;
        for endpoint in &endpoints {
            commands.entity(endpoint).remove::<TerminalIoEndpoint>();
        }
    }

    #[test]
    fn shell_process_uses_its_terminal_endpoint_for_standard_descriptors() {
        let mut app = App::new();
        app.add_plugins((ProcessPlugin, ShellPlugin));

        let terminal = app.world_mut().spawn_empty().id();
        let shell = app.world_mut().spawn(Shell { term: terminal }).id();
        app.world_mut()
            .write_message(ShellSpawnMsg::new(TestProgram, shell));
        app.update();

        assert!(
            app.world()
                .entity(terminal)
                .contains::<TerminalIoEndpoint>()
        );

        let world = app.world_mut();
        let mut processes = world.query_filtered::<(&ProcessFdTable, &ShellJob), With<Process>>();
        let (descriptors, job) = processes
            .single(world)
            .expect("the shell should spawn one process");
        assert_eq!(job.0, shell);
        for fd in [
            FileDescriptor::STDIN,
            FileDescriptor::STDOUT,
            FileDescriptor::STDERR,
        ] {
            let endpoint = descriptors
                .get(fd)
                .expect("the standard descriptor should be open");
            assert_eq!(endpoint.entity(), terminal);
            assert_eq!(
                endpoint.component_type_id(),
                std::any::TypeId::of::<TerminalIoEndpoint>()
            );
        }
    }

    #[test]
    fn unavailable_terminal_endpoint_prevents_process_spawn() {
        let mut app = App::new();
        app.add_plugins((ProcessPlugin, ShellPlugin));

        let terminal = app.world_mut().spawn_empty().id();
        let shell = app.world_mut().spawn(Shell { term: terminal }).id();
        app.world_mut()
            .entity_mut(terminal)
            .remove::<TerminalIoEndpoint>();
        app.world_mut()
            .write_message(ShellSpawnMsg::new(TestProgram, shell));
        app.update();

        assert_eq!(process_count(&mut app), 0);
    }

    #[test]
    fn endpoint_removal_before_deferred_spawn_does_not_leave_stale_descriptors() {
        let mut app = App::new();
        app.add_plugins(ProcessPlugin);
        app.add_message::<ShellSpawnMsg>();
        app.register_io_component::<TerminalIoEndpoint>();
        app.add_systems(
            Update,
            (remove_terminal_endpoint_once, spawn_process).chain_ignore_deferred(),
        );

        let terminal = app.world_mut().spawn_empty().id();
        let shell = app.world_mut().spawn(Shell { term: terminal }).id();
        app.world_mut()
            .write_message(ShellSpawnMsg::new(TestProgram, shell));
        app.update();

        assert_eq!(process_count(&mut app), 0);
        app.world_mut()
            .entity_mut(terminal)
            .insert(TerminalIoEndpoint);
        app.update();
        assert_eq!(process_count(&mut app), 0);
    }
}
