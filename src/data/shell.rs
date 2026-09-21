//! A special [`Process`] used to control other processes.

use bevy::prelude::*;
use bevy::platform::collections::HashMap;
use q_proc::prelude::*;

// Kernel equivalent: pty follower.
// Obviated by single-use.
/// Marker struct for shell entities. The systems associated with this
/// struct must be implemented outside this crate.
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = ShellTarget)]
#[require(ForegroundProcessGroup)]
pub struct Shell {
    #[relationship]
    pub term: Entity,
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
#[relationship_target(relationship = Shell)]
#[require(TerminalIoEndpoint)]
pub struct ShellTarget(Entity);
impl ShellTarget {
    pub fn target(&self) -> Entity {
        self.0
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
