//! The `edit` binary — entry point.
//!
//! Phase 0 stub. Real startup (terminal init, the `Application` event loop, and
//! the panic-safe terminal-restore guard) arrives in Phase 2; see
//! docs/roadmap.md.

fn main() {
    // Touch rvision so the workspace wiring is exercised, not merely declared.
    let origin = rvision::geometry::Point::default();
    println!("edit — scaffolding in place at {origin:?}. See docs/roadmap.md.");
}
