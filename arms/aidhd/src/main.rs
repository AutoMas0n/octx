use clap::Parser;
use octx_aidhd::cli::Cli;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    octx_aidhd::app::run(cli)
}
