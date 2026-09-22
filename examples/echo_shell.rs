//! A minimal interactive shell running through q_proc and q_term.
//!
//! Type `echo hello world` and press Enter. [`ShellKeyboardPlugin`] handles
//! canonical line editing and fd0 delivery; the shell launches an echo child
//! with [`ShellSpawnMsg`], and both processes write through fd1.

use bevy::{prelude::*, window::WindowResolution};
use q_shell::prelude::*;

const PROMPT: &[u8] = b"$ ";

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
struct EchoShellProgram;

q_proc::impl_program_label!(EchoShellProgram, "echo-shell");

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
struct EchoProgram;

q_proc::impl_program_label!(EchoProgram, "echo");

#[derive(Component, Default)]
struct EchoShellState {
    prompted: bool,
    waiting: bool,
}

fn main() {
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "q_shell echo example".into(),
                resolution: WindowResolution::new(720, 400),
                ..default()
            }),
            ..default()
        }),
        ProcessPlugin,
        TerminalPlugin::default(),
        ShellPlugin::<TerminalIoEndpoint>::default().with_process(Process {
            prog: EchoShellProgram.intern(),
            signal_overrides: default(),
            argv: Vec::new(),
            environ: default(),
        }),
        ShellKeyboardPlugin::default(),
    ));
    app.program::<EchoShellProgram>()
        .add_system(Update, run_shell);
    app.program::<EchoProgram>().add_system(Update, run_echo);
    app.add_systems(Startup, setup);
    app.run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let terminal = commands.spawn(Terminal).id();
    commands.insert_resource(ActiveShellKeyboardInput::new(terminal));
    commands.spawn((
        Node {
            width: vw(100),
            height: vh(100),
            ..default()
        },
        BackgroundColor(Color::BLACK),
        VtUi::new(terminal),
    ));
    commands.spawn((
        Shell::<TerminalIoEndpoint>::new(terminal),
        EchoShellState::default(),
    ));
}

fn run_shell(
    In(shell): In<Entity>,
    mut commands: Commands,
    mut shells: Query<(
        &mut EchoShellState,
        &mut ProcessInputBuffer<TerminalInputPayload>,
    )>,
    jobs: Query<&ShellJobTarget>,
    mut writes: MessageWriter<ProcessWriteMsg<Vec<u8>>>,
) {
    let (mut state, mut input) = r!(shells.get_mut(shell));

    if !state.prompted {
        writes.write(ProcessWriteMsg::stdout(shell, PROMPT.to_vec()));
        state.prompted = true;
    }
    if state.waiting {
        if jobs.get(shell).is_ok_and(|jobs| !jobs.jobs().is_empty()) {
            return;
        }
        writes.write(ProcessWriteMsg::stdout(shell, PROMPT.to_vec()));
        state.waiting = false;
    }

    let Some(line) = input
        .get_mut(&FileDescriptor::STDIN)
        .and_then(|lines| lines.pop_front())
    else {
        return;
    };
    let TerminalInputPayload::Bytes(line) = line.as_ref() else {
        commands.entity(shell).remove::<Process>();
        return;
    };
    let line = String::from_utf8_lossy(line);
    let mut words = line.split_whitespace();
    match words.next() {
        None => {
            writes.write(ProcessWriteMsg::stdout(shell, PROMPT.to_vec()));
        }
        Some("echo") => {
            commands.write_message(ShellSpawnMsg::with_args(
                EchoProgram,
                shell,
                words.map(str::to_owned).collect(),
            ));
            state.waiting = true;
        }
        Some(command) => {
            writes.write(ProcessWriteMsg::stdout(
                shell,
                format!("{command}: command not found\r\n").into_bytes(),
            ));
            writes.write(ProcessWriteMsg::stdout(shell, PROMPT.to_vec()));
        }
    }
}

fn run_echo(
    In(process): In<Entity>,
    processes: Query<&Process>,
    mut writes: MessageWriter<ProcessWriteMsg<Vec<u8>>>,
    mut commands: Commands,
) {
    let process_info = r!(processes.get(process));
    let mut output = process_info.argv.join(" ").into_bytes();
    output.extend_from_slice(b"\r\n");
    writes.write(ProcessWriteMsg::stdout(process, output));
    commands.entity(process).remove::<Process>();
}
