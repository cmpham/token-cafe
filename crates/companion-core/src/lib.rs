//! Transport-independent v2 protocol and authoritative/replicated state.
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;
pub mod wire {
    include!(concat!(env!("OUT_DIR"), "/token_cafe.v2.rs"));
}

pub mod authority;
pub mod codec;
pub mod crypto;
pub mod model;

pub mod transport;

pub mod client;

pub mod sync;
