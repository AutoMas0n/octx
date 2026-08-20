use thiserror::Error;

/// Errors that can occur in the stx library.
#[derive(Error, Debug)]
pub enum StxError {
    /// A generic error placeholder.
    #[error("something went wrong: {0}")]
    Generic(String),
}

/// Example function — your library logic goes here.
pub fn run(_args: &CliArgs) -> Result<String, StxError> {
    Ok("Hello from stx library!".to_string())
}

/// Shared CLI argument struct, defined here so lib- and test- code can inspect it.
#[derive(clap::Parser, Debug)]
#[command(name = "stx", about = "A CLI tool built with clap + thiserror + anyhow")]
pub struct CliArgs {
    /// Example positional argument.
    pub name: Option<String>,
}