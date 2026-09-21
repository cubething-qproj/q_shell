//! The primary [`Plugin`] for q_shell.

use std::marker::PhantomData;

use bevy::ecs::schedule::{InternedScheduleLabel, ScheduleLabel};

use crate::prelude::*;

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
        app.add_observer(fallback_to_shell_foreground);
        backfill_terminal_foreground(app);
        app.add_systems(
            schedule,
            (
                write_terminal_output
                    .after(ProcessSystems::RouteWrites)
                    .before(TerminalSystems::Process),
                route_terminal_replies.after(TerminalSystems::Process),
            ),
        );
    }
}

/// Registers shell spawning and the selected terminal I/O adapter.
#[derive(Debug)]
pub struct ShellPlugin<T: ShellIo = TerminalIoEndpoint> {
    update_schedule: InternedScheduleLabel,
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
            marker: PhantomData,
        }
    }
}

impl<T: ShellIo> Plugin for ShellPlugin<T> {
    fn build(&self, app: &mut App) {
        use crate::systems::spawn::*;
        app.add_message::<ShellSpawnMsg>();
        app.register_io_component::<T>();
        app.add_systems(self.update_schedule, spawn_process::<T>);
        T::add_systems(app, self.update_schedule);
    }
}
