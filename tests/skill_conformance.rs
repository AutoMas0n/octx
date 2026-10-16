//! Every arm's `skill.md` conforms to `SKILL-CONTRACT.md`.
//!
//! An arm's skill is its discovery surface: octx publishes it beside the binary
//! and projects it into agent directories as `<arm>-octx/SKILL.md`, so a missing
//! section is a wrong tool call rather than a documentation blemish. This test
//! walks `arms/*` and fails when an arm has no skill, when the `name` or
//! `description` frontmatter is missing, when `name` is not `<arm>-octx`, when a
//! required section is missing or out of order, or when a skill instructs a
//! credential through the environment.
//!
//! The contract is documented in `SKILL-CONTRACT.md` at the repository root.

use std::path::{Path, PathBuf};

/// The seven required sections, in the order they must appear.
const REQUIRED: [&str; 7] = [
    "Description",
    "Usage",
    "Options",
    "Examples",
    "Credentials",
    "Prerequisites",
    "Failure modes",
];

/// Top-level (`##`) headings, in file order, with the leading hashes trimmed.
fn section_headings(markdown: &str) -> Vec<&str> {
    markdown
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("## ")?;
            // `### ` also starts with `##`, so reject deeper levels explicitly.
            if rest.starts_with('#') {
                None
            } else {
                Some(rest.trim())
            }
        })
        .collect()
}

/// A skill may only mention the retired environment mechanism to disown it.
fn env_credential_violation(markdown: &str) -> Option<String> {
    for (index, line) in markdown.lines().enumerate() {
        let lower = line.to_lowercase();
        let exports_a_secret =
            lower.contains("export ") && (lower.contains("token") || lower.contains("key"));
        // The delegation note in the agent skill names `OCTX_TOKEN_<HOST>` to
        // explain that credentials are *not* read from the environment.
        let names_a_token_var = line.contains("OCTX_TOKEN_") && !lower.contains("never");

        if exports_a_secret || names_a_token_var {
            return Some(format!("line {}: {}", index + 1, line.trim()));
        }
    }
    None
}

/// The skill must open with a YAML frontmatter block carrying `name` and
/// `description`; without it the published `SKILL.md` is not loadable. `name`
/// must be the arm name suffixed `-octx`, so octx-provided skills are
/// identifiable wherever an agent lists them.
fn frontmatter_violation(arm: &str, markdown: &str) -> Option<String> {
    let mut lines = markdown.lines();

    if lines.next().map(str::trim_end) != Some("---") {
        return Some("missing frontmatter block — expected `---` on the first line".into());
    }

    let expected = format!("{arm}-octx");
    let mut name: Option<String> = None;
    let mut description = false;
    for line in lines {
        if line.trim_end() == "---" {
            match name {
                None => return Some("frontmatter is missing `name`".into()),
                Some(ref found) if found != &expected => {
                    return Some(format!(
                        "frontmatter `name` is `{found}` — expected `{expected}` (the arm name suffixed `-octx`)"
                    ));
                }
                _ => {}
            }
            if !description {
                return Some("frontmatter is missing `description`".into());
            }
            return None;
        }
        if let Some((key, value)) = line.trim().split_once(':') {
            match key.trim() {
                "name" if !value.trim().is_empty() => name = Some(value.trim().to_string()),
                "description" if !value.trim().is_empty() => description = true,
                _ => {}
            }
        }
    }

    Some("frontmatter block is not closed with `---`".into())
}

/// Check one skill's text against the contract.
fn check_skill(arm: &str, markdown: &str) -> Result<(), String> {
    if let Some(violation) = frontmatter_violation(arm, markdown) {
        return Err(violation);
    }

    let headings = section_headings(markdown);

    for required in REQUIRED {
        if !headings.contains(&required) {
            return Err(format!("missing section `## {required}`"));
        }
    }

    if let Some(unexpected) = headings.iter().find(|h| !REQUIRED.contains(h)) {
        return Err(format!(
            "unexpected section `## {unexpected}` — the seven required sections are the only top-level headings"
        ));
    }

    let positions: Vec<usize> = REQUIRED
        .iter()
        .map(|required| {
            headings
                .iter()
                .position(|h| h == required)
                .expect("presence checked above")
        })
        .collect();
    if positions.windows(2).any(|pair| pair[0] > pair[1]) {
        return Err(format!(
            "sections are out of order — expected {}",
            REQUIRED.join(", ")
        ));
    }

    match env_credential_violation(markdown) {
        Some(offence) => Err(format!(
            "instructs a credential through the environment ({offence}); name the host and `octx creds add <host>` instead"
        )),
        None => Ok(()),
    }
}

/// Every problem found under `arms_dir`, one message per arm.
fn check_arms(arms_dir: &Path) -> Vec<String> {
    let entries = match std::fs::read_dir(arms_dir) {
        Ok(entries) => entries,
        Err(e) => return vec![format!("cannot read {}: {e}", arms_dir.display())],
    };

    let mut problems = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let arm = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let skill_path = path.join("skill.md");
        match std::fs::read_to_string(&skill_path) {
            Err(_) => problems.push(format!("arm `{arm}` has no {skill_path:?}")),
            Ok(markdown) => {
                if let Err(problem) = check_skill(&arm, &markdown) {
                    problems.push(format!("arm `{arm}`: {problem}"));
                }
            }
        }
    }
    problems.sort();
    problems
}

