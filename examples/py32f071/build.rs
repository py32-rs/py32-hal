fn main() {
    // `--nmagic` is required because the memory sections are not aligned to
    // 0x10000 boundaries.
    println!("cargo:rustc-link-arg=--nmagic");
    println!("cargo:rustc-link-arg=-Tlink.x");
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
}
