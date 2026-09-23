//! A minimal interactive shell running through q_proc and q_term.
//!
//! Registering the echo program makes `echo "hello world"` available without
//! a hand-written command table. The default [`ShellPlugin`] handles input,
//! dispatch, prompt, and terminal output.

use bevy::{prelude::*, window::WindowResolution};
use q_shell::prelude::*;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
struct EchoProgram;

q_proc::impl_program_label!(EchoProgram, "echo");

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
        ShellPlugin::default(),
    ));
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
    commands.spawn(Shell::<TerminalIoEndpoint>::new(terminal));
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
