//! # edit
//!
//! The `edit` text editor: an MS-DOS EDIT-style application built on the
//! [`rvision`] terminal UI framework.
//!
//! The crate is split into a thin binary ([`main`](../main.rs)) and this library
//! so the editor's logic — starting with the [`text`] document model — is
//! documented and unit-tested without dragging in terminal startup. The
//! dependency only points one way: `edit` knows about `rvision`, never the
//! reverse (see CLAUDE.md).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod app;
pub mod dialogs;
pub mod editor;
pub mod file;
mod history;
pub mod search;
pub mod text;
