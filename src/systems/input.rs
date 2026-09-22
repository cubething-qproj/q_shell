//! Optional host-keyboard and terminal line-discipline integration.

use std::any::TypeId;

use bevy::input::{ButtonState, keyboard::KeyboardInput};

use crate::prelude::*;

pub(crate) fn keyboard_input(
    mut keyboard: MessageReader<KeyboardInput>,
    active: Option<Res<ActiveShellKeyboardInput>>,
    terminals: Query<(), With<ShellTarget<TerminalIoEndpoint>>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut input: MessageWriter<TermInputMsg>,
) {
    use bevy::input::keyboard::Key;

    let terminal = r!(active).terminal();
    if !terminals.contains(terminal) {
        warn!("The active shell keyboard terminal has no attached shell");
        return;
    }
    let control = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    for event in keyboard.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        let input_value = match &event.logical_key {
            Key::Character(text) if control && text.eq_ignore_ascii_case("d") => TermInput::Eof,
            Key::Enter => TermInput::Submit,
            Key::Backspace => TermInput::Erase,
            Key::Space => TermInput::Text(" ".to_owned()),
            Key::Character(text) => TermInput::Text(text.to_string()),
            _ => continue,
        };
        input.write(TermInputMsg {
            term: terminal,
            input: input_value,
        });
    }
}

pub(crate) fn process_line_input(
    mut input: MessageReader<TermInputMsg>,
    mut terminals: Query<&mut LineDiscipline, With<ShellTarget<TerminalIoEndpoint>>>,
    foreground: Query<&VtForegroundProcessTarget>,
    processes: Query<&ProcessFdTable, With<Process>>,
    mut process_input: MessageWriter<ProcessInputMsg<TerminalInputPayload>>,
    mut terminal_output: MessageWriter<VtWriteMsg>,
) {
    for message in input.read() {
        let mut discipline = c!(terminals.get_mut(message.term));
        match (&mut *discipline, &message.input) {
            (LineDiscipline::Raw, _) => {
                warn!("Raw shell keyboard input is not implemented");
            }
            (LineDiscipline::Canonical { buffer }, TermInput::Text(text)) => {
                buffer.push_str(text);
                terminal_output.write(VtWriteMsg::new(message.term, text.as_bytes().to_vec()));
            }
            (LineDiscipline::Canonical { buffer }, TermInput::Erase) => {
                if buffer.pop().is_some() {
                    terminal_output.write(VtWriteMsg::new(message.term, b"\x08 \x08".to_vec()));
                }
            }
            (LineDiscipline::Canonical { buffer }, TermInput::Submit) => {
                let mut bytes = std::mem::take(buffer).into_bytes();
                bytes.push(b'\n');
                terminal_output.write(VtWriteMsg::new(message.term, b"\r\n".to_vec()));
                deliver_input(
                    message.term,
                    TerminalInputPayload::Bytes(bytes),
                    &foreground,
                    &processes,
                    &mut process_input,
                );
            }
            (LineDiscipline::Canonical { buffer }, TermInput::Eof) => {
                let payload = if buffer.is_empty() {
                    TerminalInputPayload::Eof
                } else {
                    TerminalInputPayload::Bytes(std::mem::take(buffer).into_bytes())
                };
                deliver_input(
                    message.term,
                    payload,
                    &foreground,
                    &processes,
                    &mut process_input,
                );
            }
            (_, _) => {
                warn!("Unsupported terminal input operation");
            }
        }
    }
}

fn deliver_input(
    terminal: Entity,
    payload: TerminalInputPayload,
    foreground: &Query<&VtForegroundProcessTarget>,
    processes: &Query<&ProcessFdTable, With<Process>>,
    input: &mut MessageWriter<ProcessInputMsg<TerminalInputPayload>>,
) {
    let process = r!(foreground.get(terminal)).process();
    let descriptors = r!(processes.get(process));
    let endpoint = r!(descriptors.get(FileDescriptor::STDIN));
    if endpoint.entity() != terminal
        || endpoint.component_type_id() != TypeId::of::<TerminalIoEndpoint>()
    {
        warn!("Discarding terminal input for process {process:?} whose stdin was redirected");
        return;
    }
    input.write(ProcessInputMsg::new(
        process,
        FileDescriptor::STDIN,
        endpoint,
        payload,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_input_echoes_and_reaches_the_shell_process() {
        let mut app = App::new();
        app.add_plugins((
            ProcessPlugin,
            ShellPlugin::<TerminalIoEndpoint>::default(),
            ShellKeyboardPlugin::default(),
        ));
        app.add_message::<VtWriteMsg>();

        let terminal = app.world_mut().spawn_empty().id();
        app.insert_resource(ActiveShellKeyboardInput::new(terminal));
        let shell = app
            .world_mut()
            .spawn(Shell::<TerminalIoEndpoint>::new(terminal))
            .id();
        app.update();

        app.world_mut().write_message(TermInputMsg {
            term: terminal,
            input: TermInput::Text("echo hello".to_owned()),
        });
        app.world_mut().write_message(TermInputMsg {
            term: terminal,
            input: TermInput::Submit,
        });
        app.update();
        app.update();

        let input = app
            .world()
            .entity(shell)
            .get::<ProcessInputBuffer<TerminalInputPayload>>()
            .expect("the shell should have a terminal input buffer")
            .get(&FileDescriptor::STDIN)
            .expect("the submitted line should reach shell stdin");
        assert_eq!(input.len(), 1);
        assert_eq!(
            input[0].as_ref(),
            &TerminalInputPayload::Bytes(b"echo hello\n".to_vec())
        );
        assert!(matches!(
            app.world().entity(terminal).get::<LineDiscipline>(),
            Some(LineDiscipline::Canonical { buffer }) if buffer.is_empty()
        ));

        let echo = app
            .world_mut()
            .resource_mut::<Messages<VtWriteMsg>>()
            .drain()
            .flat_map(|write| write.bytes)
            .collect::<Vec<_>>();
        assert_eq!(echo, b"echo hello\r\n");
    }

    #[test]
    fn canonical_eof_flushes_text_then_signals_eof_when_empty() {
        let mut app = App::new();
        app.add_plugins((
            ProcessPlugin,
            ShellPlugin::<TerminalIoEndpoint>::default(),
            ShellKeyboardPlugin::default(),
        ));

        let terminal = app.world_mut().spawn_empty().id();
        let shell = app
            .world_mut()
            .spawn(Shell::<TerminalIoEndpoint>::new(terminal))
            .id();
        app.update();

        app.world_mut().write_message(TermInputMsg {
            term: terminal,
            input: TermInput::Text("partial".to_owned()),
        });
        app.world_mut().write_message(TermInputMsg {
            term: terminal,
            input: TermInput::Eof,
        });
        app.world_mut().write_message(TermInputMsg {
            term: terminal,
            input: TermInput::Eof,
        });
        app.update();
        app.update();

        let input = app
            .world()
            .entity(shell)
            .get::<ProcessInputBuffer<TerminalInputPayload>>()
            .expect("the shell should have a terminal input buffer")
            .get(&FileDescriptor::STDIN)
            .expect("EOF input should reach shell stdin");
        assert_eq!(input.len(), 2);
        assert_eq!(
            input[0].as_ref(),
            &TerminalInputPayload::Bytes(b"partial".to_vec())
        );
        assert_eq!(input[1].as_ref(), &TerminalInputPayload::Eof);
    }
}
