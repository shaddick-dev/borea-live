fn main() {
    slint_build::compile_with_config(
        "ui/borea.slint",
        slint_build::CompilerConfiguration::new().with_style("material".into()),
    )
    .expect("compile Borea's Slint interface");
}
