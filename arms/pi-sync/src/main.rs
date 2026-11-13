//! pi-sync arm binary entrypoint.

fn main() {
    if let Err(error) = octx_pi_sync::run() {
        eprintln!("pi-sync: {error:#}");
        std::process::exit(1);
    }
}
