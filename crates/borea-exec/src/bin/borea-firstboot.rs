#![forbid(unsafe_code)]
fn report(result: Result<(), borea_core::Hint>) -> std::process::ExitCode {
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
fn main() -> std::process::ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("--help") {
        println!(
            "borea-firstboot [--appimages | --after-login USER | --status]\nborea-firstboot --extract-appimage DIRECTORY FILENAME (unprivileged only)"
        );
        return std::process::ExitCode::SUCCESS;
    }
    if args.first().map(String::as_str) == Some("--extract-appimage") && args.len() == 3 {
        return report(borea_exec::postinstall::extract_image(&args[1], &args[2]));
    }
    if args.as_slice() == ["--status"] {
        return report(borea_exec::postinstall::show_status());
    }
    if !nix::unistd::Uid::effective().is_root() {
        eprintln!("borea-firstboot requires root.");
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
        [] => report(borea_exec::postinstall::firstboot()),
        [mode, user] if mode == "--after-login" => {
            report(borea_exec::postinstall::after_login(user))
        }
        [mode] if mode == "--appimages" => report(borea_exec::postinstall::update_images()),
        _ => {
            eprintln!("Unknown first-boot helper option.");
            std::process::ExitCode::FAILURE
        }
    }
}
