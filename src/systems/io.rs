//! Translation between routed process I/O and virtual-terminal messages.

use std::any::TypeId;

use crate::prelude::*;

pub(crate) fn set_shell_foreground(
    added: On<Add, Shell<TerminalIoEndpoint>>,
    shells: Query<&Shell<TerminalIoEndpoint>>,
    mut commands: Commands,
) {
    let shell = r!(shells.get(added.entity));
    commands
        .entity(added.entity)
        .insert(VtForegroundProcess::new(shell.term()));
}

pub(crate) fn set_process_foreground(
    added: On<Add, ForegroundInputProcess>,
    foreground: Query<&ForegroundInputProcess>,
    memberships: Query<&ForegroundProcess>,
    shells: Query<&Shell<TerminalIoEndpoint>>,
    mut commands: Commands,
) {
    let foreground = r!(foreground.get(added.entity));
    if !memberships
        .get(added.entity)
        .is_ok_and(|membership| membership.shell() == foreground.shell())
    {
        warn!("Removing foreground input selection outside the foreground process group");
        commands
            .entity(added.entity)
            .remove::<ForegroundInputProcess>();
        return;
    }
    let shell = r!(shells.get(foreground.shell()));
    commands
        .entity(added.entity)
        .insert(VtForegroundProcess::new(shell.term()));
}

pub(crate) fn clear_process_foreground(
    removed: On<Remove, ForegroundProcess>,
    memberships: Query<&ForegroundProcess>,
    foreground: Query<&ForegroundInputProcess>,
    mut commands: Commands,
) {
    let membership = r!(memberships.get(removed.entity));
    if foreground
        .get(removed.entity)
        .is_ok_and(|foreground| foreground.shell() == membership.shell())
    {
        commands
            .entity(removed.entity)
            .remove::<ForegroundInputProcess>();
    }
}

pub(crate) fn fallback_to_shell_foreground(
    removed: On<Remove, ForegroundInputProcess>,
    foreground: Query<&ForegroundInputProcess>,
    mut commands: Commands,
) {
    let shell = r!(foreground.get(removed.entity)).shell();
    commands.queue(move |world: &mut World| {
        if world.get::<ForegroundInputProcessTarget>(shell).is_some() {
            return;
        }
        let term = r!(world.get::<Shell<TerminalIoEndpoint>>(shell)).term();
        if world.get::<TerminalIoEndpoint>(term).is_none() {
            return;
        }
        world
            .entity_mut(shell)
            .insert(VtForegroundProcess::new(term));
    });
}

pub(crate) fn backfill_terminal_foreground(app: &mut App) {
    let foreground = {
        let world = app.world_mut();
        let mut shells = world.query::<(
            Entity,
            &Shell<TerminalIoEndpoint>,
            Option<&ForegroundInputProcessTarget>,
        )>();
        shells
            .iter(world)
            .map(|(shell_entity, shell, process)| {
                (
                    process.map_or(shell_entity, ForegroundInputProcessTarget::process),
                    shell.term(),
                )
            })
            .collect::<Vec<_>>()
    };
    for (peer, term) in foreground {
        app.world_mut()
            .entity_mut(peer)
            .insert(VtForegroundProcess::new(term));
    }
}

pub(crate) fn hang_up_terminal(
    removed: On<Remove, TerminalIoEndpoint>,
    terminals: Query<&ShellTarget<TerminalIoEndpoint>>,
    jobs: Query<(Entity, &ShellJob)>,
    foreground: Query<&VtForegroundProcessTarget>,
    signals: Option<MessageWriter<SignalMsg>>,
    mut commands: Commands,
) {
    let shell = r!(terminals.get(removed.entity)).target();
    if let Ok(foreground) = foreground.get(removed.entity) {
        commands
            .entity(foreground.process())
            .remove::<VtForegroundProcess>();
    }

    let mut signals = r!(signals);
    signals.write(SignalMsg {
        term: removed.entity,
        target: shell,
        signal: Sig::Hup,
    });
    for (job, owner) in &jobs {
        if owner.0 == shell {
            signals.write(SignalMsg {
                term: removed.entity,
                target: job,
                signal: Sig::Hup,
            });
        }
    }
}

