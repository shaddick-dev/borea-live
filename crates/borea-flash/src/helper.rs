//! Build Borea Live when needed, then write and verify a USB stick.
#[path = "main.rs"]
mod flash;

fn main() -> std::process::ExitCode {
    flash::main()
}
