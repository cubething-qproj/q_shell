//! Translation between routed process I/O and virtual-terminal messages.

use std::any::TypeId;

use crate::prelude::*;

pub(crate) fn write_terminal_output(
    mut process_writes: MessageReader<EndpointWriteMsg<Vec<u8>>>,
    terminal_endpoints: Query<(), With<TerminalIoEndpoint>>,
    terminal_writes: Option<MessageWriter<VtWriteMsg>>,
) {
    let Some(mut terminal_writes) = terminal_writes else {
        return;
    };
    for write in process_writes
        .read()
        .filter(|write| write.endpoint().component_type_id() == TypeId::of::<TerminalIoEndpoint>())
    {
        let terminal = write.endpoint().entity();
        if !terminal_endpoints.contains(terminal) {
            warn!("Discarding write to closed terminal endpoint {terminal:?}");
            continue;
        }
        terminal_writes.write(VtWriteMsg::from_peer(
            terminal,
            write.process(),
            write.payload().clone(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use bevy::platform::collections::HashMap;

    use super::*;

    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    struct TestProgram;

    q_proc::impl_program_label!(TestProgram, "test");

    #[derive(Resource, Default)]
    struct ObservedWrite(Option<VtWriteMsg>);

    fn observe_terminal_write(
        mut writes: MessageReader<VtWriteMsg>,
        mut observed: ResMut<ObservedWrite>,
    ) {
        observed.0 = writes.read().next().cloned();
    }

    #[test]
    fn process_output_reaches_terminal_processing_in_the_same_update() {
        let mut app = App::new();
        app.add_plugins((ProcessPlugin, ShellPlugin::<TerminalIoEndpoint>::default()));
        app.add_message::<VtWriteMsg>();
        app.init_resource::<ObservedWrite>();
        app.add_systems(
            Update,
            observe_terminal_write.in_set(TerminalSystems::Process),
        );

        let terminal = app.world_mut().spawn(TerminalIoEndpoint).id();
        let terminal_handle = {
            let world = app.world_mut();
            let mut endpoints = world.query_filtered::<(), With<TerminalIoEndpoint>>();
            let endpoints = endpoints.query(world);
            world
                .resource::<IoComponentCache>()
                .handle::<TerminalIoEndpoint>(terminal, &endpoints)
                .expect("the terminal endpoint should produce a handle")
        };
        let mut descriptors = ProcessFdTable::default();
        descriptors.set(FileDescriptor::STDOUT, terminal_handle);
        let process = app
            .world_mut()
            .spawn((
                Process {
                    prog: TestProgram.intern(),
                    signal_overrides: HashMap::new(),
                    argv: Vec::new(),
                    environ: HashMap::new(),
                },
                descriptors,
            ))
            .id();

        app.world_mut()
            .write_message(ProcessWriteMsg::<Vec<u8>>::stdout(
                process,
                b"hello".to_vec(),
            ));
        app.update();

        let observed = app
            .world()
            .resource::<ObservedWrite>()
            .0
            .as_ref()
            .expect("terminal processing should observe the write in the same update");
        assert_eq!(observed.term, terminal);
        assert_eq!(observed.from, Some(process));
        assert_eq!(observed.bytes, b"hello");
    }
}
