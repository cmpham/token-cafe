//! Shared pixel renderer for desktop and ESP32-S3. Platform IO stays outside.
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;
#[rustfmt::skip]
pub mod assets;
pub mod approval;
pub mod gfx;
pub mod review;
pub mod scene;
pub mod text;
#[cfg(not(feature = "std"))]
trait FloatMath {
    fn floor(self) -> Self;
    fn sin(self) -> Self;
    fn cos(self) -> Self;
    fn hypot(self, other: Self) -> Self;
}
#[cfg(not(feature = "std"))]
impl FloatMath for f64 {
    fn floor(self) -> Self {
        libm::floor(self)
    }
    fn sin(self) -> Self {
        libm::sin(self)
    }
    fn cos(self) -> Self {
        libm::cos(self)
    }
    fn hypot(self, other: Self) -> Self {
        libm::hypot(self, other)
    }
}

pub const ASSET_DIGEST: [u8; 32] = include!(concat!(env!("OUT_DIR"), "/asset_digest.rs"));
