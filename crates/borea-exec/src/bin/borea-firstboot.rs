#![forbid(unsafe_code)]
fn invoked_as(name: &str) -> bool {
    std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(std::path::Path::file_name)
        .is_some_and(|n| n == name)
}
fn report(result: Result<(), borea_core::Hint>) -> std::process::ExitCode {
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
const USAGE: &str = "\
borea — this system's apps, the Borea way.

Usage: borea COMMAND [OPTIONS] [NAME…]

  in NAME…        Install apps, extras or any package (install, i)
  rm NAME…        Remove what Borea added, nothing else (remove, uninstall)
  ls              What Borea added, and where from (list)
  s TERM          Search Borea's catalog, this system's repositories, the AUR
                  and Flathub (search)
  up              Update the system, Borea's AUR apps and Flatpaks (update)
  which NAME…     Where each would come from, installing nothing (info)
  st              The apps chosen while installing (status)

Where `in` and `which` look, unless told:
  Borea's catalog by name or id (the AUR first on Arch, the system's own
  repositories elsewhere, then Flathub), then this system's repositories, then
  the AUR on Arch, then a Flathub app id.

  -n, --native    Only this system's repositories
  -a, --aur       Only the AUR (Arch and CachyOS)
  -f, --flatpak   Only Flathub: a catalog app by name, or a Flathub app id
  -y, --yes       Accepted; Borea never asks

Changing the system needs root: sudo borea in firefox";
/// The source flags and names after a command.
fn split(args: &[String]) -> Result<(borea_exec::system::Pin, Vec<String>), String> {
    use borea_exec::system::Pin;
    let mut pin = Pin::Any;
    let mut names = Vec::new();
    for arg in args {
        let next = match arg.as_str() {
            "-n" | "--native" => Pin::Native,
            "-a" | "--aur" => Pin::Aur,
            "-f" | "--flatpak" => Pin::Flatpak,
            "-y" | "--yes" | "--noconfirm" => continue,
            flag if flag.starts_with('-') => return Err(format!("Unknown option {flag}.")),
            name => {
                names.push(name.to_owned());
                continue;
            }
        };
        if pin != Pin::Any && pin != next {
            return Err("Choose one source: -n, -a or -f.".into());
        }
        pin = next;
    }
    Ok((pin, names))
}
fn main() -> std::process::ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let command = args.first().map(String::as_str);
    let rest = args.get(1..).unwrap_or_default();
    let (pin, names) = match split(rest) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return std::process::ExitCode::FAILURE;
        }
    };
    match command {
        Some("--help" | "-h" | "help") | None
            if rest.is_empty() && !invoked_as("borea-firstboot") =>
        {
            println!("{USAGE}");
            return std::process::ExitCode::SUCCESS;
        }
        Some("ls" | "list") if rest.is_empty() => return report(borea_exec::system::list()),
        Some("st" | "status") if rest.is_empty() => {
            return report(borea_exec::postinstall::show_status());
        }
        Some("which" | "info") if !names.is_empty() => {
            return report(borea_exec::system::which(&names, pin));
        }
        Some("rm" | "remove" | "uninstall") if pin != borea_exec::system::Pin::Any => {
            eprintln!(
                "`borea rm` removes what Borea added, from wherever it came; it takes no -n, -a or -f."
            );
            return std::process::ExitCode::FAILURE;
        }
        Some("s" | "search") if names.len() == 1 && pin == borea_exec::system::Pin::Any => {
            return report(borea_exec::system::search(&names[0]));
        }
        _ => {}
    }
    if args.first().map(String::as_str) == Some("--extract-appimage") && args.len() == 3 {
        return report(borea_exec::postinstall::extract_image(&args[1], &args[2]));
    }
    if args.as_slice() == ["--status"] {
        return report(borea_exec::postinstall::show_status());
    }
    if !nix::unistd::Uid::effective().is_root() {
        eprintln!("This needs root: run it with sudo.");
        return std::process::ExitCode::FAILURE;
    }
    if std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(std::path::Path::file_name)
        .is_some_and(|name| name == "update-initramfs")
    {
        println!("Borea: deferring initramfs generation until boot configuration is complete.");
        return std::process::ExitCode::SUCCESS;
    }
    match args.as_slice() {
        [command, ..]
            if matches!(command.as_str(), "in" | "i" | "install") && !names.is_empty() =>
        {
            report(borea_exec::system::install(&names, pin))
        }
        [command, ..]
            if matches!(command.as_str(), "rm" | "remove" | "uninstall")
                && !names.is_empty()
                && pin == borea_exec::system::Pin::Any =>
        {
            report(borea_exec::system::remove(&names))
        }
        [command] if matches!(command.as_str(), "up" | "update") => {
            report(borea_exec::system::update())
        }
        [] => report(borea_exec::postinstall::firstboot()),
        [command] if command == "apps" => report(borea_exec::postinstall::firstboot()),
        [command, mode, user] if command == "apps" && mode == "--after-login" => {
            report(borea_exec::postinstall::after_login(user))
        }
        [mode] if mode == "--aur" => report(borea_exec::postinstall::aur_apps()),
        [mode, user] if mode == "--after-login" => {
            report(borea_exec::postinstall::after_login(user))
        }
        [mode] if mode == "--appimages" => report(borea_exec::postinstall::update_images()),
        _ => {
            eprintln!("Unknown command.\n\n{USAGE}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use borea_exec::system::Pin;
    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }
    #[test]
    fn source_flags_and_names_split_in_any_order() {
        assert_eq!(split(&args(&["-f", "firefox"])), Ok((Pin::Flatpak, args(&["firefox"]))));
        assert_eq!(split(&args(&["htop", "--native", "-y"])), Ok((Pin::Native, args(&["htop"]))));
        assert_eq!(split(&args(&["-a", "yay-bin", "-a"])), Ok((Pin::Aur, args(&["yay-bin"]))));
        assert!(split(&args(&["-a", "-f", "x"])).is_err(), "one source at a time");
        assert!(split(&args(&["--bogus"])).is_err());
    }
}
