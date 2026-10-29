//! Access to real block devices.
//!
//! Run as root, devices are opened directly and exclusively. Otherwise the request
//! goes through UDisks2, which asks polkit for permission: the same path GNOME Disks
//! uses to restore disk images, so `borea-flash` never needs `sudo` on a desktop.

use std::collections::HashMap;
use std::fs::File;
use std::os::unix::fs::OpenOptionsExt;

use borea_hardware::Disk;
use zbus::blocking::{Connection, Proxy};
use zbus::proxy::MethodFlags;
use zbus::zvariant::{DynamicDeserialize, DynamicType, OwnedObjectPath, Value};

use crate::FlashError;

const UDISKS: &str = "org.freedesktop.UDisks2";
const BLOCK: &str = "org.freedesktop.UDisks2.Block";
const FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
const DRIVE: &str = "org.freedesktop.UDisks2.Drive";

fn block_path(name: &str) -> String {
    format!("/org/freedesktop/UDisks2/block_devices/{name}")
}

/// Whether this process runs with an effective UID of 0.
pub fn is_root() -> bool {
    std::fs::read_to_string("/proc/self/status").ok().is_some_and(|status| {
        status
            .lines()
            .find_map(|l| l.strip_prefix("Uid:"))
            .and_then(|ids| ids.split_whitespace().nth(1).map(|euid| euid == "0"))
            .unwrap_or(false)
    })
}

fn udisks() -> Result<Connection, FlashError> {
    Connection::system().map_err(|e| FlashError::Access(format!("system bus unavailable: {e}")))
}

fn proxy<'c>(
    bus: &'c Connection,
    path: &str,
    interface: &'static str,
) -> Result<Proxy<'c>, FlashError> {
    Proxy::new(bus, UDISKS, path.to_owned(), interface)
        .map_err(|e| FlashError::Access(e.to_string()))
}

/// Call a UDisks method that polkit may guard. Without D-Bus's
/// ALLOW_INTERACTIVE_AUTHORIZATION flag, polkit never shows the password
/// prompt and UDisks answers NotAuthorizedCanObtain; GNOME Disks sets it too.
fn call_allowing_prompt<B, R>(proxy: &Proxy<'_>, method: &str, body: &B) -> zbus::Result<R>
where
    B: zbus::export::serde::ser::Serialize + DynamicType,
    R: for<'d> DynamicDeserialize<'d>,
{
    proxy
        .call_with_flags(method, MethodFlags::AllowInteractiveAuth.into(), body)?
        .ok_or_else(|| zbus::Error::Failure(format!("{method} returned no reply")))
}

/// Options for a UDisks call that may ask for a password.
fn prompt_options<'a>() -> HashMap<&'static str, Value<'a>> {
    HashMap::from([("auth.no_user_interaction", Value::from(false))])
}

/// Why UDisks refused, in words a person can act on.
fn refused(what: &str, error: zbus::Error) -> FlashError {
    let text = error.to_string();
    if text.contains("NotAuthorizedCanObtain") || text.contains("NotAuthorized") {
        FlashError::Access(format!(
            "{what} needs your password, but no password prompt could be shown. Run \
             borea-rataTUI from your desktop session (not over SSH or on a text console), \
             or run it with sudo"
        ))
    } else {
        FlashError::Access(format!("{what} was refused ({text})"))
    }
}

/// Open a whole disk for reading and writing, exclusively: the kernel refuses while
/// anything has it mounted.
pub fn open(name: &str) -> Result<File, FlashError> {
    if is_root() {
        return File::options()
            .read(true)
            .write(true)
            .custom_flags(libc::O_EXCL)
            .open(format!("/dev/{name}"))
            .map_err(|e| FlashError::Access(format!("/dev/{name}: {e}")));
    }
    let bus = udisks()?;
    let mut options = prompt_options();
    options.insert("flags", Value::from(libc::O_EXCL));
    let fd: zbus::zvariant::OwnedFd = call_allowing_prompt(
        &proxy(&bus, &block_path(name), BLOCK)?,
        "OpenDevice",
        &("rw", options),
    )
    .map_err(|e| refused(&format!("Writing {name}"), e))?;
    Ok(File::from(std::os::fd::OwnedFd::from(fd)))
}

/// Unmount every mounted filesystem on the disk.
pub fn unmount(disk: &Disk) -> Result<(), FlashError> {
    let bus = udisks()?;
    let names =
        std::iter::once(disk.name.as_str()).chain(disk.partitions.iter().map(|p| p.name.as_str()));
    for name in names {
        let Ok(fs) = proxy(&bus, &block_path(name), FILESYSTEM) else { continue };
        let mounted: Vec<Vec<u8>> = fs.get_property("MountPoints").unwrap_or_default();
        if !mounted.is_empty() {
            call_allowing_prompt::<_, ()>(&fs, "Unmount", &(prompt_options(),))
                .map_err(|e| refused(&format!("Unmounting {name}"), e))?;
        }
    }
    Ok(())
}

/// Power the stick off so it can be pulled out safely.
pub fn eject(name: &str) -> Result<(), FlashError> {
    let bus = udisks()?;
    let drive: OwnedObjectPath = proxy(&bus, &block_path(name), BLOCK)?
        .get_property("Drive")
        .map_err(|e| FlashError::Access(e.to_string()))?;
    call_allowing_prompt::<_, ()>(
        &proxy(&bus, drive.as_str(), DRIVE)?,
        "PowerOff",
        &(prompt_options(),),
    )
    .map_err(|e| refused(&format!("Ejecting {name}"), e))
}
