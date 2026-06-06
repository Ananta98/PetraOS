fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if target_os == "none" {
        let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
        // Tell cargo to pass the linker script to the linker..
        println!("cargo:rustc-link-arg=-Tlinker-{arch}.ld");
        // ..and to re-run if it changes.
        println!("cargo:rerun-if-changed=linker-{arch}.ld");
    }
}
