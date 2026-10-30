fn main() {
    slint_build::compile_with_config(
        "ui/borea.slint",
        // The built-in Material skin hard-codes violet even inside inputs.
        // Use the cool-toned stock controls beneath Borea's own Nord surfaces.
        slint_build::CompilerConfiguration::new().with_style("fluent".into()),
    )
    .expect("compile Borea's Slint interface");
}
