use image::{
    ExtendedColorType,
    codecs::ico::{IcoEncoder, IcoFrame},
    imageops::FilterType,
};
use std::{env, fs::File, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=VERSION");
    let release = std::fs::read_to_string("VERSION").expect("release version");
    let release = release.trim();
    println!("cargo:rustc-env=HITDROL_DISPLAY_VERSION={release}");
    let title = format!("HITDROL {release}");
    println!("cargo:rerun-if-changed=assets/hitdrol-logo.png");
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    for (file, width, height) in [
        ("ice-background.png", 1280, 800),
        ("ice-sword.png", 512, 768),
        ("ice-wordmark.png", 1536, 640),
    ] {
        println!("cargo:rerun-if-changed=assets/intro/{file}");
        image::open(format!("assets/intro/{file}"))
            .expect("HITDROL intro asset")
            .resize(width, height, FilterType::Lanczos3)
            .save(out.join(file))
            .expect("HITDROL intro texture");
    }
    let logo = image::open("assets/hitdrol-logo.png").expect("HITDROL logo");
    logo.resize(512, 512, FilterType::Lanczos3)
        .save(out.join("hitdrol-logo.png"))
        .expect("HITDROL UI logo");
    let frames: Vec<_> = [16, 24, 32, 48, 64, 128, 256]
        .into_iter()
        .map(|size| {
            let rgba = logo
                .resize_exact(size, size, FilterType::Lanczos3)
                .to_rgba8();
            IcoFrame::as_png(rgba.as_raw(), size, size, ExtendedColorType::Rgba8)
                .expect("HITDROL icon size")
        })
        .collect();
    let icon = out.join("hitdrol.ico");
    IcoEncoder::new(File::create(&icon).expect("HITDROL icon file"))
        .encode_images(&frames)
        .expect("HITDROL icon");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon(icon.to_str().expect("icon path"))
            .set("ProductName", &title)
            .set("ProductVersion", release)
            .set("FileDescription", &title)
            .set("InternalName", "hitdrol-launcher")
            .set("OriginalFilename", "HITDROL.exe")
            .set("LegalCopyright", "Copyright © 2026 HITDROL contributors")
            .compile()
            .expect("HITDROL Windows resources");
    }
}
