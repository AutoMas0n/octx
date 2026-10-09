use std::process::ExitCode;

use clap::Parser;
use octx_harness::cli::Cli;
use octx_harness::schema::Harness;
use octx_harness::{dispatch, help, resolve};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let config_root = resolve::config_root();
    let data_root = resolve::data_root();

    if let Some(dir) = cli.local_dir.as_deref()
        && let Err(error) = resolve::validate_local_dir(dir)
    {
        eprintln!("error: {error}");
        return ExitCode::FAILURE;
    }

    let Some(name) = cli.name.clone() else {
        let discovered = resolve::discover(
            cli.local_dir.as_deref(),
            config_root.as_deref(),
            data_root.as_deref(),
        );
        print!("{}", help::render(&discovered));
        return ExitCode::SUCCESS;
    };

    let path = match resolve::resolve(
        &name,
        cli.local_dir.as_deref(),
        config_root.as_deref(),
        data_root.as_deref(),
    ) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let harness = match Harness::load(&path) {
        Ok(harness) => harness,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let Some(harness_dir) = path.parent() else {
        eprintln!("error: harness path has no parent directory");
        return ExitCode::FAILURE;
    };

    match dispatch::dispatch(&harness, harness_dir, &cli) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
