pub mod config;
pub mod error;
pub mod manifest;
pub mod paths;
pub mod platform;
pub mod util;

pub use error::OctxError;

/// Example function — your library logic goes here.
pub fn run(_args: &CliArgs) -> Result<String, OctxError> {
    Ok("Hello from octx library!".to_string())
}

/// Shared CLI argument struct, defined here so lib- and test- code can inspect it.
#[derive(clap::Parser, Debug)]
#[command(
    name = "octx",
    about = "Octopus CLI — your tooling, one head, many arms"
)]
pub struct CliArgs {
    /// Example positional argument.
    pub name: Option<String>,
}
