use clap::Parser;

use octx_jam::cli::Cli;
use octx_jam::config::Config;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match octx_jam::run::run(cli, Config::from_env()).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
