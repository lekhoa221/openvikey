fn main() {
    println!("cargo:rerun-if-changed=assets/openvikey.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/openvikey.ico");
        resource
            .set(
                "FileDescription",
                "OpenViKey - Self-learning Vietnamese input method",
            )
            .set("ProductName", "OpenViKey")
            .set("OriginalFilename", "OpenViKey.exe")
            .set("LegalCopyright", "Copyright OpenViKey Authors")
            .compile()
            .expect("failed to compile OpenViKey Windows resources");
    }
}
