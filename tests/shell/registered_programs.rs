use std::sync::Arc;

use crate::prelude::*;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
struct EchoProgram;
q_proc::impl_program_label!(EchoProgram, "echo");

#[derive(Resource, Default)]
struct Invocations {
    args: Vec<Vec<String>>,
    finish: bool,
}

fn run_echo(
    In(process): In<Entity>,
    processes: Query<&Process>,
    mut invocations: ResMut<Invocations>,
    mut commands: Commands,
    mut writes: MessageWriter<ProcessWriteMsg<Vec<u8>>>,
) {
    let process_info = processes.get(process).unwrap();
    if invocations.args.is_empty() {
        invocations.args.push(process_info.argv.clone());
        writes.write(ProcessWriteMsg::stdout(
            process,
            format!("{}\r\n", process_info.argv.join(" ")).into_bytes(),
        ));
    }
    if invocations.finish {
        commands.entity(process).remove::<Process>();
    }
}

fn setup<L: ShellLanguage>(plugin: ShellPlugin<L>) -> (App, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, plugin));
    let terminal = app
        .world_mut()
        .spawn((Terminal, VtSize { cols: 80, rows: 10 }))
        .id();
    let shell = app
        .world_mut()
        .spawn(Shell::<TerminalIoEndpoint>::new(terminal))
        .id();
    app.insert_resource(ActiveShellKeyboardInput::new(terminal));
    app.update(); // Shell writes its prompt while terminal reflow establishes VtReady.
    app.update(); // The now-ready terminal renders the pending prompt.
    (app, terminal, shell)
}

fn terminal_text(app: &App, terminal: Entity) -> String {
    let world = app.world();
    world
        .get::<VtLineTarget>(terminal)
        .unwrap()
        .entities()
        .iter()
        .filter_map(|entity| world.get::<VtLine>(*entity))
        .map(VtLine::as_string)
        .collect::<String>()
}

fn submit(app: &mut App, terminal: Entity, text: &str) {
    app.world_mut().write_message(TermInputMsg {
        term: terminal,
        input: TermInput::Text(text.to_owned()),
    });
    app.world_mut().write_message(TermInputMsg {
        term: terminal,
        input: TermInput::Submit,
    });
    app.update(); // Line discipline emits ProcessInputMsg in Update.
    app.update(); // q_proc demuxes it in the next First before the shell runs.
}

#[test]
fn default_plugin_dispatches_registered_programs_and_waits_for_jobs() {
    let (mut app, terminal, shell) = setup(ShellPlugin::default());
    app.init_resource::<Invocations>();
    // Registration after the shell is running still makes the program available.
    app.program::<EchoProgram>().add_system(Update, run_echo);
    assert_eq!(terminal_text(&app, terminal).matches("$ ").count(), 1);

    submit(&mut app, terminal, "echo \"hello world\" a\\ b");
    app.update();
    assert_eq!(
        app.world().resource::<Invocations>().args,
        [vec!["hello world".to_owned(), "a b".to_owned()]]
    );
    assert_eq!(terminal_text(&app, terminal).matches("$ ").count(), 1);
    assert!(app.world().get::<ShellJobTarget>(shell).is_some());

    app.world_mut().resource_mut::<Invocations>().finish = true;
    app.update(); // The child exits after the interpreter's program phase.
    app.update(); // The interpreter sees the cleaned-up job and redraws the prompt.
    assert_eq!(terminal_text(&app, terminal).matches("$ ").count(), 2);
}

#[test]
fn errors_and_empty_lines_redraw_without_spawning() {
    let (mut app, terminal, shell) = setup(ShellPlugin::default());
    submit(&mut app, terminal, "missing");
    assert!(terminal_text(&app, terminal).contains("missing: command not found"));
    assert_eq!(terminal_text(&app, terminal).matches("$ ").count(), 2);
    assert!(app.world().get::<ShellJobTarget>(shell).is_none());

    submit(&mut app, terminal, "echo '");
    assert!(terminal_text(&app, terminal).contains("shell: unmatched quote"));
    assert_eq!(terminal_text(&app, terminal).matches("$ ").count(), 3);

    submit(&mut app, terminal, "  ");
    assert_eq!(terminal_text(&app, terminal).matches("$ ").count(), 4);
}

#[test]
fn eof_removes_the_shell_process() {
    let (mut app, terminal, shell) = setup(ShellPlugin::default());
    app.world_mut().write_message(TermInputMsg {
        term: terminal,
        input: TermInput::Eof,
    });
    app.update();
    app.update();
    assert!(app.world().get_entity(shell).is_err());
}

#[derive(Debug)]
struct EchoOnlyLanguage;

impl ShellLanguage for EchoOnlyLanguage {
    fn parse(&self, source: &str) -> Result<ShellIr, ShellParseError> {
        Ok(ShellIr::Command {
            program: "echo".into(),
            arguments: vec![source.trim().into()],
        })
    }
}

#[test]
fn a_custom_language_uses_the_same_registered_program_path() {
    let (mut app, terminal, _) = setup(ShellPlugin::default().with_language(EchoOnlyLanguage));
    app.init_resource::<Invocations>();
    app.program::<EchoProgram>().add_system(Update, run_echo);
    submit(&mut app, terminal, "anything at all");
    app.update();
    assert_eq!(
        app.world().resource::<Invocations>().args,
        [vec!["anything at all".to_owned()]]
    );
}

#[test]
fn invalid_utf8_is_reported_to_stderr() {
    let (mut app, terminal, shell) = setup(ShellPlugin::default());
    app.world_mut()
        .entity_mut(shell)
        .get_mut::<ProcessInputBuffer<TerminalInputPayload>>()
        .unwrap()
        .entry(FileDescriptor::STDIN)
        .or_default()
        .push_back(Arc::new(TerminalInputPayload::Bytes(vec![0xff])));
    app.update();
    assert!(terminal_text(&app, terminal).contains("shell: input is not valid UTF-8"));
}
