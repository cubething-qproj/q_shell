//! An interactive shell with a `q` program for querying the World.
//!
//! `q` plans its argument with [`q_query_lang`] against the app's type
//! registry, executes it, and prints the result. Quote the query so the shell
//! passes it through whole:
//!
//! ```text
//! $ q '#Floor | Children[..] | (@ Name ?Health)'
//! ```
//!
//! A prototype: it will move to its own home once a second consumer exists.

use bevy::{prelude::*, window::WindowResolution};
use q_query_lang::QueryPlan;
use q_shell::prelude::*;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
struct QueryProgram;

q_proc::impl_program_label!(QueryProgram, "q");

#[derive(Component, Reflect, Debug)]
#[reflect(Component)]
struct Health(u32);

#[derive(Component, Reflect, Debug)]
#[reflect(Component)]
struct Enemy;

fn main() {
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "q_shell query example".into(),
                resolution: WindowResolution::new(720, 400),
                ..default()
            }),
            ..default()
        }),
        ShellPlugin::default(),
    ));
    app.register_type::<Health>().register_type::<Enemy>();
    app.program::<QueryProgram>().add_system(Update, run_query);
    app.add_systems(Startup, (setup, spawn_scene));
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

/// Something to query besides the shell's own entities.
fn spawn_scene(mut commands: Commands) {
    commands.spawn((
        Name::new("Floor"),
        children![
            (Name::new("grunt"), Enemy, Health(10)),
            (Name::new("archer"), Enemy, Health(6)),
            Name::new("chest"),
        ],
    ));
}

/// Runs `argv` as one query. Exclusive, because executing needs `&mut World`.
fn run_query(In(process): In<Entity>, world: &mut World) {
    let query = r!(world.get::<Process>(process)).argv.join(" ");
    let registry = world.resource::<AppTypeRegistry>().clone();
    let result = QueryPlan::new(&query, &registry.read())
        .map_err(|error| error.to_string())
        .and_then(|plan| plan.execute(world).map_err(|error| error.to_string()));
    let write = match result {
        Ok(result) => ProcessWriteMsg::stdout(process, terminal_bytes(&result.to_string())),
        Err(error) => ProcessWriteMsg::stderr(process, terminal_bytes(&format!("q: {error}\n"))),
    };
    world.write_message(write);
    world.entity_mut(process).remove::<Process>();
}

/// The terminal needs `\r\n` line endings.
fn terminal_bytes(text: &str) -> Vec<u8> {
    text.replace('\n', "\r\n").into_bytes()
}
