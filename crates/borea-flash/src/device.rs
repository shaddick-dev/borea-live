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
use zbus::zvariant::{OwnedObjectPath, Value};

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
    let options: HashMap<&str, Value<'_>> = HashMap::from([("flags", Value::from(libc::O_EXCL))]);
    let fd: zbus::zvariant::OwnedFd =
        proxy(&bus, &block_path(name), BLOCK)?.call("OpenDevice", &("rw", options)).map_err(
            |e| FlashError::Access(format!("permission to write {name} was not granted ({e})")),
        )?;
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
            let options: HashMap<&str, Value<'_>> = HashMap::new();
            fs.call::<_, _, ()>("Unmount", &(options,))
                .map_err(|e| FlashError::Access(format!("could not unmount {name}: {e}")))?;
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
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    proxy(&bus, drive.as_str(), DRIVE)?
        .call::<_, _, ()>("PowerOff", &(options,))
        .map_err(|e| FlashError::Access(format!("could not eject {name}: {e}")))
}
