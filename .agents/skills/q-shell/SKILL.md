---
name: q-shell
description: Deep dive on q_shell (lib/q_shell), the process-backed shell joining q_proc and q_term — ShellPlugin/ShellBackendPlugin/ShellKeyboardPlugin, Shell and TerminalIoEndpoint, registered programs as commands, ShellLanguage/ShellIr, ShellSpawnMsg, jobs and foreground process groups, terminal input/output bridging, and ShellSystems/ShellIoSystems ordering. Use when adding shell commands, building an in-game console, swapping the shell language, or debugging shell I/O and job control.
---

# q_shell

Source: `lib/q_shell/src` (`plugins.rs`, `language.rs`, `data/shell.rs`,
`systems/{spawn,interpreter,io,input,lifecycle}.rs`). Examples:
`examples/echo_shell.rs` (minimal), `examples/query_shell.rs` (`q` command
backed by q_query_lang). Design history: `.docs/plans/shell-*.md`,
`.docs/handoffs/process-terminal-io-bridge-handoff.md`.
`use q_shell::prelude::*;` re-exports q_proc and q_term preludes too.

Read the `q-proc` skill first; shell commands are just q_proc programs.

## Minimal console

```rust
app.add_plugins((DefaultPlugins, ShellPlugin::default()));   // adds ProcessPlugin + TerminalPlugin if missing
app.program::<EchoProgram>().add_systems(Update, run_echo);  // "echo" is now a command

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let terminal = commands.spawn(Terminal).id();
    commands.insert_resource(ActiveShellKeyboardInput::new(terminal)); // required for keyboard
    commands.spawn((Node { width: vw(100), height: vh(100), ..default() },
                    BackgroundColor(Color::BLACK), VtUi::new(terminal)));
    commands.spawn(Shell::<TerminalIoEndpoint>::new(terminal));
}

fn run_echo(q: Query<(Entity, &Process), With<EchoProgram>>,
            mut out: MessageWriter<ProcessWriteMsg<Vec<u8>>>, mut commands: Commands) {
    for (e, p) in &q {
        out.write(ProcessWriteMsg::stdout(e, format!("{}\r\n", p.argv.join(" ")).into_bytes()));
        commands.entity(e).exit(0);
    }
}
```

Programs needing `&mut World` (e.g. q_query_lang execution) can be exclusive
systems: collect `(Entity, argv)` first, then `world.write_message(...)` and
`world.entity_mut(e).exit(code)` — see `query_shell.rs`.

## How a command runs

1. `ShellKeyboardPlugin` (`TerminalSystems::Input`) maps `KeyboardInput` for
   the `ActiveShellKeyboardInput` terminal to `TermInputMsg`, then canonical
   line discipline echoes and on Enter sends
   `ProcessInputMsg<TerminalInputPayload::Bytes(line + "\n")>` to the
   terminal's foreground input process. Ctrl-D sends `Eof` on an empty line.
2. Next frame, the shell's interpreter (`run_shell::<L>`, program `"shell"`)
   pops stdin, parses with the installed `ShellLanguage`, looks the program up
   in q_proc's `Programs`, writes `ShellSpawnMsg`, and waits until the shell's
   `ShellJobTarget` is empty, then reprints `$ `.
3. `spawn_process` (`ShellSystems::Spawn`) spawns `Process { argv (without the
   program name), … }` with stdin/out/err → the terminal's endpoint, plus
   `ShellJob(shell)`, `ForegroundProcess(shell)`, `ForegroundInputProcess(shell)`.
4. Output: `EndpointWriteMsg<Vec<u8>>` to `TerminalIoEndpoint` →
   `VtWriteMsg::from_peer` (`ShellIoSystems::ProcessOutput`). VT replies →
   foreground process stdin (`ShellIoSystems::TerminalReplies`).

Errors (parse, unknown command, non-UTF-8) go to the shell's stderr.
`EOF` to the shell removes its `Process` → the shell entity despawns, and its
jobs with it (`ShellJobTarget` is `linked_spawn`).

## Key types

- `Shell<T: ShellIo = TerminalIoEndpoint>::new(term)` — immutable relationship
  to the terminal (`ShellTarget<T>` on the terminal, which requires
  `LineDiscipline`). On add, inserts `T` on the terminal and the configured
  shell `Process`; standard fds are wired by an observer.
- `ShellIo`: an `IoComponent + Default` that installs its adapter systems.
  `TerminalIoEndpoint` (Stdin `TerminalInputPayload`, Stdout `Vec<u8>`) is the
  built-in one; `ShellBackendPlugin::<MyIo>` supports custom endpoints.
- `ShellLanguage::parse(&str) -> Result<ShellIr, ShellParseError>` — pure.
  `ShellIr::{Empty, Command { program, arguments }}`. `SimpleShellLanguage`
  uses shlex: quotes/escapes work; `|`, `>`, `;` are **plain arguments**.
  Swap via `ShellPlugin::default().with_language(MyLang)`; only one language
  per app (asserts).
- `ShellPlugin::with_process(p)` / `ShellBackendPlugin::with_process(p)` set
  the shell's process template (e.g. a custom interpreter program).
- Foreground: `ForegroundProcessGroup` (on shell) ⇄ `ForegroundProcess`;
  `ForegroundInputProcessTarget` ⇄ `ForegroundInputProcess` selects the one
  stdin owner. Change atomically with
  `commands.set_foreground_job(shell, processes, Some(input))`. The shell
  projects this into q_term's `VtForegroundProcess`; when the input owner
  goes away, foreground falls back to the shell.

## Ordering (in `update_schedule`, default `Update`)

`TerminalSystems::Input → ShellSystems::Spawn → ShellSystems::Foreground →
ShellIoSystems::SyncForeground → (ApplyDeferred) → ProcessSystems::RunPrograms`;
then `ProcessSystems::Cleanup → ShellIoSystems::Cleanup → (ApplyDeferred) →
ShellIoSystems::ProcessOutput → TerminalSystems::Process →
ShellIoSystems::TerminalReplies → TerminalSystems::Render`.
For a custom `update_schedule`, a program must be registered there or the
process sets are empty (ShellPlugin does this for the interpreter).

## Gotchas

- Add `ShellPlugin` **before** spawning a `Shell`, or the interpreter never
  runs for it.
- No `ActiveShellKeyboardInput` → keyboard input silently ignored.
- Use `\r\n` in output.
- If a process's stdin was redirected away from the terminal, terminal input
  and replies for it are discarded (warn).
- `shell` as a command is rejected (subshells unsupported, TODO #26). Raw
  mode, pipelines, redirection, signals: not implemented. Removing
  `TerminalIoEndpoint` writes `Sig::Hup` messages that nothing consumes yet.
- q_screens cleanup will despawn the terminal/shell on screen change unless
  they're `Persistent`.

Tests: `tests/shell/{io_bridge,registered_programs}.rs` and unit tests in
`systems/*.rs`. Run `just test -p q_shell` from the org root.
