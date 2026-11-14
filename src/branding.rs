use std::sync::LazyLock;

pub const NAME: &str = "HITDROL";
pub const RELEASE: &str = env!("HITDROL_DISPLAY_VERSION");
pub const TITLE: &str = concat!("HITDROL ", env!("HITDROL_DISPLAY_VERSION"));
pub const PROFILE: &str = "HITDROLLauncher";
pub const APP_ID: &str = "HITDROL.Launcher";
pub const LOGO: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/hitdrol-logo.png"));

static PIXELS: LazyLock<image::RgbaImage> = LazyLock::new(|| {
    image::load_from_memory(LOGO)
        .expect("embedded HITDROL logo")
        .to_rgba8()
});

pub fn texture(ctx: &egui::Context) -> egui::TextureHandle {
    ctx.load_texture(
        "hitdrol-logo",
        egui::ColorImage::from_rgba_unmultiplied(
            [PIXELS.width() as usize, PIXELS.height() as usize],
            PIXELS.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    )
}

pub fn window_icon() -> winit::window::Icon {
    winit::window::Icon::from_rgba(PIXELS.as_raw().clone(), PIXELS.width(), PIXELS.height())
        .expect("embedded HITDROL window icon")
}
