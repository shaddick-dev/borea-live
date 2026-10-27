//! `borea doctor` and `borea catalog`.

use std::io::{self, Write};

use borea_distro::{Finding, adapter, assess, machine_findings};
use borea_hardware::{DiskUsage, HardwareSnapshot};
use borea_host::{HostCapability, HostEnvironment, HostKind};
use borea_live_catalog::Catalog;
use borea_live_core::{Fact, Family};

use crate::Result;
use crate::ui::{GIB, Out, list, row, section};

pub fn catalog(out: &mut Out) -> Result {
    let catalog = Catalog::bundled()?;
    let trust = if catalog.signed { "signed" } else { "unsigned development data" };
    writeln!(out, "Windpad catalog · reviewed {} · {trust}\n", catalog.reviewed)?;
    for family in Family::ALL {
        writeln!(out, "{family}")?;
        for distro in catalog.family(family) {
            writeln!(out, "  {} · {} · {}", distro.name, distro.channel, distro.status)?;
            writeln!(out, "    editions: {}", list(distro.editions.iter().map(|e| &e.name)))?;
            if !distro.variants.is_empty() {
                writeln!(out, "    variants: {}", list(&distro.variants))?;
            }
            if let Some(edition) = distro.composable() {
                writeln!(
                    out,
                    "    {}: compose your own desktop → borea shells {}",
                    edition.name, distro.id
                )?;
            }
            if !distro.matrix_complete {
                writeln!(out, "    full upstream edition matrix not yet represented")?;
            }
        }
        writeln!(out)?;
    }
    writeln!(out, "Versions are resolved when an installation is prepared, never stored here.")?;
    Ok(())
}

pub fn run(out: &mut Out) -> Result {
    let catalog = Catalog::bundled()?;
    let host = borea_host::detect();
    let hw = borea_hardware::scan();

    section(out, "Installation environment")?;
    environment(out, &host)?;
    section(out, "This machine")?;
    machine(out, &hw)?;
    section(out, "Targets from this environment")?;
    for distro in Family::ALL.into_iter().flat_map(|family| catalog.family(family)) {
        let Some(spec) = adapter(distro.id.as_str()) else {
            row(out, &distro.name, "no adapter")?;
            continue;
        };
        let a = assess(distro, spec, &host, &hw);
        row(out, &distro.name, a.verdict())?;
        row(out, "", format_args!("via {}", spec.acquisition))?;
        if !a.missing_host.is_empty() {
            row(out, "", format_args!("host lacks: {}", list(&a.missing_host)))?;
        }
        findings(out, &a.findings)?;
        if let Some(edition) = distro.composable() {
            let compositors = catalog.desktops.compositors(&distro.id).filter(|o| o.available());
            let names = list(compositors.map(|o| &o.item.name));
            row(out, "", format_args!("◇ {}: compose with {names}", edition.name))?;
        }
        let (built_in, optional): (Vec<_>, Vec<_>) =
            catalog.extras.on(&distro.id).partition(|o| o.provision.built_in);
        if !optional.is_empty() {
            let names = optional.iter().map(|o| match o.provision.default {
                true => format!("{} (ticked)", o.extra.name),
                false => o.extra.name.clone(),
            });
            row(out, "", format_args!("◇ Extras: {}", list(names)))?;
        }
        if !built_in.is_empty() {
            row(
                out,
                "",
                format_args!("◇ Built in: {}", list(built_in.iter().map(|o| &o.extra.name))),
            )?;
        }
    }
    writeln!(
        out,
        "\nNo target is installable yet: adapters are planned, and this catalog is unsigned.\n\
         Nothing on this machine was changed."
    )?;
    Ok(())
}

fn findings(out: &mut Out, findings: &[Finding]) -> io::Result<()> {
    for finding in findings {
        match finding {
            Finding::Blocker(text) => row(out, "", format_args!("✕ {text}"))?,
            Finding::Unknown(text) => row(out, "", format_args!("? {text}"))?,
            Finding::Recommend(text) => row(out, "", format_args!("→ {text}"))?,
        }
    }
    Ok(())
}

