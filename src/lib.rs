//! Token Café: the Claude Code and Codex sessions on this machine as patrons
//! of a JRPG-style night net café. The scene renders a 480x272 pixel-art
//! frame; the binary puts it in a window.

#[rustfmt::skip]
pub mod assets;
pub mod approval;
pub mod collect;
pub mod gfx;
pub mod review;
pub mod scene;
pub mod usage;

#[cfg(feature = "bluetooth")]
pub mod companion;
