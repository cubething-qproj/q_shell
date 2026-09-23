//! The primary [`Plugin`] for q_shell.

use std::{any::TypeId, marker::PhantomData, sync::Mutex};

use bevy::{
    ecs::schedule::{ApplyDeferred, InternedScheduleLabel, ScheduleLabel},
    input::keyboard::KeyboardInput,
};

use crate::{
    language::{ShellLanguage, ShellLanguageConfig, SimpleShellLanguage},
    prelude::*,
};

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
struct InstalledShellLanguage(TypeId);

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
pub struct ShellBackendPlugin<T: ShellIo = TerminalIoEndpoint> {
    update_schedule: InternedScheduleLabel,
    process: Process,
    marker: PhantomData<fn() -> T>,
}

impl<T: ShellIo> Default for ShellBackendPlugin<T> {
    fn default() -> Self {
        Self::new(Update)
    }
}

impl<T: ShellIo> ShellBackendPlugin<T> {
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

/// Installs a terminal-backed shell with a configurable language.
#[derive(Debug)]
pub struct ShellPlugin<L: ShellLanguage = SimpleShellLanguage> {
    update_schedule: InternedScheduleLabel,
    process: Process,
    language: Mutex<Option<L>>,
}

impl Default for ShellPlugin<SimpleShellLanguage> {
    fn default() -> Self {
        Self::new(Update)
    }
}

impl ShellPlugin<SimpleShellLanguage> {
    /// Configures the update schedule for terminal input and shell processing.
    pub fn new(update_schedule: impl ScheduleLabel) -> Self {
        Self {
            update_schedule: update_schedule.intern(),
            process: DefaultShellProcess::<TerminalIoEndpoint>::default().default_process,
            language: Mutex::new(Some(SimpleShellLanguage)),
        }
    }
}

impl<L: ShellLanguage> ShellPlugin<L> {
    /// Replaces the shell language while retaining the schedule and process template.
    pub fn with_language<M: ShellLanguage>(self, language: M) -> ShellPlugin<M> {
        ShellPlugin {
            update_schedule: self.update_schedule,
            process: self.process,
            language: Mutex::new(Some(language)),
        }
    }

    /// Sets the immutable process template used by subsequently created shells.
    pub fn with_process(mut self, process: Process) -> Self {
        self.process = process;
        self
    }
}

impl<L: ShellLanguage> Plugin for ShellPlugin<L> {
    fn build(&self, app: &mut App) {
        if let Some(installed) = app.world().get_resource::<InstalledShellLanguage>() {
            assert_eq!(
                installed.0,
                TypeId::of::<L>(),
                "only one shell language can be installed"
            );
        }
        app.insert_resource(InstalledShellLanguage(TypeId::of::<L>()));

        if !app.is_plugin_added::<ProcessPlugin>() {
            app.add_plugins(ProcessPlugin);
        }
        if !app.is_plugin_added::<TerminalPlugin>() {
            app.add_plugins(TerminalPlugin::default());
        }
        if !app.is_plugin_added::<ShellBackendPlugin<TerminalIoEndpoint>>() {
            app.add_plugins(
                ShellBackendPlugin::<TerminalIoEndpoint>::new(self.update_schedule)
                    .with_process(self.process.clone()),
            );
        }
        if !app.is_plugin_added::<ShellKeyboardPlugin>() {
            app.add_plugins(ShellKeyboardPlugin::new(self.update_schedule));
        }

        let language = self
            .language
            .lock()
            .expect("shell language lock was poisoned")
            .take()
            .expect("shell plugin was already built");
        app.insert_resource(ShellLanguageConfig(language));
        app.add_systems(
            self.update_schedule,
            crate::systems::interpreter::run_shell::<L>.in_set(ProcessSystems::RunPrograms),
        );
    }
}

/// Maps the active marked terminal's host keyboard through canonical q_term line discipline to fd0.
///
/// Submitted input is emitted during the configured schedule and becomes visible
/// to process programs after q_proc demultiplexes it in the next frame's `First`
/// schedule. Raw-mode keyboard encoding is intentionally deferred until TUI
/// requirements are known.
#[derive(Debug)]
pub struct ShellKeyboardPlugin {
    update_schedule: InternedScheduleLabel,
}

impl Default for ShellKeyboardPlugin {
    fn default() -> Self {
        Self::new(Update)
    }
}

impl ShellKeyboardPlugin {
    /// Configures the schedule shared by terminal input and shell processing.
    pub fn new(update_schedule: impl ScheduleLabel) -> Self {
        Self {
            update_schedule: update_schedule.intern(),
        }
    }
}

impl Plugin for ShellKeyboardPlugin {
    fn build(&self, app: &mut App) {
        use crate::systems::input::*;
        app.add_message::<KeyboardInput>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.add_message::<TermInputMsg>();
        app.add_message::<VtWriteMsg>();
        app.add_systems(
            self.update_schedule,
            (keyboard_input, process_line_input)
                .chain()
                .in_set(TerminalSystems::Input),
        );
    }
}

impl<T: ShellIo> Plugin for ShellBackendPlugin<T> {
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