fn environment(out: &mut Out, host: &HostEnvironment) -> io::Result<()> {
    row(out, "Kind", &host.kind)?;
    if let Some(os) = &host.os {
        row(out, "Running on", format_args!("{} (never limits targets)", os.pretty_name))?;
    }
    let guidance = match host.kind {
        HostKind::BoreaLive { .. } => "reference environment",
        HostKind::LiveSession => "usable when the capabilities below suffice",
        HostKind::InstalledSystem => "boot Borea Live or another live session to install",
    };
    row(out, "", guidance)?;
    match host.capabilities.is_empty() {
        true => row(out, "Capabilities", "none detected")?,
        false => row(out, "Capabilities", list(&host.capabilities))?,
    }
    if !host.has(HostCapability::NetworkManager) {
        row(out, "Wi-Fi", "unavailable: NetworkManager is not running here")?;
    }
    if !host.has(HostCapability::GraphicalSession) {
        row(out, "Windpad", "unavailable: no Wayland or X11 session")?;
    }
    Ok(())
}

fn machine(out: &mut Out, hw: &HardwareSnapshot) -> io::Result<()> {
    match &hw.cpu {
        Fact::Known(cpu) => {
            let model = cpu.model.as_deref().unwrap_or("unknown model");
            let level = cpu.x86_level.map(|l| format!(" · {l}")).unwrap_or_default();
            row(out, "CPU", format_args!("{model} · {} threads{level}", cpu.logical_cores))?;
        }
        Fact::Unknown { reason } => row(out, "CPU", format_args!("unknown ({reason})"))?,
    }
    match &hw.memory_bytes {
        Fact::Known(bytes) => row(out, "Memory", format_args!("{:.1} GiB", *bytes as f64 / GIB))?,
        unknown => row(out, "Memory", unknown)?,
    }
    let secure_boot = match hw.secure_boot {
        Fact::Known(true) => "on",
        Fact::Known(false) => "off",
        Fact::Unknown { .. } => "unknown",
    };
    row(out, "Firmware", format_args!("{} · Secure Boot {secure_boot}", hw.firmware))?;
    let power = if hw.battery { " · on battery-capable hardware" } else { "" };
    row(out, "Device", format_args!("{}{power}", hw.device_class))?;
    if hw.gpus.is_empty() {
        row(out, "Graphics", "none detected")?;
    }
    for gpu in &hw.gpus {
        let driver = gpu.host_driver.as_deref().unwrap_or("no driver bound");
        let id = gpu.label();
        row(out, "Graphics", format_args!("{id} · host driver {driver} · {}", gpu.pci_slot))?;
    }
    for display in &hw.displays {
        let place = if display.internal { "built-in" } else { "external" };
        let mode = display.mode.as_deref().unwrap_or("mode unknown");
        row(out, "Display", format_args!("{} · {place} · {mode}", display.connector))?;
    }
    for disk in &hw.disks {
        let model = disk.model.as_deref().unwrap_or("unknown model");
        let size = disk.size_bytes as f64 / 1e9;
        let parts = match disk.partitions.len() {
            0 => String::from("no partitions"),
            1 => String::from("1 partition"),
            n => format!("{n} partitions"),
        };
        let marker = if disk.usage == DiskUsage::Free { "○" } else { "●" };
        row(
            out,
            "Disk",
            format_args!(
                "{marker} {} · {model} · {size:.0} GB · {} · {parts} · {}",
                disk.name, disk.transport, disk.usage
            ),
        )?;
    }
    for nic in &hw.network {
        let kind = if nic.wireless { "Wi-Fi" } else { "wired" };
        let driver = nic.host_driver.as_deref().unwrap_or("no driver");
        row(out, "Network", format_args!("{} · {kind} · {driver}", nic.interface))?;
    }
    if !hw.bluetooth.is_empty() {
        row(out, "Bluetooth", list(&hw.bluetooth))?;
    }
    for input in &hw.inputs {
        row(out, "Input", format_args!("{} · {}", input.kind, input.name))?;
    }
    findings(out, &machine_findings(hw))?;
    row(out, "", "no disk is selected; storage is always an explicit later choice")
}
