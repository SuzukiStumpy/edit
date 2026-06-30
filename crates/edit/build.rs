//! Build script: stamps the short git commit hash into the binary via the
//! `EDIT_GIT_SHA` compile-time env var, so the About box can read
//! `edit X.Y.Z (sha)` (ADR 0024).
//!
//! Best-effort: if git or the repository metadata is unavailable — e.g. building
//! from a packaged source archive with no `.git`, or on a machine without git —
//! the stamp is the empty string and the About box simply omits the `(sha)`
//! suffix. Nothing here can fail the build.
//!
//! We emit no `rerun-if-changed`, so Cargo's default applies: this script re-runs
//! whenever a file in the crate changes. That keeps the stamp fresh during normal
//! development and, crucially, makes every clean/release build (a fresh CI
//! checkout) accurate. A bare `git commit` touching no source may leave a locally
//! cached stamp one commit stale until the next rebuild — immaterial for an About
//! box, and not worth the cross-platform fragility of watching `.git` by hand.

use std::process::Command;

fn main() {
    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();

    println!("cargo:rustc-env=EDIT_GIT_SHA={sha}");
}
