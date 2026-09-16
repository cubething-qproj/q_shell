use q_term::msgs::term::{apply_scroll, process_input};

use crate::prelude::*;

/// Counts how many `TermRedrawRequestedMsg` entries the in-test reader has
/// observed across all frames.
#[derive(Resource, Default, Debug)]
struct MutatedReadCount(u32);

/// Reader system installed in `TerminalSystems::Process` after
/// `process_input`. Counts every `TermRedrawRequestedMsg` it sees.
fn count_mutations(
    mut reader: MessageReader<TermRedrawRequestedMsg>,
    mut count: ResMut<MutatedReadCount>,
) {
    for _ in reader.read() {
        count.0 += 1;
    }
}

/// Verifies that a `TermStdOut` written from `Startup` produces exactly
/// one `TermRedrawRequestedMsg` visible to a reader sitting after
/// `process_input` in the `TerminalSystems::Process` chain on the same
/// frame. Exercises the writer to reader seam inside `Process`.
#[test]
fn process_chain_emits_buffer_mutated_in_order() {
    let mut app = get_test_app();
    app.init_resource::<MutatedReadCount>();

    app.add_systems(Startup, |mut commands: Commands| {
        let term_id = commands.spawn(Terminal).id();
        // VtSize must be present synchronously, otherwise `process_input`
        // drops the input on the first frame (TermInfo unresolvable).
        commands
            .entity(term_id)
            .insert(VtSize { cols: 80, rows: 24 });
        commands.write_message(StdOut::write(term_id, "x"));
    });

    // Pin the reader strictly between `process_input` and the rest of the
    // chain. Without `.before(apply_scroll)` Bevy is free to schedule it
    // after `apply_reflow`, which also emits `TermRedrawRequestedMsg` (the
    // `VtSize::on_insert` hook fires a reflow), poisoning the count.
    app.add_systems(
        Update,
        count_mutations
            .in_set(TerminalSystems::Process)
            .after(process_input)
            .before(apply_scroll),
    );

    app.add_step(0, |count: Res<MutatedReadCount>, mut commands: Commands| {
        // Wait until the reader has observed at least one mutation, then
        // assert the count is exactly one and exit.
        if count.0 == 0 {
            return;
        }
        r!(commands.assert(
            count.0 == 1,
            "Reader should observe exactly one TermRedrawRequestedMessage",
        ));
        commands.write_message(AppExit::Success);
    });

    assert!(app.run().is_success());
}
