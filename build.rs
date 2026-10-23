// Windows: embed the app icon and version information in the .exe.
fn main() {
    println!("cargo:rerun-if-changed=assets/homeup.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/homeup.ico");
        res.set("ProductName", "HomeUp");
        res.set("FileDescription", "HomeUp — modelador 3D");
        res.set("LegalCopyright", "© 2026 Pedro Cordeiro Lima");
        if let Err(e) = res.compile() {
            println!("cargo:warning=ícone do executável não embutido: {e}");
        }
    }
}
