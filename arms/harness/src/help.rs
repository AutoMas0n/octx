//! Help-text rendering for the harness arm.

use crate::resolve::Discovered;

/// Render the list of discovered harnesses.
#[must_use]
pub fn render(discovered: &[Discovered]) -> String {
    if discovered.is_empty() {
        return "No harnesses found — run `octx sync` or pass --local-dir.\n".to_string();
    }
    let mut out = String::from("Available harnesses:\n");
    for harness in discovered {
        let description = harness.description.as_deref().unwrap_or("(no description)");
        out.push_str(&format!(
            "  {:<20} {}\n      {}\n",
            harness.name,
            description,
            harness.path.display()
        ));
    }
    out
}
