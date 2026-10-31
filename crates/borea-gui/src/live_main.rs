//! Borea’s Iced frontend over the existing Rust installer workflow.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    borea_gui::live::desktop::run(true)
}
