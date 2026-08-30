/// Octopus CLI — your tooling, one head, many arms.
#[tokio::main]
async fn main() {
    if let Err(e) = octx::cli::run().await {
        eprintln!("error: {}", e);
        std::process::exit(e.exit_code());
    }
}
