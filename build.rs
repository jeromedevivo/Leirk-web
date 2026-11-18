// Embeds the app icon and version info into leirk.exe.
fn main() {
    println!("cargo:rerun-if-changed=assets/leirk.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/leirk.ico").set("ProductName", "Leirk").set("FileDescription", "Leirk");
        res.compile().expect("Windows resources");
    }
}
