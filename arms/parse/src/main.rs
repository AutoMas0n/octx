use clap::Parser;

#[derive(Parser)]
#[command(name = "parse", about = "Not yet implemented")]
struct Args;

fn main() {
    let _args = Args::parse();
    eprintln!("parse arm: not yet implemented");
    std::process::exit(1);
}
