#![forbid(unsafe_code)]
//! Runs once when the live ISO boots (`borea-probe.service`, see `dist/`), before the installer
//! starts. It scans the machine the ISO booted on and writes what it found, plus the driver plan for
//! each distro, to `/run/borea/hardware.json`. The installer only reads that file: picking a distro
//! on page 03 swaps which plan applies, without probing again.
//!
//! Nothing here is specific to any model; the sample machines in [`crate::hardware::samples`] exist
//! only for tests and the design prototype.

use std::fs;
use std::io;
use std::path::Path;

use crate::drivers::{DriverPlan, plan_drivers};
use crate::hardware::{Hardware, MuxMode, pci_ids, scan};
use crate::source::Distro;

/// Where the live ISO keeps hwdata's PCI name list (Arch/Fedora: hwdata, Debian: pci.ids).
pub const PCI_IDS: &[&str] = &["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids"];
pub const OUT: &str = "/run/borea/hardware.json";

/// Scan the running machine.
pub fn probe() -> io::Result<Hardware> {
    let ids = PCI_IDS.iter().find_map(|p| fs::read_to_string(p).ok()).unwrap_or_default();
    let names = pci_ids(&ids);
    scan(Path::new("/"), &names)
}

fn q(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn list<T>(xs: &[T], f: impl Fn(&T) -> String) -> String {
    format!("[{}]", xs.iter().map(f).collect::<Vec<_>>().join(","))
}

fn plan_json(p: &DriverPlan) -> String {
    format!(
        "{{\"found\":{},\"packages\":{},\"sources\":{},\"services\":{},\"kernelArgs\":{},\"modprobe\":{},\"mok\":{},\"warning\":{}}}",
        list(&p.found, |f| format!("[{},{}]", q(&f.what), q(&f.how))),
        list(&p.packages, |x| format!("[{},{}]", q(x.source.key()), q(x.name))),
        list(&p.sources, |x| q(x.key())),
        list(&p.services, |x| q(x)),
        list(&p.kernel_args, |x| q(x)),
        list(&p.modprobe, |x| q(x)),
        p.mok,
        p.warning.as_deref().map(q).unwrap_or_else(|| "null".into())
    )
}

/// What the installer reads: the machine, its GPUs, and a driver plan per distro.
pub fn report(hw: &Hardware) -> String {
    let gpus = list(&hw.gpus, |g| {
        format!(
            "{{\"slot\":{},\"id\":\"{:04x}:{:04x}\",\"name\":{},\"discrete\":{},\"bootVga\":{},\"drivesPanel\":{}}}",
            q(&g.slot),
            g.vendor,
            g.device,
            q(&g.name),
            g.is_discrete(hw.gpus.len()),
            g.boot_vga,
            g.drives_panel
        )
    });
    let mux = match &hw.mux {
        Some(m) => q(if m.mode == MuxMode::Hybrid { "hybrid" } else { "discrete" }),
        None => "null".into(),
    };
    let plans: Vec<String> = Distro::ALL
        .iter()
        .map(|d| format!("{}:{}", q(d.key()), plan_json(&plan_drivers(hw, *d))))
        .collect();
    format!(
        "{{\"name\":{},\"laptop\":{},\"hybrid\":{},\"mux\":{},\"secureBoot\":{},\"gpus\":{},\"drivers\":{{{}}}}}",
        q(&hw.display_name()),
        hw.is_laptop(),
        hw.hybrid(),
        mux,
        hw.secure_boot,
        gpus,
        plans.join(",")
    )
}

/// Probe and write [`OUT`] (or `out`). Returns the machine so callers can log it.
pub fn write(out: &Path) -> io::Result<Hardware> {
    let hw = probe()?;
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(out, report(&hw))?;
    Ok(hw)
}
