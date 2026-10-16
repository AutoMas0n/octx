//! Integration test: an arm's skill follows the arm's install/uninstall lifecycle.
//!
//! Drives the real `octx` binary against a loopback registry, so the install
//! path runs end to end without touching the network or the user's config.

use std::io::{ErrorKind, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A minimal, conforming skill for the fixture arm.
const SKILL: &str = "\
---
name: fixture
description: A fixture arm used to verify the skill lifecycle.
---

# fixture

## Description
A fixture arm used to verify the skill lifecycle.

## Usage
```
octx x fixture
```

## Options
- `--flag` — does a thing

## Examples
- `octx x fixture --flag`

## Credentials
None.

## Prerequisites
None.

## Failure modes
- Nothing to do — exits non-zero.
";

/// Bind a loopback listener and return it with its base URL.
fn bind_server() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback port");
    listener.set_nonblocking(true).expect("set nonblocking");
    let addr = listener.local_addr().expect("read local addr");
    (listener, format!("http://{addr}"))
}

/// Serve canned responses for each requested path until the returned flag is set.
fn spawn_server(listener: TcpListener, routes: Vec<(String, Vec<u8>)>) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);
    let routes = Arc::new(routes);

    std::thread::spawn(move || {
        while !stop_thread.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buf = [0u8; 4096];
                    let read = stream.read(&mut buf).unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..read]).into_owned();
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/")
                        .to_string();
                    let body = routes
                        .iter()
                        .find(|(route, _)| *route == path)
                        .map(|(_, body)| body.clone());

                    match body {
                        Some(body) => {
                            let head = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            );
                            let _ = stream.write_all(head.as_bytes());
                            let _ = stream.write_all(&body);
                        }
                        None => {
                            let _ = stream.write_all(
                                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            );
                        }
                    }
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });

    stop
}

/// A per-test scratch directory.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("octx-lifecycle-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Run the real `octx` binary against isolated config, data, and home
/// directories. `HOME` matters: agent detection reads it, so without the
/// override a test would publish into the developer's real agent directories.
fn octx(args: &[&str], config_home: &Path, data_home: &Path, home: &Path) -> std::process::Output {
    ProcCommand::new(env!("CARGO_BIN_EXE_octx"))
        .args(args)
        .env("XDG_CONFIG_HOME", config_home)
        .env("XDG_DATA_HOME", data_home)
        .env("HOME", home)
        .output()
        .expect("octx should run")
}

#[test]
fn skill_follows_install_and_uninstall() {
    let base = scratch("install-uninstall");
    let config_home = base.join("config");
    let data_home = base.join("data");
    let agent_dir = base.join("agent");
    // A home that has pi installed, and deliberately does not have claude.
    let home = base.join("home");
    std::fs::create_dir_all(home.join(".pi")).expect("create pi home");
    std::fs::create_dir_all(config_home.join("octx")).expect("create config home");
    std::fs::create_dir_all(&data_home).expect("create data home");
    std::fs::create_dir_all(&agent_dir).expect("create agent dir");

    // Serve a fixture arm's binary and skill over loopback.
    let binary = b"#!/bin/sh\necho fixture\n".to_vec();
    let checksum = octx::util::sha256_digest(&binary);
    let target = octx::platform::detect();

    let (listener, base_url) = bind_server();
    let mut downloads = serde_json::Map::new();
    downloads.insert(
        target.to_string(),
        serde_json::json!({
            "url": format!("{base_url}/fixture.bin"),
            "sha256": checksum,
        }),
    );
    let index = serde_json::json!({
        "registry_version": 1,
        "updated": "2026-01-01T00:00:00Z",
        "head": null,
        "storage": null,
        "arms": {
            "fixture": {
                "description": "A fixture arm used to verify the skill lifecycle.",
                "repository": "github.com/example/fixture",
                "skill_url": format!("{base_url}/fixture.skill.md"),
                "versions": {
                    "1.0.0": { "downloads": serde_json::Value::Object(downloads) }
                }
            }
        }
    })
    .to_string();

    let routes = vec![
        ("/registry-index.json".to_string(), index.into_bytes()),
        ("/fixture.bin".to_string(), binary),
        ("/fixture.skill.md".to_string(), SKILL.as_bytes().to_vec()),
    ];
    let stop = spawn_server(listener, routes);

    // Point octx at the loopback registry.
    std::fs::write(
        config_home.join("octx").join("config.toml"),
        format!("registry_url = \"{base_url}/registry-index.json\"\n"),
    )
    .expect("write config");

    // 1. Link the agent directory before the arm exists.
    let linked = octx(
        &["link", "add", agent_dir.to_str().unwrap()],
        &config_home,
        &data_home,
        &home,
    );
    assert!(
        linked.status.success(),
        "link add failed: {}",
        String::from_utf8_lossy(&linked.stderr)
    );

    // 2. Install the arm from the loopback registry.
    let installed = octx(&["install", "fixture"], &config_home, &data_home, &home);
    assert!(
        installed.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&installed.stderr)
    );

    // The skill is projected into the linked directory.
    let projection = agent_dir.join("fixture-octx").join("SKILL.md");
    assert!(
        projection.exists(),
        "install should project fixture-octx/SKILL.md into the linked directory"
    );
    assert_eq!(
        std::fs::read_to_string(&projection).expect("read projection"),
        SKILL,
        "the projected skill should match the published skill"
    );

    // pi is detected from the home directory with no `link add`, so the same
    // skill also lands under pi's own skills directory.
    let detected = home.join(".pi/agent/skills/fixture-octx/SKILL.md");
    assert!(
        detected.exists(),
        "install should project into a detected agent without a link"
    );
    assert!(
        !home.join(".claude").exists(),
        "octx must not create the directory of an agent that is not present"
    );

    // 3. Uninstall the arm.
    let removed = octx(&["uninstall", "fixture"], &config_home, &data_home, &home);
    assert!(
        removed.status.success(),
        "uninstall failed: {}",
        String::from_utf8_lossy(&removed.stderr)
    );

    // The projection is gone with the arm.
    assert!(
        !agent_dir.join("fixture-octx").exists(),
        "uninstall should remove the projection"
    );
    assert!(
        !detected.exists(),
        "uninstall should remove the projection from a detected agent"
    );

    stop.store(true, Ordering::SeqCst);
    let _ = std::fs::remove_dir_all(&base);
}