fn repo_arms_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("arms")
}

/// A scratch directory so negative tests never touch the real workspace.
fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("octx-skill-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// A minimal conforming skill, used as the base for the negative cases.
const CONFORMING: &str = "\
---
name: demo-octx
description: Does one thing.
---

# demo

## Description
Does one thing.

## Usage
```
octx x demo
```

## Options
- `--flag` — does a thing

## Examples
- `octx x demo --flag`

## Credentials
None.

## Prerequisites
None.

## Failure modes
- Nothing to do — exits non-zero.
";

#[test]
fn every_arm_has_a_conforming_skill() {
    let problems = check_arms(&repo_arms_dir());
    assert!(
        problems.is_empty(),
        "skill conformance failed:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn conforming_skill_passes() {
    assert_eq!(check_skill("demo", CONFORMING), Ok(()));
}

#[test]
fn missing_skill_file_is_reported_and_names_the_arm() {
    let arms = scratch_dir("missing-file");
    std::fs::create_dir_all(arms.join("deploy")).expect("create arm dir");

    let problems = check_arms(&arms);
    assert_eq!(problems.len(), 1, "expected one problem, got {problems:?}");
    assert!(
        problems[0].contains("deploy"),
        "problem should name the arm: {}",
        problems[0]
    );
}

#[test]
fn missing_section_is_reported_by_name() {
    let without_options = CONFORMING.replace("## Options\n- `--flag` — does a thing\n\n", "");
    let error = check_skill("demo", &without_options).expect_err("should reject a missing section");
    assert!(
        error.contains("## Options"),
        "error should name the missing section: {error}"
    );
}

#[test]
fn empty_skill_is_reported_rather_than_accepted() {
    let error = check_skill("demo", "").expect_err("should reject an empty skill");
    assert!(
        error.contains("frontmatter"),
        "error should name the first missing element: {error}"
    );
}

#[test]
fn missing_frontmatter_is_reported() {
    let without = CONFORMING
        .split("---\n")
        .nth(2)
        .expect("CONFORMING should have a frontmatter block")
        .to_string();
    let error =
        check_skill("demo", &without).expect_err("should reject a skill without frontmatter");
    assert!(
        error.contains("frontmatter"),
        "error should explain the violation: {error}"
    );
}

#[test]
fn frontmatter_without_description_is_reported() {
    let without = CONFORMING.replace("description: Does one thing.\n", "");
    let error =
        check_skill("demo", &without).expect_err("should reject frontmatter without description");
    assert!(
        error.contains("description"),
        "error should name the missing field: {error}"
    );
}

#[test]
fn frontmatter_without_name_is_reported() {
    let without = CONFORMING.replace("name: demo-octx\n", "");
    let error = check_skill("demo", &without).expect_err("should reject frontmatter without name");
    assert!(
        error.contains("name"),
        "error should name the missing field: {error}"
    );
}

#[test]
fn frontmatter_name_without_octx_suffix_is_reported() {
    let without = CONFORMING.replace("name: demo-octx\n", "name: demo\n");
    let error = check_skill("demo", &without).expect_err("should reject a name without -octx");
    assert!(
        error.contains("-octx"),
        "error should name the expected suffix: {error}"
    );
}

#[test]
fn sections_out_of_order_are_reported() {
    // Swap Examples and Credentials.
    let swapped = CONFORMING.replace("## Credentials\nNone.\n\n", "").replace(
        "## Prerequisites\nNone.\n",
        "## Prerequisites\nNone.\n\n## Credentials\nNone.\n",
    );
    let error = check_skill("demo", &swapped).expect_err("should reject out-of-order sections");
    assert!(
        error.contains("out of order"),
        "error should say the order is wrong: {error}"
    );
}

#[test]
fn an_extra_section_is_rejected() {
    let with_environment = CONFORMING.replace(
        "## Credentials\nNone.\n",
        "## Environment\n- `OCTX_TOKEN_DEMO` — injected\n\n## Credentials\nNone.\n",
    );
    let error = check_skill("demo", &with_environment).expect_err("should reject an extra section");
    assert!(
        error.contains("## Environment"),
        "error should name the unexpected section: {error}"
    );
}

#[test]
fn an_environment_credential_instruction_is_rejected() {
    let stale = CONFORMING.replace(
        "## Credentials\nNone.",
        "## Credentials\n- `OCTX_TOKEN_GITHUB` — injected when the arm is spawned",
    );
    let error = check_skill("demo", &stale)
        .expect_err("should reject an environment credential instruction");
    assert!(
        error.contains("environment"),
        "error should explain the violation: {error}"
    );
}

#[test]
fn a_delegation_note_that_disowns_the_environment_is_allowed() {
    let disowned = CONFORMING.replace(
        "## Credentials\nNone.",
        "## Credentials\nCredentials are never read from the environment; the arm fetches the `github` host (delegated as `OCTX_TOKEN_GITHUB`). Add it with `octx creds add github`.",
    );
    assert_eq!(check_skill("demo", &disowned), Ok(()));
}

#[test]
fn subsections_inside_a_required_section_are_allowed() {
    let with_subsection = CONFORMING.replace(
        "## Options\n- `--flag` — does a thing",
        "## Options\n### Flags\n- `--flag` — does a thing",
    );
    assert_eq!(check_skill("demo", &with_subsection), Ok(()));
}
