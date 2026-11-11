fn main() {
    println!("cargo:rerun-if-changed=native/raw_bridge.cpp");
    println!("cargo:rerun-if-changed=native/raw_bridge.h");
    println!("cargo:rerun-if-changed=native/libraw/libraw");
    println!("cargo:rerun-if-changed=native/jpeg_bridge.c");
    println!("cargo:rerun-if-changed=native/jpeg");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        cc::Build::new()
            .file("native/jpeg_bridge.c")
            .include("native/jpeg")
            .flag_if_supported("/utf-8")
            .compile("chromora_jpeg_bridge");
        println!("cargo:rustc-link-search=native={}/native/jpeg", std::env::var("CARGO_MANIFEST_DIR").unwrap());
        println!("cargo:rustc-link-lib=static=jpeg-static");
        cc::Build::new()
            .cpp(true)
            .file("native/raw_bridge.cpp")
            .include("native/libraw")
            .flag_if_supported("/std:c++17")
            .flag_if_supported("/utf-8")
            .compile("chromora_raw_bridge");
    }
}
