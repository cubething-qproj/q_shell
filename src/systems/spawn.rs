//! Spawns a [`Process`] in response to a [`ShellSpawnMsg`].
//!
//! The terminal entity used for stdio is taken directly from the target [`Shell`].

use crate::prelude::*;
use bevy::platform::collections::HashMap;

fn configure_shell_process_io<T: ShellIo>(world: &mut World, shell: Entity) {
    let terminal_entity = r!(world.get::<Shell<T>>(shell)).term();
    if world.get::<T>(terminal_entity).is_none() {
        world.entity_mut(terminal_entity).insert(T::default());
    }
    let terminal = {
        let mut endpoint_query = world.query_filtered::<(), With<T>>();
        let endpoints = endpoint_query.query(world);
        world
            .resource::<IoComponentCache>()
            .handle::<T>(terminal_entity, &endpoints)
    };
    let terminal = r!(terminal);
    let mut descriptors = r!(world.get_mut::<ProcessFdTable>(shell));
    descriptors.set(FileDescriptor::STDIN, terminal);
    descriptors.set(FileDescriptor::STDOUT, terminal);
    descriptors.set(FileDescriptor::STDERR, terminal);
}

pub(crate) fn configure_shell_process<T: ShellIo>(
    added: On<Add, Shell<T>>,
    mut commands: Commands,
) {
    let shell = added.entity;
    commands.queue(move |world: &mut World| configure_shell_process_io::<T>(world, shell));
}

pub(crate) fn backfill_shell_processes<T: ShellIo>(app: &mut App) {
    let shells = {
        let world = app.world_mut();
        let mut shells = world.query_filtered::<Entity, With<Shell<T>>>();
        shells.iter(world).collect::<Vec<_>>()
    };
    for shell in shells {
        configure_shell_process_io::<T>(app.world_mut(), shell);
    }
}

