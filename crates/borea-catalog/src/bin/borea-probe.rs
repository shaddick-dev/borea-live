#![forbid(unsafe_code)]
//! `borea-probe`                 print what this machine is and which drivers each distro would get
//! `borea-probe --write [path]`  write the installer's hardware file (default /run/borea/hardware.json)
//!
//! Runs at boot on the live ISO via dist/borea-probe.service. Safe to run on any Linux machine:
//! it only reads /sys, /proc and the PCI name list.

use std::path::PathBuf;
use std::process::ExitCode;

use borea_catalog::probe;
use borea_catalog::{Distro, plan_drivers};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--write") {
        let out = PathBuf::from(args.get(1).map(String::as_str).unwrap_or(probe::OUT));
        return match probe::write(&out) {
            Ok(hw) => {
                eprintln!("borea-probe: {} -> {}", hw.display_name(), out.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("borea-probe: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let hw = match probe::probe() {
        Ok(hw) => hw,
        Err(e) => {
            eprintln!("borea-probe: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("{} · {}", hw.display_name(), if hw.is_laptop() { "laptop" } else { "desktop" });
    for g in &hw.gpus {
        println!(
            "  {}  {:04x}:{:04x}  {:?}{}",
            g.name,
            g.vendor,
            g.device,
            g.family,
            if g.drives_panel { "  (drives the screen)" } else { "" }
        );
    }
    if let Some(m) = &hw.mux {
        println!(
            "  MUX: {:?}{}",
            m.mode,
            m.control.as_deref().map(|c| format!(" via {c}")).unwrap_or_default()
        );
    }
    println!("  Secure Boot: {}", if hw.secure_boot { "on" } else { "off" });
    for d in Distro::ALL {
        let p = plan_drivers(&hw, d);
        let names: Vec<&str> = p.packages.iter().map(|x| x.name).collect();
        println!("\n{}: {}", d.name(), names.join(" "));
        if let Some(w) = p.warning {
            println!("  ! {w}");
        }
    }
    ExitCode::SUCCESS
}
