//! A special [`Process`] used to control other processes.

use std::marker::PhantomData;

use bevy::{
    ecs::{lifecycle::HookContext, world::DeferredWorld},
    platform::collections::HashMap,
    prelude::*,
};
use q_proc::prelude::*;

use crate::plugins::ShellIo;

// Kernel equivalent: pty follower.
// Obviated by single-use.
/// Marker struct for shell entities. The systems associated with this
/// struct must be implemented outside this crate.
#[derive(Component, Reflect, Debug)]
#[component(immutable, on_add = Shell::<T>::on_add)]
#[relationship(relationship_target = ShellTarget<T>)]
#[require(ForegroundProcessGroup)]
pub struct Shell<T: ShellIo = TerminalIoEndpoint> {
    #[relationship]
    pub term: Entity,
    marker: PhantomData<fn() -> T>,
}

impl<T: ShellIo> Shell<T> {
    /// Creates a shell attached to `term` and backed by `T`.
    pub fn new(term: Entity) -> Self {
        Self {
            term,
            marker: PhantomData,
        }
    }

    fn on_add(mut world: DeferredWorld, context: HookContext) {
        let term = world
            .get::<Self>(context.entity)
            .expect("the shell was just inserted")
            .term;
        world.commands().entity(term).entry::<T>().or_default();
    }
}

#[derive(Message, Debug)]
pub struct ShellSpawnMsg {
    pub prog: InternedProgramLabel,
    pub shell: Entity,
    pub argv: Vec<String>,
    pub environ: HashMap<String, String>,
}
impl ShellSpawnMsg {
    pub fn new(prog: impl ProgramLabel, shell: Entity) -> Self {
        Self {
            prog: prog.intern(),
            shell,
            argv: Vec::new(),
            environ: HashMap::new(),
        }
    }
    pub fn with_args(prog: impl ProgramLabel, shell: Entity, argv: Vec<String>) -> Self {
        Self {
            prog: prog.intern(),
            shell,
            argv,
            environ: HashMap::new(),
        }
    }
}

/// Marks a terminal entity as a byte-oriented process I/O endpoint.
#[derive(Component, Reflect, Debug, Default)]
pub struct TerminalIoEndpoint;

impl IoComponent for TerminalIoEndpoint {
    type Stdin = Vec<u8>;
    type Stdout = Vec<u8>;
}

/// Attached to the terminal when spawning a [`Shell`].
#[derive(Component, Reflect, Debug)]
#[relationship_target(relationship = Shell<T>)]
pub struct ShellTarget<T: ShellIo = TerminalIoEndpoint> {
    #[relationship]
    shell: Entity,
    marker: PhantomData<fn() -> T>,
}
impl<T: ShellIo> ShellTarget<T> {
    pub fn target(&self) -> Entity {
        self.shell
    }
}

/// Marker for a process owned by a [`Shell`].
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = ShellJobTarget)]
pub struct ShellJob(pub Entity);

/// The [`Shell`] which owns this [`Job`].
#[derive(Component, Reflect, Debug)]
#[relationship_target(relationship = ShellJob)]
pub struct ShellJobTarget(Entity);

/// This group gets its stdio piped directly to the [`Terminal`].
/// **Important:** this should _only_ be set by the shell.
/// If this is empty, then the owning [`Shell`] owns the pty.
#[derive(Component, Reflect, Debug, Default)]
#[relationship_target(relationship = ForegroundProcess)]
pub struct ForegroundProcessGroup {
    #[relationship_target]
    processes: Vec<Entity>,
}
impl ForegroundProcessGroup {
    pub fn processes(&self) -> &[Entity] {
        &self.processes
    }
}

/// Selects the one process that receives terminal input for a [`Shell`].
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = ForegroundInputProcessTarget)]
pub struct ForegroundInputProcess(Entity);
impl ForegroundInputProcess {
    pub fn new(shell: Entity) -> Self {
        Self(shell)
    }
    pub fn shell(&self) -> Entity {
        self.0
    }
}

/// The process selected to receive terminal input for a [`Shell`].
#[derive(Component, Reflect, Debug)]
#[relationship_target(relationship = ForegroundInputProcess, linked_spawn)]
pub struct ForegroundInputProcessTarget(Entity);
impl ForegroundInputProcessTarget {
    pub fn process(&self) -> Entity {
        self.0
    }
}

/// Attached to a [`Process`] in the [`ForegroundProcessGroup`].
/// The inner value is a pointer to a [`Shell`] with an attached
/// [`ForegroundProcessGroup`]
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = ForegroundProcessGroup)]
pub struct ForegroundProcess(pub(crate) Entity);
impl ForegroundProcess {
    pub fn new(shell: Entity) -> Self {
        Self(shell)
    }
    pub fn shell(&self) -> Entity {
        self.0
    }
}
