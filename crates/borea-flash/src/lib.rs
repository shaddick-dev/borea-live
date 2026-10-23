//! `borea-flash`: write the Borea Live ISO to a USB stick, safely.
//!
//! - Only removable and USB disks are offered. The disk running this system, the
//!   disk holding the image, and anything too small are refused with a reason.
//! - The image must be a bootable hybrid ISO, and is checked against its published
//!   SHA-256 when one sits beside it.
//! - Writing needs the device's name typed back, re-checks the target just before
//!   opening it, opens it exclusively, and reads every byte back to verify.

pub mod app;
pub mod device;
pub mod image;
pub mod install;
pub mod make;
pub mod target;
pub mod terminal;
pub mod write;

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum FlashError {
    #[error("{path}: {why}", path = .0.display(), why = .1)]
    Image(PathBuf, String),
    #[error("{0}")]
    Access(String),
    #[error("{0} is no longer ready to write: {1}")]
    Changed(String, String),
    #[error("reading the image failed: {0}")]
    Read(std::io::Error),
    #[error("writing the stick failed: {0}")]
    Write(std::io::Error),
    #[error("verification failed: the stick differs from the image at byte {0}")]
    Mismatch(u64),
}
