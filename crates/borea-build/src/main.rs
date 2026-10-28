//! `borea-build`: everything that turns this workspace into Borea Live.
//!
//! ```text
//! borea-build iso [--configure-only] [--secure-boot DIR] [--jobs N]   build the ISO with Buildroot
//! borea-build keys DIR                                     make a Secure Boot signing key
//! borea-build concept-data                                 refresh design/windpad.html's catalog
//! ```
//!
//! Buildroot also runs it as its post-build and post-image hooks: `iso` links
//! `target/live/hooks/post-build` and `post-image` to this binary, which then acts
//! on the name it was started as.

mod compose;
mod concept;
mod hooks;
mod initramfs;
mod iso;
mod keys;
mod run;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub type Result<T = ()> = std::result::Result<T, String>;

const USAGE: &str = "\
usage: borea-build compose [--refresh-runtime] [--ca-cert PATH] [--bin-dir PATH]
       borea-build iso [--configure-only] [--secure-boot DIR] [--jobs N]
       borea-build keys DIR
       borea-build concept-data";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().collect();
    let invoked_as = Path::new(&args.remove(0))
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    let result = match (invoked_as.as_str(), args.as_slice()) {
        ("post-build", [target_dir, ..]) => hooks::post_build(Path::new(target_dir)),
        ("post-image", [images_dir, ..]) => hooks::post_image(Path::new(images_dir)),
        (_, ["iso", rest @ ..]) => iso::run(rest),
        (_, ["compose", rest @ ..]) => compose::run(rest),
        (_, ["compose-root"]) => compose::root(),
        (_, ["keys", dir]) => keys::make(Path::new(dir)),
        (_, ["concept-data"]) => repo().and_then(|repo| concept::refresh(&repo)),
        (_, ["-h" | "--help" | "help"]) => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("borea-build: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The Borea checkout: the nearest directory, from here or from this binary, that
/// holds `live/buildroot/external.desc`.
pub fn repo() -> Result<PathBuf> {
    let starts = [std::env::current_dir().ok(), std::env::current_exe().ok()];
    starts
        .into_iter()
        .flatten()
        .flat_map(|start| start.ancestors().map(Path::to_path_buf).collect::<Vec<_>>())
        .find(|dir| dir.join("live/buildroot/external.desc").is_file())
        .ok_or_else(|| "run this inside the Borea checkout".into())
}
