use anyhow::Result;
use clap::Parser;
use stx::{run, CliArgs};

fn main() -> Result<()> {
    let args = CliArgs::parse();
    let output = run(&args)?;
    println!("{output}");
    Ok(())
}