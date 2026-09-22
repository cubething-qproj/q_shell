use crate::prelude::*;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
struct BridgeProgram;

q_proc::impl_program_label!(BridgeProgram, "bridge-test");

#[derive(Resource, Default)]
struct BridgeState {
    enabled: bool,
    sent: bool,
    reply: Vec<u8>,
}

#[derive(Resource, Default)]
struct TerminalText(String);

fn bridge_program(
    In(process): In<Entity>,
    mut writes: MessageWriter<ProcessWriteMsg<Vec<u8>>>,
    mut buffers: Query<&mut ProcessInputBuffer<TerminalInputPayload>>,
    mut state: ResMut<BridgeState>,
) {
    if state.enabled && !state.sent {
        state.sent = true;
        writes.write(ProcessWriteMsg::stdout(process, b"hello\x1b[5n".to_vec()));
    }

    let mut buffer = r!(buffers.get_mut(process));
    for payload in buffer.remove(&FileDescriptor::STDIN).unwrap_or_default() {
        if let TerminalInputPayload::Bytes(bytes) = payload.as_ref() {
            state.reply.extend(bytes);
        }
    }
}

fn capture_terminal_text(
    terminals: Query<TermInfo>,
    lines: Query<(Entity, &VtLine)>,
    mut text: ResMut<TerminalText>,
) {
    let terminal = r!(terminals.single());
    text.0 = terminal
        .lines(&lines)
        .map(|(_, line)| line.as_string())
        .collect();
}

#[test]
fn shell_process_output_and_vt_replies_cross_the_full_bridge() {
    let mut app = get_test_app();
    app.add_plugins(
        ShellPlugin::<TerminalIoEndpoint>::default().with_process(Process {
            prog: BridgeProgram.intern(),
            signal_overrides: Default::default(),
            argv: Vec::new(),
            environ: Default::default(),
        }),
    );
    app.init_resource::<BridgeState>();
    app.init_resource::<TerminalText>();
    app.program::<BridgeProgram>()
        .add_system(Update, bridge_program);
    app.add_systems(PostUpdate, capture_terminal_text);

    let terminal = app
        .world_mut()
        .spawn((Terminal, VtSize { cols: 20, rows: 5 }))
        .id();
    let shell = app
        .world_mut()
        .spawn(Shell::<TerminalIoEndpoint>::new(terminal))
        .id();

    app.update();
    assert!(app.world().entity(shell).contains::<Process>());
    assert!(app.world().entity(terminal).contains::<VtReady>());

    app.world_mut().resource_mut::<BridgeState>().enabled = true;
    app.update();
    assert_eq!(app.world().resource::<TerminalText>().0, "hello");
    assert!(app.world().resource::<BridgeState>().reply.is_empty());

    app.update();
    assert_eq!(app.world().resource::<BridgeState>().reply, b"\x1b[0n");
}
