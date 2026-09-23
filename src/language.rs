//! Parse shell source without performing I/O or starting a process.

use std::fmt;

use bevy::prelude::Resource;

/// Parsed shell source consumed directly by the interpreter.
///
/// This is the initial command AST, not an intermediate compilation stage.
/// Its variants can grow as the bundled language learns more syntax.
#[derive(Debug, PartialEq, Eq)]
pub enum ShellIr {
    Empty,
    Command {
        program: String,
        arguments: Vec<String>,
    },
}

/// A parse error that can be displayed through the shell's stderr descriptor.
#[derive(Debug)]
pub struct ShellParseError(String);

impl ShellParseError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ShellParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// A swappable, side-effect-free parser for a shell's input language.
pub trait ShellLanguage: fmt::Debug + Send + Sync + 'static {
    fn parse(&self, source: &str) -> Result<ShellIr, ShellParseError>;
}

#[derive(Resource)]
pub(crate) struct ShellLanguageConfig<L: ShellLanguage>(pub L);

/// The bundled single-command language; richer grammars may replace it.
#[derive(Debug, Default)]
pub struct SimpleShellLanguage;

impl ShellLanguage for SimpleShellLanguage {
    fn parse(&self, source: &str) -> Result<ShellIr, ShellParseError> {
        let mut words = shlex::split(source)
            .ok_or_else(|| ShellParseError::new("unmatched quote"))?
            .into_iter();
        Ok(match words.next() {
            Some(program) => ShellIr::Command {
                program,
                arguments: words.collect(),
            },
            None => ShellIr::Empty,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_quoted_and_escaped_arguments() {
        assert_eq!(
            SimpleShellLanguage
                .parse("echo \"hello world\" a\\ b")
                .unwrap(),
            ShellIr::Command {
                program: "echo".into(),
                arguments: vec!["hello world".into(), "a b".into()],
            }
        );
    }

    #[test]
    fn empty_line_and_invalid_quote_are_distinct() {
        assert_eq!(SimpleShellLanguage.parse(" \t\n").unwrap(), ShellIr::Empty);
        assert!(SimpleShellLanguage.parse("echo '").is_err());
    }
}
