//! The default interpreter consumes one parsed command and waits for its job.

use crate::{language::*, prelude::*};

const PROMPT: &[u8] = b"$ ";

#[derive(Component, Default)]
pub(crate) struct ShellExecution {
    phase: ShellPhase,
}

#[derive(Clone, Copy, Default)]
enum ShellPhase {
    #[default]
    Ready,
    Waiting,
}

pub(crate) fn run_shell<L: ShellLanguage>(
    mut commands: Commands,
    mut shells: Query<
        (
            Entity,
            &Process,
            &mut ProcessInputBuffer<TerminalInputPayload>,
            Option<&mut ShellExecution>,
        ),
        With<Shell<TerminalIoEndpoint>>,
    >,
    jobs: Query<&ShellJobTarget>,
    programs: Res<Programs>,
    language: Res<ShellLanguageConfig<L>>,
    mut writes: MessageWriter<ProcessWriteMsg<Vec<u8>>>,
) {
    for (shell, process, mut input, execution) in &mut shells {
        if process.prog != DefaultShellProgram.intern() {
            continue;
        }
        let Some(mut execution) = execution else {
            commands.entity(shell).insert(ShellExecution::default());
            prompt(shell, &mut writes);
            continue;
        };

        match execution.phase {
            ShellPhase::Waiting if jobs.get(shell).is_ok_and(|jobs| !jobs.jobs().is_empty()) => {
                continue;
            }
            ShellPhase::Waiting => {
                execution.phase = ShellPhase::Ready;
                prompt(shell, &mut writes);
                continue;
            }
            ShellPhase::Ready => {}
        }

        let Some(payload) = input
            .get_mut(&FileDescriptor::STDIN)
            .and_then(|lines| lines.pop_front())
        else {
            continue;
        };
        let TerminalInputPayload::Bytes(bytes) = payload.as_ref() else {
            commands.entity(shell).remove::<Process>();
            continue;
        };
        let source = match std::str::from_utf8(bytes) {
            Ok(source) => source,
            Err(_) => {
                write_error(shell, "shell: input is not valid UTF-8", &mut writes);
                prompt(shell, &mut writes);
                continue;
            }
        };
        match language.0.parse(source) {
            Ok(ShellIr::Empty) => prompt(shell, &mut writes),
            Ok(ShellIr::Command { program, arguments }) => {
                if let Some(program_label) = programs.get_by_name(&program) {
                    commands.write_message(ShellSpawnMsg::with_args(
                        program_label,
                        shell,
                        arguments,
                    ));
                    execution.phase = ShellPhase::Waiting;
                } else {
                    write_error(shell, &format!("{program}: command not found"), &mut writes);
                    prompt(shell, &mut writes);
                }
            }
            Err(error) => {
                write_error(shell, &format!("shell: {error}"), &mut writes);
                prompt(shell, &mut writes);
            }
        }
    }
}

fn prompt(shell: Entity, writes: &mut MessageWriter<ProcessWriteMsg<Vec<u8>>>) {
    writes.write(ProcessWriteMsg::stdout(shell, PROMPT.to_vec()));
}

fn write_error(shell: Entity, message: &str, writes: &mut MessageWriter<ProcessWriteMsg<Vec<u8>>>) {
    writes.write(ProcessWriteMsg::stderr(
        shell,
        format!("{message}\r\n").into_bytes(),
    ));
}