pub fn spawn_process<T: ShellIo>(
    mut commands: Commands,
    q_shell: Query<(Entity, &Shell<T>)>,
    mut reader: MessageReader<ShellSpawnMsg>,
) {
    for msg in reader.read() {
        let (shell_id, shell) = c!(q_shell.get(msg.shell));
        let terminal_entity = shell.term();
        let prog = msg.prog;
        let argv = msg.argv.clone();
        let environ = msg.environ.clone();

        commands.queue(move |world: &mut World| {
            let terminal = {
                let mut endpoint_query = world.query_filtered::<(), With<T>>();
                let endpoints = endpoint_query.query(world);
                world
                    .resource::<IoComponentCache>()
                    .handle::<T>(terminal_entity, &endpoints)
            };
            let terminal = r!(terminal);

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
                ForegroundInputProcess::new(shell_id),
            );
            debug!("Spawned process {process:?}");
            world.spawn(process);
        });
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::schedule::InternedScheduleLabel;

    use super::*;

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    struct TestProgram;

    q_proc::impl_program_label!(TestProgram, "test");

    fn test_process() -> Process {
        Process {
            prog: TestProgram.intern(),
            signal_overrides: HashMap::new(),
            argv: Vec::new(),
            environ: HashMap::new(),
        }
    }

    #[derive(Component, Default)]
    struct CustomShellIo;

    impl IoComponent for CustomShellIo {
        type Stdin = String;
        type Stdout = String;
    }

    impl ShellIo for CustomShellIo {
        fn add_systems(_app: &mut App, _schedule: InternedScheduleLabel) {}
    }

    fn process_count(app: &mut App) -> usize {
        let world = app.world_mut();
        let mut processes = world.query_filtered::<Entity, (With<Process>, With<ShellJob>)>();
        processes.iter(world).count()
    }

    fn assert_standard_descriptors<T: IoComponent>(descriptors: &ProcessFdTable, terminal: Entity) {
        for fd in [
            FileDescriptor::STDIN,
            FileDescriptor::STDOUT,
            FileDescriptor::STDERR,
        ] {
            let endpoint = descriptors
                .get(fd)
                .expect("the standard descriptor should be open");
            assert_eq!(endpoint.entity(), terminal);
            assert_eq!(endpoint.component_type_id(), std::any::TypeId::of::<T>());
        }
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
    fn shell_and_child_use_the_terminal_endpoint_for_standard_descriptors() {
        let mut app = App::new();
        app.add_plugins((
            ProcessPlugin,
            ShellPlugin::<TerminalIoEndpoint>::default().with_process(test_process()),
        ));

        let terminal = app.world_mut().spawn_empty().id();
        let shell = app
            .world_mut()
            .spawn(Shell::<TerminalIoEndpoint>::new(terminal))
            .id();
        app.world_mut()
            .write_message(ShellSpawnMsg::new(TestProgram, shell));
        app.update();

        assert!(
            app.world()
                .entity(terminal)
                .contains::<TerminalIoEndpoint>()
        );
        let shell_entity = app.world().entity(shell);
        assert_eq!(
            shell_entity
                .get::<Process>()
                .expect("the shell should be a process")
                .prog,
            TestProgram.intern()
        );
        assert_standard_descriptors::<TerminalIoEndpoint>(
            shell_entity
                .get::<ProcessFdTable>()
                .expect("the shell process should have descriptors"),
            terminal,
        );

        let world = app.world_mut();
        let mut processes = world.query_filtered::<(&ProcessFdTable, &ShellJob), With<Process>>();
        let (descriptors, job) = processes
            .single(world)
            .expect("the shell should spawn one child process");
        assert_eq!(job.0, shell);
        assert_standard_descriptors::<TerminalIoEndpoint>(descriptors, terminal);
    }

    #[test]
    fn shell_plugin_accepts_a_custom_io_component() {
        let mut app = App::new();
        app.add_plugins((
            ProcessPlugin,
            ShellPlugin::<TerminalIoEndpoint>::default(),
            ShellPlugin::<CustomShellIo>::default(),
        ));

        let terminal = app.world_mut().spawn_empty().id();
        let shell = app
            .world_mut()
            .spawn(Shell::<CustomShellIo>::new(terminal))
            .id();
        app.world_mut()
            .write_message(ShellSpawnMsg::new(TestProgram, shell));
        app.update();

        assert!(app.world().entity(terminal).contains::<CustomShellIo>());
        assert_standard_descriptors::<CustomShellIo>(
            app.world()
                .entity(shell)
                .get::<ProcessFdTable>()
                .expect("the shell process should have descriptors"),
            terminal,
        );
        let process = {
            let world = app.world_mut();
            let mut processes = world
                .query_filtered::<(Entity, &ProcessFdTable), (With<Process>, With<ShellJob>)>();
            let (process, descriptors) = processes
                .single(world)
                .expect("only the matching shell backend should spawn a child process");
            assert_standard_descriptors::<CustomShellIo>(descriptors, terminal);
            process
        };

        app.world_mut()
            .write_message(ProcessWriteMsg::<String>::stdout(
                process,
                "typed output".to_owned(),
            ));
        app.update();
        let writes = app
            .world_mut()
            .resource_mut::<Messages<EndpointWriteMsg<String>>>()
            .drain()
            .collect::<Vec<_>>();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].process(), process);
        assert_eq!(writes[0].endpoint().entity(), terminal);
        assert_eq!(writes[0].payload(), "typed output");
    }

    #[test]
    fn shell_attaches_io_before_plugin_registration() {
        let mut app = App::new();
        app.add_plugins(ProcessPlugin);

        let terminal = app.world_mut().spawn_empty().id();
        let shell = app
            .world_mut()
            .spawn(Shell::<CustomShellIo>::new(terminal))
            .id();
        assert!(app.world().entity(terminal).contains::<CustomShellIo>());

        app.add_plugins(ShellPlugin::<CustomShellIo>::default());
        assert_standard_descriptors::<CustomShellIo>(
            app.world()
                .entity(shell)
                .get::<ProcessFdTable>()
                .expect("registration should backfill shell descriptors"),
            terminal,
        );
        app.world_mut()
            .write_message(ShellSpawnMsg::new(TestProgram, shell));
        app.update();
        assert_eq!(process_count(&mut app), 1);
    }

    #[test]
    fn unavailable_terminal_endpoint_prevents_process_spawn() {
        let mut app = App::new();
        app.add_plugins((ProcessPlugin, ShellPlugin::<TerminalIoEndpoint>::default()));

        let terminal = app.world_mut().spawn_empty().id();
        let shell = app
            .world_mut()
            .spawn(Shell::<TerminalIoEndpoint>::new(terminal))
            .id();
        app.world_mut()
            .entity_mut(terminal)
            .remove::<TerminalIoEndpoint>();
        app.world_mut()
            .write_message(ShellSpawnMsg::new(TestProgram, shell));
        app.update();

        assert_eq!(process_count(&mut app), 0);
    }

    /// Models a terminal closing in the same frame that its shell requests a process spawn.
    #[test]
    fn endpoint_removal_before_deferred_spawn_does_not_leave_stale_descriptors() {
        let mut app = App::new();
        app.add_plugins((ProcessPlugin, ShellPlugin::<TerminalIoEndpoint>::default()));
        app.add_systems(
            Update,
            remove_terminal_endpoint_once
                .before_ignore_deferred(spawn_process::<TerminalIoEndpoint>),
        );

        let terminal = app.world_mut().spawn_empty().id();
        let shell = app
            .world_mut()
            .spawn(Shell::<TerminalIoEndpoint>::new(terminal))
            .id();
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
