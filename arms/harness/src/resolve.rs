//! Harness name resolution across the three layers.

use std::path::{Path, PathBuf};

use crate::schema::{Harness, is_harness_dir};

/// The octx config root (`{config_dir}`); harnesses live under `harnesses/<name>/`.
///
/// Namespaced under `octx/` to match the rest of octx (`config.toml`, `creds.enc`,
/// `skills/`). The mirror layer already spells out `octx/`; this layer must too,
/// otherwise a harness in `{config_dir}/octx/harnesses/` is silently invisible.
pub fn config_root() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("octx"))
}

/// The OS data root (`{data_dir}`); the mirror lives under `octx/storage/harnesses`.
pub fn data_root() -> Option<PathBuf> {
    dirs::data_dir()
}

/// Validate an explicit `--local-dir` before discovery or resolution, so a
/// mistyped path fails loudly instead of silently resolving to nothing.
pub fn validate_local_dir(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!(
            "--local-dir `{}` is not a directory",
            dir.display()
        ));
    }
    if !is_harness_dir(dir) {
        return Err(format!(
            "--local-dir `{}` does not contain a harness.yaml",
            dir.display()
        ));
    }
    Ok(())
}

/// Resolve a harness name to its `harness.yaml` path.
///
/// Order: `--local-dir`, then user-authored `{config_dir}/harnesses/<name>/`,
/// then the read-only mirror `{data_dir}/octx/storage/harnesses/<name>/`.
pub fn resolve(
    name: &str,
    local_dir: Option<&Path>,
    config_root: Option<&Path>,
    data_root: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(dir) = local_dir {
        let candidate = dir.join("harness.yaml");
        if candidate.is_file() {
            return Ok(candidate);
        }
        return Err(format!(
            "no harness.yaml in --local-dir `{}`",
            dir.display()
        ));
    }

    let mut layers: Vec<PathBuf> = Vec::new();
    if let Some(root) = config_root {
        layers.push(root.join("harnesses").join(name).join("harness.yaml"));
    }
    if let Some(root) = data_root {
        layers.push(
            root.join("octx/storage/harnesses")
                .join(name)
                .join("harness.yaml"),
        );
    }
    for candidate in &layers {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }

    let user_hint = config_root
        .map(|root| root.join("harnesses").join(name).display().to_string())
        .unwrap_or_else(|| format!("{{config_dir}}/harnesses/{name}"));
    Err(format!(
        "harness `{name}` not found — run `octx sync`, pass --local-dir, or place a copy in {user_hint}/"
    ))
}

/// A harness discovered for the listing help.
#[derive(Debug, Clone)]
pub struct Discovered {
    /// Harness name.
    pub name: String,
    /// Description, when present.
    pub description: Option<String>,
    /// Where it was found.
    pub path: PathBuf,
}

fn scan(root: &Path, out: &mut Vec<Discovered>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut found: Vec<Discovered> = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() || !is_harness_dir(&dir) {
            continue;
        }
        let (name, description) = match Harness::load(&dir.join("harness.yaml")) {
            Ok(harness) => (harness.name, harness.description),
            Err(_) => (entry.file_name().to_string_lossy().to_string(), None),
        };
        found.push(Discovered {
            name,
            description,
            path: dir.join("harness.yaml"),
        });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    for item in found {
        if !out.iter().any(|existing| existing.name == item.name) {
            out.push(item);
        }
    }
}

/// Discover harnesses across all layers, de-duplicated by name (user shadows mirror).
#[must_use]
pub fn discover(
    local_dir: Option<&Path>,
    config_root: Option<&Path>,
    data_root: Option<&Path>,
) -> Vec<Discovered> {
    let mut out = Vec::new();
    if let Some(dir) = local_dir
        && is_harness_dir(dir)
    {
        let harness = Harness::load(&dir.join("harness.yaml")).ok();
        out.push(Discovered {
            name: harness.as_ref().map(|h| h.name.clone()).unwrap_or_else(|| {
                dir.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            }),
            description: harness.and_then(|h| h.description),
            path: dir.join("harness.yaml"),
        });
    }
    if let Some(root) = config_root {
        scan(&root.join("harnesses"), &mut out);
    }
    if let Some(root) = data_root {
        scan(&root.join("octx/storage/harnesses"), &mut out);
    }
    out
}
