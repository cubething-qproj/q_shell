//! A shell that orchestrates [`q_proc::Process`] spawning.

mod data;
mod plugins;
pub mod systems;

pub mod prelude {
    pub use super::data::prelude::*;
    pub use super::plugins::*;
    pub use bevy::prelude::*;
    pub use q_proc::prelude::*;
    pub use q_term::prelude::*;
    pub use tiny_bail::prelude::*;
}
