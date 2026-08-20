use anyhow::Result;
use clap::Parser;
use octx::{CliArgs, run};

fn main() -> Result<()> {
    let args = CliArgs::parse();
    let output = run(&args)?;
    println!("{output}");
    Ok(())
}
