//! The floating island: an always-on-top pill that shows dictation state in the
//! user's line of sight, next to whatever app they're dictating into.
//!
//! `panel` owns the window and AppKit behaviour (macOS-only, like `injection`).
//! `position` is the pure geometry it uses to place that window, split out so the
//! offscreen-recovery logic can be tested without a screen.

#[cfg(target_os = "macos")]
pub mod panel;
pub mod position;
