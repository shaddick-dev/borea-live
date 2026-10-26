#![forbid(unsafe_code)]
fn main() -> std::process::ExitCode {
    if !nix::unistd::Uid::effective().is_root() {
        eprintln!("borea-firstboot requires root.");
        return std::process::ExitCode::FAILURE;
    }
    let result = if std::env::args().nth(1).as_deref() == Some("--appimages") {
        borea_exec::postinstall::update_images()
    } else {
        borea_exec::postinstall::firstboot()
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}
