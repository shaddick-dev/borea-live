#![forbid(unsafe_code)]
//! Borea Live as a hub: prepare the disk the way the user chose, then start
//! another distribution's own installer from the stick.
//!
//! - **Whole disk**: nothing is changed here; the distribution's installer wipes
//!   the disk itself, after its own confirmation.
//! - **Alongside**: Borea shrinks Windows by the size the user chose, with the
//!   same checks as a native install (BitLocker off, Fast Startup off, NTFS
//!   healthy, 30 GiB left for Windows). The installer then installs into that
//!   free space.
//!
//! The installer's ISO is downloaded to the stick's data partition, checked
//! against its published SHA-256 and the distribution's pinned signing key, and
//! booted from that file with `kexec`, using the ISO's own loopback entry.
pub mod boot;
pub mod exec;
pub mod extras;
pub mod plan;
pub mod stick;
pub mod upstream;
