//! The primary [`Plugin`] for q_shell.

use std::marker::PhantomData;

use bevy::ecs::schedule::{ApplyDeferred, InternedScheduleLabel, ScheduleLabel};

use crate::prelude::*;

/// Ordered shell-management phases.
#[derive(SystemSet, Debug, Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ShellSystems {
    /// Spawn requested shell jobs.
    Spawn,
    /// Apply foreground job-control transitions.
    Foreground,
}

/// Ordered shell I/O integration phases.
#[derive(SystemSet, Debug, Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ShellIoSystems {
    /// Project q_shell foreground state into the selected I/O backend.
    SyncForeground,
    /// Clean relationships belonging to removed processes.
    Cleanup,
    /// Translate routed process output into backend writes.
    ProcessOutput,
    /// Translate backend replies into process input.
    TerminalReplies,
}

#[derive(Resource, Default)]
struct ShellLifecycle;

#[derive(Resource)]
pub(crate) struct DefaultShellProcess<T: ShellIo> {
    pub(crate) default_process: Process,
    marker: PhantomData<T>,
}

impl<T: ShellIo> Default for DefaultShellProcess<T> {
    fn default() -> Self {
        Self::new(Process {
            prog: DefaultShellProgram.intern(),
            signal_overrides: Default::default(),
            argv: Vec::new(),
            environ: Default::default(),
        })
    }
}

impl<T: ShellIo> DefaultShellProcess<T> {
    pub fn new(default_process: Process) -> Self {
        Self {
            default_process,
            marker: PhantomData,
        }
    }
}

/// An I/O endpoint component that can back a [`Shell`].
///
/// Implementations install the adapter systems for their message lanes. The
/// component itself is added to each shell's terminal entity.
pub trait ShellIo: IoComponent + Default {
    /// Installs this endpoint's adapter systems in `schedule`.
    fn add_systems(app: &mut App, schedule: InternedScheduleLabel);
}

impl ShellIo for TerminalIoEndpoint {
    fn add_systems(app: &mut App, schedule: InternedScheduleLabel) {
        use crate::systems::io::*;
        app.add_observer(set_shell_foreground);
        app.add_observer(set_process_foreground);
        app.add_observer(clear_process_foreground);
        app.add_observer(fallback_to_shell_foreground);
        app.add_observer(hang_up_terminal);
        backfill_terminal_foreground(app);
        app.add_systems(
            schedule,
            (
                write_terminal_output.in_set(ShellIoSystems::ProcessOutput),
                route_terminal_replies.in_set(ShellIoSystems::TerminalReplies),
            ),
        );
    }
}

/// Registers shell spawning and the selected terminal I/O adapter.
#[derive(Debug)]
pub struct ShellPlugin<T: ShellIo = TerminalIoEndpoint> {
    update_schedule: InternedScheduleLabel,
    process: Process,
    marker: PhantomData<fn() -> T>,
}

impl<T: ShellIo> Default for ShellPlugin<T> {
    fn default() -> Self {
        Self::new(Update)
    }
}

impl<T: ShellIo> ShellPlugin<T> {
    /// Configures the schedule shared by process routing and terminal processing.
    pub fn new(update_schedule: impl ScheduleLabel) -> Self {
        Self {
            update_schedule: update_schedule.intern(),
            process: DefaultShellProcess::<T>::default().default_process,
            marker: PhantomData,
        }
    }

    /// Sets the immutable process template used by subsequently created shells.
    pub fn with_process(mut self, process: Process) -> Self {
        self.process = process;
        self
    }
}

impl<T: ShellIo> Plugin for ShellPlugin<T> {
    fn build(&self, app: &mut App) {
        use crate::systems::{lifecycle::*, spawn::*};
        app.add_message::<ShellSpawnMsg>();
        app.insert_resource(DefaultShellProcess::<T>::new(self.process.clone()));
        app.register_io_component::<T>();
        app.add_observer(configure_shell_process::<T>);
        if !app.world().contains_resource::<ShellLifecycle>() {
            app.init_resource::<ShellLifecycle>();
            app.add_observer(cleanup_removed_process);
        }
        backfill_shell_processes::<T>(app);
        app.configure_sets(
            self.update_schedule,
            (
                TerminalSystems::Input,
                ShellSystems::Spawn,
                ShellSystems::Foreground,
                ShellIoSystems::SyncForeground,
                ProcessSystems::RunPrograms,
            )
                .chain(),
        );
        app.configure_sets(
            self.update_schedule,
            (
                ShellIoSystems::Cleanup.after(ProcessSystems::Cleanup),
                ShellIoSystems::ProcessOutput
                    .after(ShellIoSystems::Cleanup)
                    .before(TerminalSystems::Process),
                ShellIoSystems::TerminalReplies
                    .after(TerminalSystems::Process)
                    .before(TerminalSystems::Render),
            ),
        );
        app.add_systems(
            self.update_schedule,
            (
                spawn_process::<T>.in_set(ShellSystems::Spawn),
                ApplyDeferred
                    .after(ShellIoSystems::SyncForeground)
                    .before(ProcessSystems::RunPrograms),
                ApplyDeferred
                    .after(ShellIoSystems::Cleanup)
                    .before(ShellIoSystems::ProcessOutput),
            ),
        );
        T::add_systems(app, self.update_schedule);
    }
}
