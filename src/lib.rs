//! Pure logic of Sail: layout math, navigation state and entry ids.
//!
//! Nothing here touches the firmware, so it is unit-tested on the host with
//! `cargo test --lib --target <host triple>`.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod id;
pub mod layout;
pub mod nav;
