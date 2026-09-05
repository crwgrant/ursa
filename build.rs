fn main() {
    println!("cargo:rerun-if-changed=packaging/AppIcon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("windows") {
        return;
    }

    let mut res = winresource::WindowsResource::new();
    res.set_icon("packaging/AppIcon.ico");
    res.set("ProductName", "Ursa");
    res.set("FileDescription", "Ursa");
    res.compile().expect("embed Windows icon");
}
