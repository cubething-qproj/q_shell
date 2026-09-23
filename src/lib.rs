//! Process-backed shells for Bevy.
//!
//! `ShellPlugin::default()` installs the q_proc process layer, q_term terminal,
//! canonical keyboard adapter, and a small interactive language.
//!
//! Registering a q_proc program with `app.program::<T>().add_system(...)`
//! makes its validated `ProgramName` available as a shell command, even after
//! the shell plugin is installed. A `Process` receives argument tokens in
//! `argv` **without** the program name; programs choose their own argument
//! parser. Parse errors and unknown commands use the shell's stderr descriptor.
//!
//! The bundled `SimpleShellLanguage` parses one command per line with quoting
//! and escapes. `|`, `>`, and `;` are ordinary arguments, not partially
//! implemented shell operators. Another language can be supplied through
//! `ShellPlugin::default().with_language(MyLanguage)`; it parses to the same
//! `ShellIr` command AST for the standard interpreter. Use the lower-level
//! `ShellBackendPlugin<T>` for a custom endpoint or interpreter process.

mod data;
mod language;
mod plugins;
pub mod systems;

pub mod prelude {
    pub use super::data::prelude::*;
    pub use super::language::{ShellIr, ShellLanguage, ShellParseError, SimpleShellLanguage};
    pub use super::plugins::*;
    pub use bevy::prelude::*;
    pub use q_proc::prelude::*;
    pub use q_term::prelude::*;
    pub use tiny_bail::prelude::*;
}
