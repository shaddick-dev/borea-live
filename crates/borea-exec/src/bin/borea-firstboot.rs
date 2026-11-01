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

Usage: borea COMMAND

  install NAME…   Add apps, extras or any package: Borea's catalog first (by name
                  or id), then this system's repositories, then the AUR on Arch,
                  then Flathub by app id (as root)
  remove NAME…    Remove what Borea added, nothing else (as root)
  list            What Borea added, and where from
  status          The apps chosen at installation: installed, waiting or retrying

Installed by Borea; the apps chosen while installing finish quietly after the
first login (borea-firstboot.service).";
fn main() -> std::process::ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("--help" | "-h" | "help") | None
            if args.len() <= 1 && !invoked_as("borea-firstboot") =>
        {
            println!("{USAGE}");
            return std::process::ExitCode::SUCCESS;
        }
        Some("list") if args.len() == 1 => return report(borea_exec::system::list()),
        Some("status") if args.len() == 1 => {
            return report(borea_exec::postinstall::show_status());
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
        [command, names @ ..] if command == "install" && !names.is_empty() => {
            report(borea_exec::system::install(names))
        }
        [command, names @ ..] if command == "remove" && !names.is_empty() => {
            report(borea_exec::system::remove(names))
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
