//! Pinned Mole's complete cleanup as a separately confirmed operation scope.
//! It cannot accept item IDs/paths or consume an existing subset confirmation.
pub(crate) mod bridge;
mod commands;
pub(crate) mod journal;
mod plan;
mod runner;
mod terminal;
pub(crate) use commands::*;
use std::{
    fs::File,
    sync::{atomic::AtomicBool, Mutex},
};
#[derive(Default)]
pub(crate) struct Controller {
    plan: Mutex<Option<plan::Authorized>>,
    input: Mutex<Option<(String, File)>>,
    authorization: AtomicBool,
}

#[cfg(test)]
mod native_tests;