pub(crate) fn write_terminal_output(
    mut process_writes: MessageReader<EndpointWriteMsg<Vec<u8>>>,
    terminal_endpoints: Query<(), With<TerminalIoEndpoint>>,
    terminal_writes: Option<MessageWriter<VtWriteMsg>>,
) {
    let mut terminal_writes = r!(terminal_writes);
    for write in process_writes
        .read()
        .filter(|write| write.endpoint().component_type_id() == TypeId::of::<TerminalIoEndpoint>())
    {
        let terminal = write.endpoint().entity();
        if !terminal_endpoints.contains(terminal) {
            warn!("Discarding write to closed terminal endpoint {terminal:?}");
            continue;
        }
        terminal_writes.write(VtWriteMsg::from_peer(
            terminal,
            write.process(),
            write.payload().clone(),
        ));
    }
}

pub(crate) fn route_terminal_replies(
    replies: Option<MessageReader<VtReplyMsg>>,
    terminals: Query<&VtForegroundProcessTarget, With<TerminalIoEndpoint>>,
    processes: Query<&ProcessFdTable, With<Process>>,
    mut input: MessageWriter<ProcessInputMsg<TerminalInputPayload>>,
) {
    let mut replies = r!(replies);
    for reply in replies.read() {
        let foreground = c!(terminals.get(reply.term));
        let process = foreground.process();
        let descriptors = c!(processes.get(process));
        let endpoint = c!(descriptors.get(FileDescriptor::STDIN));
        if endpoint.entity() != reply.term
            || endpoint.component_type_id() != TypeId::of::<TerminalIoEndpoint>()
        {
            warn!("Discarding terminal reply for process {process:?} whose stdin was redirected");
            continue;
        }
        input.write(ProcessInputMsg::new(
            process,
            FileDescriptor::STDIN,
            endpoint,
            TerminalInputPayload::Bytes(reply.bytes.clone()),
        ));
    }
}

#[cfg(test)]
mod tests {
    use bevy::platform::collections::HashMap;

    use super::*;

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    struct TestProgram;

    q_proc::impl_program_label!(TestProgram, "test");

    #[derive(Resource, Default)]
    struct ObservedWrite(Option<VtWriteMsg>);

    fn observe_terminal_write(
        mut writes: MessageReader<VtWriteMsg>,
        mut observed: ResMut<ObservedWrite>,
    ) {
        observed.0 = writes.read().next().cloned();
    }

    fn spawn_shell_process(app: &mut App) -> (Entity, Entity, Entity) {
        let terminal = app.world_mut().spawn_empty().id();
        let shell = app
            .world_mut()
            .spawn(Shell::<TerminalIoEndpoint>::new(terminal))
            .id();
        app.world_mut()
            .write_message(ShellSpawnMsg::new(TestProgram, shell));
        app.update();
        let process = {
            let world = app.world_mut();
            let mut processes = world.query_filtered::<Entity, (With<Process>, With<ShellJob>)>();
            processes
                .single(world)
                .expect("the shell should spawn one process")
        };
        (terminal, shell, process)
    }

