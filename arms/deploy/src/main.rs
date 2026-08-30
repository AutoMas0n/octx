use clap::Parser;

#[derive(Parser)]
#[command(name = "deploy", about = "Not yet implemented")]
struct Args;

fn main() {
    let _args = Args::parse();
    eprintln!("deploy arm: not yet implemented");
    std::process::exit(1);
}
