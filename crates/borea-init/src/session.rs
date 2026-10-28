//! Launch the live desktop with a usable software fallback on troublesome GPUs.
use std::{
    fs,
    process::{Command, ExitCode},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Graphics {
    Hardware,
    Software,
}

fn software_requested(cmdline: &str) -> bool {
    cmdline
        .split_whitespace()
        .any(|arg| matches!(arg, "borea.safe-graphics" | "borea.graphics=software" | "nomodeset"))
}

fn compositor(graphics: Graphics) -> Command {
    let mut command = Command::new("/usr/bin/cage");
    command.args(["-d", "-s", "--", "/usr/bin/borea-live"]);
    // Ignore renderer overrides inherited from a developer's terminal.
    for key in [
        "WLR_RENDERER",
        "WLR_RENDERER_ALLOW_SOFTWARE",
        "WLR_NO_HARDWARE_CURSORS",
        "LIBGL_ALWAYS_SOFTWARE",
    ] {
        command.env_remove(key);
    }
    if graphics == Graphics::Software {
        command
            .env("WLR_RENDERER", "pixman")
            .env("WLR_RENDERER_ALLOW_SOFTWARE", "1")
            .env("WLR_NO_HARDWARE_CURSORS", "1")
            .env("LIBGL_ALWAYS_SOFTWARE", "1");
    }
    command
}

fn launch(
    cmdline: &str,
    mut run: impl FnMut(Graphics) -> Result<bool, String>,
) -> Result<(), String> {
    let modes: &[Graphics] = if software_requested(cmdline) {
        &[Graphics::Software]
    } else {
        &[Graphics::Hardware, Graphics::Software]
    };
    let mut last_error = String::new();
    for mode in modes {
        eprintln!("BOREA: starting installer with {mode:?} graphics.");
        match run(*mode) {
            Ok(true) => return Ok(()),
            Ok(false) => last_error = format!("the {mode:?} compositor exited unsuccessfully"),
            Err(error) => last_error = error,
        }
        eprintln!("BOREA: {last_error}");
    }
    Err(format!(
        "The desktop could not start: {last_error}. Press Ctrl+Alt+F2 for the console, or reboot and choose Borea Live (safe graphics)."
    ))
}

fn main() -> ExitCode {
    let cmdline = fs::read_to_string("/proc/cmdline").unwrap_or_default();
    match launch(&cmdline, |graphics| {
        compositor(graphics)
            .status()
            .map(|status| status.success())
            .map_err(|error| error.to_string())
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("BOREA: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_boot_skips_hardware_and_sets_both_compositor_and_client_renderers() {
        for flag in ["borea.safe-graphics", "nomodeset", "borea.graphics=software"] {
            let mut modes = Vec::new();
            assert!(
                launch(flag, |mode| {
                    modes.push(mode);
                    Ok(true)
                })
                .is_ok()
            );
            assert_eq!(modes, [Graphics::Software]);
        }
        assert!(!software_requested("notnomodeset borea.safe-graphics=0"));
        let command = compositor(Graphics::Software);
        let values: std::collections::BTreeMap<_, _> = command.get_envs().collect();
        assert_eq!(
            values.get(std::ffi::OsStr::new("WLR_RENDERER")),
            Some(&Some(std::ffi::OsStr::new("pixman")))
        );
        assert_eq!(
            values.get(std::ffi::OsStr::new("LIBGL_ALWAYS_SOFTWARE")),
            Some(&Some(std::ffi::OsStr::new("1")))
        );
    }

    #[test]
    fn failed_hardware_falls_back_once_and_failed_software_reports_an_error() {
        let mut modes = Vec::new();
        assert!(
            launch("", |mode| {
                modes.push(mode);
                Ok(mode == Graphics::Software)
            })
            .is_ok()
        );
        assert_eq!(modes, [Graphics::Hardware, Graphics::Software]);
        let mut count = 0;
        assert!(
            launch("", |_| {
                count += 1;
                Err("missing compositor".into())
            })
            .is_err()
        );
        assert_eq!(count, 2);
    }
}
