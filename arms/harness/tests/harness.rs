//! Tests for harness resolution, YAML parsing, defaults-merge, and listing.

use std::fs;
use std::path::Path;

use clap::Parser;
use octx_harness::cli::Cli;
use octx_harness::schema::Harness;
use octx_harness::{dispatch, help, resolve};
use tempfile::TempDir;

fn cli(args: &[&str]) -> Cli {
    Cli::parse_from(std::iter::once("harness").chain(args.iter().copied()))
}

fn write(dir: &Path, relative: &str, contents: &str) {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

const HARNESS: &str = r#"
schema: 1
name: develop-arm
description: Develop an arm
agent: pi
defaults:
  model: claude-sonnet-4
  permission_mode: approve-reads
  system_prompt: You are a careful reviewer.
  timeout: 300
  max_turns: 8
  format: ndjson
  cwd: ./
script:
  path: script.py
  args:
    - --default
agent_config:
  pi:
    thinking: high
"#;

#[test]
fn parses_a_valid_harness() {
    let harness = Harness::parse(HARNESS).expect("valid harness");
    assert_eq!(harness.name, "develop-arm");
    assert_eq!(harness.agent, "pi");
    assert_eq!(harness.defaults.model.as_deref(), Some("claude-sonnet-4"));
    assert_eq!(harness.defaults.timeout, Some(300));
    assert_eq!(harness.script.args, vec!["--default"]);
    assert_eq!(harness.agent_config.pi.thinking.as_deref(), Some("high"));
}

#[test]
fn reports_missing_required_field() {
    let error = Harness::parse("name: broken\nscript:\n  path: s.py\n").unwrap_err();
    assert!(error.contains("agent"), "error: {error}");
}

#[test]
fn resolution_layers_and_shadowing() {
    let temp = TempDir::new().unwrap();
    let config = temp.path().join("config");
    let data = temp.path().join("data");
    write(
        &config,
        "harnesses/develop-arm/harness.yaml",
        "name: develop-arm\nagent: pi\nscript:\n  path: s.py\n",
    );
    write(
        &data,
        "octx/storage/harnesses/develop-arm/harness.yaml",
        "name: develop-arm\nagent: pi\nscript:\n  path: s.py\n",
    );

    let from_user = resolve::resolve(
        "develop-arm",
        None,
        Some(config.as_path()),
        Some(data.as_path()),
    )
    .unwrap();
    assert!(
        from_user.starts_with(&config),
        "user layer should win: {from_user:?}"
    );

    let from_mirror = resolve::resolve(
        "develop-arm",
        None,
        Some(temp.path().join("empty-config").as_path()),
        Some(data.as_path()),
    )
    .unwrap();
    assert!(
        from_mirror.starts_with(&data),
        "mirror layer: {from_mirror:?}"
    );

    let error =
        resolve::resolve("nope", None, Some(config.as_path()), Some(data.as_path())).unwrap_err();
    assert!(error.contains("octx sync"), "error: {error}");
    assert!(error.contains("--local-dir"), "error: {error}");
}

#[test]
fn local_dir_overrides_resolution() {
    let temp = TempDir::new().unwrap();
    let local = temp.path().join("my-harness");
    write(
        &local,
        "harness.yaml",
        "name: my-harness\nagent: pi\nscript:\n  path: s.py\n",
    );
    let resolved = resolve::resolve("anything", Some(local.as_path()), None, None).unwrap();
    assert_eq!(resolved, local.join("harness.yaml"));
}

#[test]
fn cli_overrides_win_and_script_path_is_absolute() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    let harness = Harness::parse(HARNESS).unwrap();
    let args =
        dispatch::build_agent_args(&harness, dir, &cli(&["--model", "gpt-4o", "--", "extra"]));

    let value_after = |flag: &str| {
        let index = args.iter().position(|a| a == flag).unwrap();
        args[index + 1].clone()
    };
    assert_eq!(value_after("--model"), "gpt-4o", "CLI model should win");
    assert_eq!(value_after("--thinking"), "high");
    assert_eq!(
        value_after("--system-prompt"),
        "You are a careful reviewer."
    );
    assert_eq!(value_after("--format"), "ndjson");
    assert_eq!(value_after("--cwd"), dir.display().to_string());
    assert_eq!(
        value_after("--script"),
        dir.join("script.py").display().to_string()
    );
    let separator = args.iter().position(|a| a == "--").unwrap();
    assert_eq!(&args[separator + 1..], &["--default", "extra"]);
}

#[test]
fn discover_lists_only_directories_with_a_harness() {
    let temp = TempDir::new().unwrap();
    let config = temp.path().join("config");
    let data = temp.path().join("data");
    write(
        &config,
        "harnesses/alpha/harness.yaml",
        "name: alpha\ndescription: first\nagent: pi\nscript:\n  path: s.py\n",
    );
    write(&config, "harnesses/not-a-harness/readme.md", "no yaml here");
    write(
        &data,
        "octx/storage/harnesses/beta/harness.yaml",
        "name: beta\ndescription: second\nagent: pi\nscript:\n  path: s.py\n",
    );
    write(
        &data,
        "octx/storage/harnesses/alpha/harness.yaml",
        "name: alpha\ndescription: shadowed\nagent: pi\nscript:\n  path: s.py\n",
    );

    let discovered = resolve::discover(None, Some(config.as_path()), Some(data.as_path()));
    let names: Vec<&str> = discovered.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "beta"]);
    assert_eq!(discovered[0].description.as_deref(), Some("first"));

    let rendered = help::render(&discovered);
    assert!(rendered.contains("alpha"));
    assert!(rendered.contains("beta"));
    assert!(!rendered.contains("not-a-harness"));
}

#[test]
fn config_layer_is_namespaced_under_octx() {
    let root = resolve::config_root().expect("config_root");
    assert!(
        root.ends_with("octx"),
        "config_root should be `<config_dir>/octx`, got {}",
        root.display()
    );
    assert!(
        root.join("harnesses").ends_with("octx/harnesses"),
        "user harnesses should live in `<config_dir>/octx/harnesses`"
    );
}

#[test]
fn local_dir_is_validated_loudly() {
    let temp = TempDir::new().unwrap();

    let missing = temp.path().join("does-not-exist");
    let error = resolve::validate_local_dir(&missing).unwrap_err();
    assert!(error.contains("not a directory"), "error: {error}");

    let empty = temp.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    let error = resolve::validate_local_dir(&empty).unwrap_err();
    assert!(error.contains("harness.yaml"), "error: {error}");

    let good = temp.path().join("good");
    write(
        &good,
        "harness.yaml",
        "name: good\nagent: pi\nscript:\n  path: s.py\n",
    );
    assert!(resolve::validate_local_dir(&good).is_ok());
}
