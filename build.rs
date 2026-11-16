fn main() {
    // KDE caches icons by name: a new design needs a new name, so the name carries a hash.
    let svg = std::fs::read("assets/gang-branch-2000.svg").expect("assets/gang-branch-2000.svg");
    let hash = svg.iter().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(*b)).wrapping_mul(0x0100_0193)
    });
    println!("cargo:rustc-env=ICON_NAME=gangbranch2000{hash:08x}");
    println!("cargo:rerun-if-changed=assets/gang-branch-2000.svg");

    // The icon and name Windows Explorer shows for the .exe.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/gangbranch2000.ico")
            .set("ProductName", "Gang Branch 2000")
            .set("FileDescription", "Gang Branch 2000");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icône Windows non intégrée : {e}");
        }
    }
    println!("cargo:rerun-if-changed=assets/icons/gangbranch2000.ico");
}
