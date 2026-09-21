//! Shell relationship cleanup for removed processes.

use crate::prelude::*;

pub(crate) fn cleanup_removed_process(removed: On<Remove, Process>, mut commands: Commands) {
    let process = removed.entity;
    commands.queue(move |world: &mut World| {
        let mut process = r!(world.get_entity_mut(process));
        process.remove::<(
            ShellJob,
            ForegroundProcess,
            ForegroundInputProcess,
            VtForegroundProcess,
        )>();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    struct TestProgram;

    q_proc::impl_program_label!(TestProgram, "test");

    #[derive(Resource)]
    struct ForegroundProbe {
        shell: Entity,
        process: Entity,
        terminal: Entity,
        transitioned: bool,
        observed: bool,
    }

    fn queue_foreground_transition(mut commands: Commands, mut probe: ResMut<ForegroundProbe>) {
        if probe.transitioned {
            return;
        }
        probe.transitioned = true;
        commands
            .entity(probe.process)
            .insert(ForegroundInputProcess::new(probe.shell));
    }

    fn observe_foreground_transition(
        foreground: Query<&VtForegroundProcessTarget>,
        mut probe: ResMut<ForegroundProbe>,
    ) {
        probe.observed = foreground
            .get(probe.terminal)
            .is_ok_and(|foreground| foreground.process() == probe.process);
    }

    #[derive(Resource)]
    struct FinalWriteProbe {
        process: Entity,
        terminal: Entity,
        exited: bool,
        observed: bool,
    }

    fn exit_with_final_write(
        mut commands: Commands,
        mut writes: MessageWriter<ProcessWriteMsg<Vec<u8>>>,
        mut probe: ResMut<FinalWriteProbe>,
    ) {
        if probe.exited {
            return;
        }
        probe.exited = true;
        writes.write(ProcessWriteMsg::stdout(probe.process, b"final".to_vec()));
        commands.entity(probe.process).remove::<Process>();
    }

    fn observe_final_write_admission(
        mut writes: MessageReader<VtWriteMsg>,
        foreground: Query<&VtForegroundProcessTarget>,
        mut probe: ResMut<FinalWriteProbe>,
    ) {
        for write in writes.read() {
            if write.term != probe.terminal || write.bytes != b"final" {
                continue;
            }
            let foreground = r!(foreground.get(probe.terminal));
            probe.observed = write.from != Some(foreground.process());
        }
    }

    fn assert_process_cleanup(despawn: bool) {
        let mut app = App::new();
        app.add_plugins((ProcessPlugin, ShellPlugin::<TerminalIoEndpoint>::default()));

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
            let mut processes = world.query_filtered::<Entity, With<Process>>();
            processes.single(world).expect("one process should spawn")
        };

        if despawn {
            app.world_mut().despawn(process);
        } else {
            app.world_mut().entity_mut(process).remove::<Process>();
        }
        app.update();

        if !despawn {
            let process_entity = app.world().entity(process);
            assert!(!process_entity.contains::<ShellJob>());
            assert!(!process_entity.contains::<ForegroundProcess>());
            assert!(!process_entity.contains::<ForegroundInputProcess>());
            assert!(!process_entity.contains::<VtForegroundProcess>());
        }
        let foreground = app
            .world()
            .entity(terminal)
            .get::<VtForegroundProcessTarget>()
            .expect("the shell should regain the terminal foreground");
        assert_eq!(foreground.process(), shell);
    }

    #[test]
    fn foreground_barrier_exposes_the_selected_process_before_programs() {
        let mut app = App::new();
        app.add_plugins((ProcessPlugin, ShellPlugin::<TerminalIoEndpoint>::default()));

        let terminal = app.world_mut().spawn_empty().id();
        let shell = app
            .world_mut()
            .spawn(Shell::<TerminalIoEndpoint>::new(terminal))
            .id();
        let process = app
            .world_mut()
            .spawn(Process {
                prog: TestProgram.intern(),
                signal_overrides: Default::default(),
                argv: Vec::new(),
                environ: Default::default(),
            })
            .id();
        app.insert_resource(ForegroundProbe {
            shell,
            process,
            terminal,
            transitioned: false,
            observed: false,
        });
        app.add_systems(
            Update,
            (
                queue_foreground_transition.in_set(ShellSystems::Foreground),
                observe_foreground_transition.in_set(ProcessSystems::RunPrograms),
            ),
        );

        app.update();
        assert!(app.world().resource::<ForegroundProbe>().observed);
    }

    #[test]
    fn cleanup_barrier_updates_foreground_before_final_output_admission() {
        let mut app = App::new();
        app.add_plugins((ProcessPlugin, ShellPlugin::<TerminalIoEndpoint>::default()));
        app.add_message::<VtWriteMsg>();

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
            let mut processes = world.query_filtered::<Entity, With<Process>>();
            processes.single(world).expect("one process should spawn")
        };
        app.insert_resource(FinalWriteProbe {
            process,
            terminal,
            exited: false,
            observed: false,
        });
        app.add_systems(
            Update,
            (
                exit_with_final_write.in_set(ProcessSystems::RunPrograms),
                observe_final_write_admission.in_set(TerminalSystems::Process),
            ),
        );

        app.update();
        assert!(app.world().resource::<FinalWriteProbe>().observed);
    }

    #[test]
    fn process_removal_cleans_shell_relationships_and_restores_fallback() {
        assert_process_cleanup(false);
    }

    #[test]
    fn process_despawn_cleans_shell_relationships_and_restores_fallback() {
        assert_process_cleanup(true);
    }
}
