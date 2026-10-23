fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        .with_bundled_translations("lang")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app.slint", config).expect("failed to compile the Slint UI");

    println!("cargo:rerun-if-changed=lang");
    println!("cargo:rerun-if-changed=assets/favicon.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/favicon.ico")
            .set_manifest_file("assets/app.manifest")
            .set("ProductName", "AlwaysEnglish")
            .set("FileDescription", "AlwaysEnglish - keep apps on the English input method")
            .set("LegalCopyright", "Apache-2.0. Based on ChaXxl/AlwaysEnglish.");
        res.compile().expect("failed to embed Windows resources");
    }
}
