//! Launch the live desktop: on the GPU when the machine has a real one (fast on
//! any desktop or laptop), in software in a VM or with safe graphics, and back
//! to software if the GPU path fails.
use std::{
    fs,
    process::{Command, ExitCode},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Graphics {
    Hardware,
    Software,
}

/// Software only: the safe-graphics entry, or the kernel told not to use the GPU.
fn software_requested(cmdline: &str) -> bool {
    cmdline
        .split_whitespace()
        .any(|arg| matches!(arg, "borea.safe-graphics" | "borea.graphics=software" | "nomodeset"))
}

fn hardware_requested(cmdline: &str) -> bool {
    cmdline.split_whitespace().any(|arg| arg == "borea.graphics=hardware")
}

/// Kernel graphics drivers of real GPUs. Virtual and firmware framebuffers
/// (virtio-gpu, bochs, QXL, simpledrm, efifb) stay on software, which is what
/// left a VM's display blank on the hardware path.
const REAL_GPUS: &[&str] = &["i915", "xe", "amdgpu", "radeon", "nouveau"];

fn real_gpu(drivers: &[String]) -> bool {
    drivers.iter().any(|d| REAL_GPUS.contains(&d.as_str()))
}

/// The drivers bound to the machine's DRM devices.
fn gpu_drivers() -> Vec<String> {
    fs::read_dir("/sys/class/drm")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("card"))
        .filter_map(|e| fs::read_link(e.path().join("device/driver")).ok())
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect()
}

fn modes(cmdline: &str, drivers: &[String]) -> &'static [Graphics] {
    if software_requested(cmdline) {
        &[Graphics::Software]
    } else if hardware_requested(cmdline) || real_gpu(drivers) {
        &[Graphics::Hardware, Graphics::Software]
    } else {
        &[Graphics::Software]
    }
}

fn compositor(graphics: Graphics, smoke: bool) -> Command {
    let mut command = Command::new("/usr/bin/cage");
    command.args(["-d", "-s", "--", "/usr/bin/borea-live"]);
    if smoke {
        command.arg("--boot-smoke");
    }
    // Ignore renderer overrides inherited from a developer's terminal.
    for key in [
        "WLR_RENDERER",
        "WLR_RENDERER_ALLOW_SOFTWARE",
        "WLR_NO_HARDWARE_CURSORS",
        "LIBGL_ALWAYS_SOFTWARE",
        "ICED_BACKEND",
        "BOREA_GRAPHICS",
    ] {
        command.env_remove(key);
    }
    if graphics == Graphics::Software {
        command
            .env("WLR_RENDERER", "pixman")
            .env("WLR_RENDERER_ALLOW_SOFTWARE", "1")
            .env("WLR_NO_HARDWARE_CURSORS", "1")
            .env("LIBGL_ALWAYS_SOFTWARE", "1")
            // The installer draws on the CPU too, and keeps its motion light.
            .env("ICED_BACKEND", "tiny-skia")
            .env("BOREA_GRAPHICS", "software");
    } else {
        // GPU first (wgpu: Vulkan or OpenGL), the CPU renderer if that fails.
        command.env("ICED_BACKEND", "wgpu,tiny-skia").env("BOREA_GRAPHICS", "hardware");
    }
    command
}

fn launch(
    cmdline: &str,
    drivers: &[String],
    mut run: impl FnMut(Graphics) -> Result<bool, String>,
) -> Result<(), String> {
    let modes = modes(cmdline, drivers);
    let mut last_error = String::new();
    for mode in modes {
        eprintln!("Borea: starting installer with {mode:?} graphics.");
        match run(*mode) {
            Ok(true) => return Ok(()),
            Ok(false) => last_error = format!("the {mode:?} compositor exited unsuccessfully"),
            Err(error) => last_error = error,
        }
        eprintln!("Borea: {last_error}");
    }
    Err(format!(
        "The desktop could not start: {last_error}. Press Ctrl+Alt+F2 for the console, or reboot and choose Borea Live (safe graphics)."
    ))
}

fn main() -> ExitCode {
    let cmdline = fs::read_to_string("/proc/cmdline").unwrap_or_default();
    let drivers = gpu_drivers();
    eprintln!(
        "Borea: graphics drivers: {}",
        if drivers.is_empty() { "none".to_owned() } else { drivers.join(", ") }
    );
    match launch(&cmdline, &drivers, |graphics| {
        compositor(graphics, cmdline.split_whitespace().any(|arg| arg == "borea.boot-smoke"))
            .status()
            .map(|status| status.success())
            .map_err(|error| error.to_string())
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Borea: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    fn env(command: &Command, key: &str) -> Option<String> {
        command
            .get_envs()
            .find(|(k, _)| *k == std::ffi::OsStr::new(key))
            .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
    }

    #[test]
    fn safe_boot_skips_hardware_and_sets_both_compositor_and_client_renderers() {
        for flag in ["borea.safe-graphics", "nomodeset", "borea.graphics=software"] {
            let mut modes = Vec::new();
            assert!(
                launch(flag, &d(&["i915"]), |mode| {
                    modes.push(mode);
                    Ok(true)
                })
                .is_ok()
            );
            assert_eq!(modes, [Graphics::Software], "{flag}");
        }
        let command = compositor(Graphics::Software, false);
        assert_eq!(env(&command, "WLR_RENDERER").as_deref(), Some("pixman"));
        assert_eq!(env(&command, "LIBGL_ALWAYS_SOFTWARE").as_deref(), Some("1"));
        assert_eq!(env(&command, "ICED_BACKEND").as_deref(), Some("tiny-skia"));
    }

    #[test]
    fn a_real_gpu_draws_on_the_gpu_first() {
        for gpu in ["i915", "xe", "amdgpu", "radeon", "nouveau"] {
            let mut modes = Vec::new();
            assert!(
                launch("quiet", &d(&["simpledrm", gpu]), |mode| {
                    modes.push(mode);
                    Ok(true)
                })
                .is_ok()
            );
            assert_eq!(modes, [Graphics::Hardware], "{gpu}");
        }
        let command = compositor(Graphics::Hardware, false);
        assert_eq!(env(&command, "ICED_BACKEND").as_deref(), Some("wgpu,tiny-skia"));
        assert_eq!(env(&command, "WLR_RENDERER"), None);
    }

    #[test]
    fn a_vm_or_firmware_framebuffer_stays_on_software() {
        for drivers in
            [d(&[]), d(&["virtio-pci"]), d(&["bochs-drm"]), d(&["simpledrm"]), d(&["qxl"])]
        {
            assert_eq!(modes("quiet", &drivers), [Graphics::Software], "{drivers:?}");
        }
        assert_eq!(
            modes("borea.graphics=hardware", &d(&[])),
            [Graphics::Hardware, Graphics::Software]
        );
    }

    #[test]
    fn failed_hardware_falls_back_once_and_failed_software_reports_an_error() {
        let mut modes = Vec::new();
        assert!(
            launch("quiet", &d(&["amdgpu"]), |mode| {
                modes.push(mode);
                Ok(mode == Graphics::Software)
            })
            .is_ok()
        );
        assert_eq!(modes, [Graphics::Hardware, Graphics::Software]);
        let mut count = 0;
        assert!(
            launch("quiet", &d(&["amdgpu"]), |_| {
                count += 1;
                Err("missing compositor".into())
            })
            .is_err()
        );
        assert_eq!(count, 2);
    }
}
