mod shell;

pub mod prelude {
    pub use super::get_test_app;
    pub use bevy::prelude::*;
    pub use q_shell::prelude::*;
    pub use q_test_harness::prelude::*;
}

use prelude::*;

pub fn get_test_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TestRunnerPlugin::default(),
        ProcessPlugin,
        TerminalPlugin::default(),
        ShellPlugin::<TerminalIoEndpoint>::default(),
    ));
    app
}
