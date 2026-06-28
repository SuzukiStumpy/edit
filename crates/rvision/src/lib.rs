//! # rvision
//!
//! A [Turbo Vision](https://en.wikipedia.org/wiki/Turbo_Vision)-style terminal
//! UI framework, hand-built in Rust. It provides a retained-mode tree of view
//! objects, a three-phase event loop, and a double-buffered cell renderer that
//! talks to the terminal through a swappable backend.
//!
//! The design is recorded in `docs/adr/`; the build order in `docs/roadmap.md`.
//!
//! ## Architecture at a glance
//!
//! - **Backend / EventSource** — the only seam to the outside world. A
//!   `CrosstermBackend` drives a real terminal; a `TestBackend` drives unit
//!   tests headlessly (ADR 0002).
//! - **Screen** — a [`cell::Cell`] grid drawn into a back buffer, then diffed
//!   against the front buffer so only changed cells are flushed (ADR 0002).
//! - **View tree** — parent-owns-children trait objects; widgets never hold
//!   references to one another. Commands bubble up, broadcasts travel down
//!   (ADR 0003, 0004).
//! - **Theme** — views request colours by semantic role, resolved against a
//!   swappable theme over a truecolour-ready colour type (ADR 0005).
//!
//! Modules are introduced phase by phase; [`geometry`] is the Phase 1 seed,
//! [`event`] + [`app`] + [`crossterm_backend`] are the Phase 2 event loop, and
//! [`canvas`] + [`view`] + [`command`] are the Phase 3 view system (the retained
//! tree, its draw surface, and the command vocabulary).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod app;
pub mod backend;
pub mod buffer;
pub mod canvas;
pub mod cell;
pub mod color;
pub mod command;
pub mod crossterm_backend;
pub mod event;
pub mod geometry;
pub mod theme;
pub mod view;
