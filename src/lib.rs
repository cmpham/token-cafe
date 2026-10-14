//! Token Café: the Claude Code and Codex sessions on this machine as patrons
//! of a JRPG-style night net café. The scene renders a 480x272 pixel-art
//! frame; the binary puts it in a window.

pub use cafe_ui::{assets, gfx, review, scene};
pub mod approval;
pub mod collect;
pub mod usage;

#[cfg(feature = "bluetooth")]
pub mod companion;
