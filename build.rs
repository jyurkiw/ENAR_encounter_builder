//! Embeds the program icon (assets/icon.ico) in the Windows .exe, so Explorer,
//! the taskbar and shortcuts show it. The window's own icon is set at run time
//! from assets/icon.png (see main.rs).

fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/app.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("couldn't embed the icon (assets/app.rc)");
    }
}
