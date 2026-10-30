fn main() {
    println!("cargo::rerun-if-changed=../../packaging/windows/CloakFrame.manifest");
    println!("cargo::rerun-if-changed=../../assets/icons/cloakframe.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("../../assets/icons/cloakframe.ico")
        .set_manifest_file("../../packaging/windows/CloakFrame.manifest")
        .set("ProductName", "CloakFrame")
        .set("FileDescription", "CloakFrame");
    if let Err(e) = resource.compile() {
        panic!("cannot compile the Windows resources: {e}");
    }
}
