//! CheCheViewer — a keyboard-focused image viewer.
//!
//! The logic lives here rather than in the binary so it can be tested
//! without opening a window.

#[macro_use]
pub mod diag;

pub mod app;
pub mod cache;
pub mod decode;
pub mod input;
pub mod loader;
pub mod model;
pub mod platform;
pub mod scan;
pub mod session;
pub mod ui;
pub mod view;
