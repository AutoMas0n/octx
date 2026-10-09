use clap::Parser;
use octx_agent::cli::Cli;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match octx_agent::runner::run(cli).await {
        Ok(code) => std::process::ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(error) => {
            eprintln!("error: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