    #[test]
    fn process_output_reaches_terminal_processing_in_the_same_update() {
        let mut app = App::new();
        app.add_plugins((
            ProcessPlugin,
            ShellBackendPlugin::<TerminalIoEndpoint>::default(),
        ));
        app.add_message::<VtWriteMsg>();
        app.init_resource::<ObservedWrite>();
        app.add_systems(
            Update,
            observe_terminal_write.in_set(TerminalSystems::Process),
        );

        let terminal = app.world_mut().spawn(TerminalIoEndpoint).id();
        let terminal_handle = {
            let world = app.world_mut();
            let mut endpoints = world.query_filtered::<(), With<TerminalIoEndpoint>>();
            let endpoints = endpoints.query(world);
            world
                .resource::<IoComponentCache>()
                .handle::<TerminalIoEndpoint>(terminal, &endpoints)
                .expect("the terminal endpoint should produce a handle")
        };
        let mut descriptors = ProcessFdTable::default();
        descriptors.set(FileDescriptor::STDOUT, terminal_handle);
        let process = app
            .world_mut()
            .spawn((
                Process {
                    prog: TestProgram.intern(),
                    signal_overrides: HashMap::new(),
                    argv: Vec::new(),
                    environ: HashMap::new(),
                },
                descriptors,
            ))
            .id();

        app.world_mut()
            .write_message(ProcessWriteMsg::<Vec<u8>>::stdout(
                process,
                b"hello".to_vec(),
            ));
        app.update();

        let observed = app
            .world()
            .resource::<ObservedWrite>()
            .0
            .as_ref()
            .expect("terminal processing should observe the write in the same update");
        assert_eq!(observed.term, terminal);
        assert_eq!(observed.from, Some(process));
        assert_eq!(observed.bytes, b"hello");
    }

    #[test]
    fn spawned_process_becomes_the_terminal_foreground_peer() {
        let mut app = App::new();
        app.add_plugins((
            ProcessPlugin,
            ShellBackendPlugin::<TerminalIoEndpoint>::default(),
        ));
        let (terminal, _, process) = spawn_shell_process(&mut app);

        let foreground = app
            .world()
            .entity(terminal)
            .get::<VtForegroundProcessTarget>()
            .expect("the terminal should have a foreground peer");
        assert_eq!(foreground.process(), process);
    }

    #[test]
    fn removing_foreground_membership_clears_input_selection_and_restores_shell() {
        let mut app = App::new();
        app.add_plugins((
            ProcessPlugin,
            ShellBackendPlugin::<TerminalIoEndpoint>::default(),
        ));
        let (terminal, shell, process) = spawn_shell_process(&mut app);

        app.world_mut()
            .entity_mut(process)
            .remove::<ForegroundProcess>();
        app.update();

        let foreground = app
            .world()
            .entity(terminal)
            .get::<VtForegroundProcessTarget>()
            .expect("the terminal should fall back to its shell");
        assert_eq!(foreground.process(), shell);
    }

    #[test]
    fn existing_foreground_selection_is_backfilled_on_plugin_registration() {
        let mut app = App::new();
        app.add_plugins(ProcessPlugin);

        let terminal = app.world_mut().spawn_empty().id();
        let shell = app
            .world_mut()
            .spawn(Shell::<TerminalIoEndpoint>::new(terminal))
            .id();
        let process = app
            .world_mut()
            .spawn((
                Process {
                    prog: TestProgram.intern(),
                    signal_overrides: HashMap::new(),
                    argv: Vec::new(),
                    environ: HashMap::new(),
                },
                ForegroundInputProcess::new(shell),
            ))
            .id();

        app.add_plugins(ShellBackendPlugin::<TerminalIoEndpoint>::default());
        let foreground = app
            .world()
            .entity(terminal)
            .get::<VtForegroundProcessTarget>()
            .expect("the existing foreground selection should be projected");
        assert_eq!(foreground.process(), process);
    }

    fn assert_terminal_close_sends_hup(despawn: bool) {
        let mut app = App::new();
        app.add_plugins((
            ProcessPlugin,
            ShellBackendPlugin::<TerminalIoEndpoint>::default(),
        ));
        let (terminal, shell, process) = spawn_shell_process(&mut app);

        if despawn {
            app.world_mut().despawn(terminal);
        } else {
            app.world_mut()
                .entity_mut(terminal)
                .remove::<TerminalIoEndpoint>();
        }
        app.update();

        let mut targets = app
            .world_mut()
            .resource_mut::<Messages<SignalMsg>>()
            .drain()
            .filter(|signal| signal.signal == Sig::Hup && signal.term == terminal)
            .map(|signal| signal.target)
            .collect::<Vec<_>>();
        targets.sort();
        let mut expected = vec![shell, process];
        expected.sort();
        assert_eq!(targets, expected);
        let process_entity = app.world().entity(process);
        let descriptors = process_entity
            .get::<ProcessFdTable>()
            .expect("the process should still have a descriptor table");
        for fd in [
            FileDescriptor::STDIN,
            FileDescriptor::STDOUT,
            FileDescriptor::STDERR,
        ] {
            assert_eq!(descriptors.get(fd), None);
        }
        assert!(!process_entity.contains::<VtForegroundProcess>());
        if despawn {
            let shell_entity = app.world().entity(shell);
            assert!(shell_entity.contains::<ShellProcess>());
            assert!(!shell_entity.contains::<Shell<TerminalIoEndpoint>>());
            app.world_mut().entity_mut(shell).remove::<Process>();
            app.update();
            assert!(app.world().get_entity(shell).is_err());
            assert!(app.world().get_entity(process).is_err());
        } else {
            assert!(
                app.world()
                    .entity(terminal)
                    .get::<VtForegroundProcessTarget>()
                    .is_none()
            );
            app.world_mut().entity_mut(process).remove::<Process>();
            app.update();
            assert!(
                app.world()
                    .entity(terminal)
                    .get::<VtForegroundProcessTarget>()
                    .is_none(),
                "process cleanup must not restore foreground state on a closed endpoint"
            );
        }
    }

    #[test]
    fn terminal_endpoint_removal_sends_hup_and_closes_descriptors() {
        assert_terminal_close_sends_hup(false);
    }

    #[test]
    fn terminal_despawn_sends_hup_and_closes_descriptors() {
        assert_terminal_close_sends_hup(true);
    }

    #[test]
    fn terminal_reply_reaches_shell_process_when_no_child_is_foreground() {
        let mut app = App::new();
        app.add_plugins((
            ProcessPlugin,
            ShellBackendPlugin::<TerminalIoEndpoint>::default(),
        ));
        app.add_message::<VtReplyMsg>();

        let terminal = app.world_mut().spawn_empty().id();
        let shell = app
            .world_mut()
            .spawn(Shell::<TerminalIoEndpoint>::new(terminal))
            .id();
        app.update();
        assert_eq!(
            app.world()
                .entity(terminal)
                .get::<VtForegroundProcessTarget>()
                .expect("the shell should own the terminal foreground")
                .process(),
            shell
        );

        app.world_mut()
            .write_message(VtReplyMsg::new(terminal, b"shell reply".to_vec()));
        app.update();
        app.update();

        let input = app
            .world()
            .entity(shell)
            .get::<ProcessInputBuffer<TerminalInputPayload>>()
            .expect("the shell process should have a terminal input buffer")
            .get(&FileDescriptor::STDIN)
            .expect("the shell should receive the terminal reply");
        assert_eq!(input.len(), 1);
        assert_eq!(
            input[0].as_ref(),
            &TerminalInputPayload::Bytes(b"shell reply".to_vec())
        );
    }

    #[test]
    fn terminal_reply_reaches_foreground_process_on_the_next_first_pass() {
        let mut app = App::new();
        app.add_plugins((
            ProcessPlugin,
            ShellBackendPlugin::<TerminalIoEndpoint>::default(),
        ));
        app.add_message::<VtReplyMsg>();
        let (terminal, _, process) = spawn_shell_process(&mut app);

        app.world_mut()
            .write_message(VtReplyMsg::new(terminal, b"reply".to_vec()));
        app.update();
        assert!(
            app.world()
                .entity(process)
                .get::<ProcessInputBuffer<TerminalInputPayload>>()
                .expect("the process should have a terminal input buffer")
                .get(&FileDescriptor::STDIN)
                .is_none()
        );

        app.update();
        let input = app
            .world()
            .entity(process)
            .get::<ProcessInputBuffer<TerminalInputPayload>>()
            .expect("the process should have a terminal input buffer")
            .get(&FileDescriptor::STDIN)
            .expect("the next First pass should demux the reply");
        assert_eq!(input.len(), 1);
        assert_eq!(
            input[0].as_ref(),
            &TerminalInputPayload::Bytes(b"reply".to_vec())
        );
    }
}
