//! A short, deterministic GPU composition. No video player, sleeps or background
//! animation thread; all repaint requests pass through the launcher's frame gate.
use egui::{
    Color32, Context, Id, LayerId, Mesh, Order, Painter, Pos2, Rect, Shape, Stroke, TextureHandle,
    Vec2, pos2, vec2,
};
use std::time::{Duration, Instant};

pub const DURATION: f32 = 2.5;
pub fn dismissal(events: &[egui::Event]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            egui::Event::PointerButton { pressed: true, .. }
                | egui::Event::Touch {
                    phase: egui::TouchPhase::Start,
                    ..
                }
                | egui::Event::Key {
                    pressed: true,
                    repeat: false,
                    key: egui::Key::Enter | egui::Key::Space | egui::Key::Escape,
                    ..
                }
        )
    })
}
const IMPACT: f32 = 1.10;
const UV: Rect = Rect::from_min_max(Pos2::ZERO, pos2(1., 1.));

fn smooth(start: f32, end: f32, time: f32) -> f32 {
    let x = ((time - start) / (end - start)).clamp(0., 1.);
    x * x * (3. - 2. * x)
}
fn alpha(value: f32) -> Color32 {
    Color32::from_white_alpha((value.clamp(0., 1.) * 255.) as u8)
}
fn cyan(a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(66, 196, 255, (a.clamp(0., 1.) * 255.) as u8)
}
fn seed(i: u32) -> f32 {
    let mut n = i.wrapping_mul(747796405).wrapping_add(2891336453);
    n = ((n >> (n >> 28).wrapping_add(4)) ^ n).wrapping_mul(277803737);
    ((n >> 22) ^ n) as f32 / u32::MAX as f32
}

#[derive(Clone, Copy, Debug)]
pub struct Scene {
    pub time: f32,
    pub cover: f32,
    pub floor: f32,
    pub word: f32,
    pub drop: f32,
    pub cut: f32,
    pub burst: f32,
}
impl Scene {
    pub fn at(time: f32) -> Self {
        let time = if time.is_finite() {
            time.max(0.)
        } else {
            DURATION
        };
        let drop = ((time - 0.38) / (IMPACT - 0.38)).clamp(0., 1.);
        Self {
            time,
            cover: 1. - smooth(2.10, DURATION, time),
            // Keep the initial paused frame recognizable while Windows grants focus.
            floor: 0.16 + 0.84 * smooth(0., 0.24, time),
            word: smooth(0.12, 0.47, time),
            drop: drop * drop * drop,
            cut: smooth(0.99, 1.20, time),
            burst: (time - IMPACT).max(0.),
        }
    }
}
#[derive(Default)]
struct Clock {
    elapsed: Duration,
    previous: Option<Instant>,
    active: bool,
}
impl Clock {
    fn started(&self) -> bool {
        self.previous.is_some()
    }
    fn tick(&mut self, now: Instant, active: bool) -> f32 {
        if let Some(previous) = self.previous
            && self.active
        {
            self.elapsed += now.saturating_duration_since(previous);
        }
        self.previous = Some(now);
        self.active = active;
        self.elapsed.as_secs_f32()
    }
}
struct Assets {
    floor: TextureHandle,
    sword: TextureHandle,
    word: TextureHandle,
}
fn texture(ctx: &Context, name: &str, bytes: &[u8]) -> TextureHandle {
    let image = image::load_from_memory(bytes)
        .expect("embedded startup image")
        .to_rgba8();
    ctx.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    )
}
impl Assets {
    fn new(ctx: &Context) -> Self {
        Self {
            floor: texture(
                ctx,
                "intro-floor",
                include_bytes!(concat!(env!("OUT_DIR"), "/ice-background.png")),
            ),
            sword: texture(
                ctx,
                "intro-sword",
                include_bytes!(concat!(env!("OUT_DIR"), "/ice-sword.png")),
            ),
            word: texture(
                ctx,
                "intro-wordmark",
                include_bytes!(concat!(env!("OUT_DIR"), "/ice-wordmark.png")),
            ),
        }
    }
}
pub struct Intro {
    clock: Clock,
    assets: Option<Assets>,
    fixed: Option<f32>,
    pub elapsed: f32,
    sound_enabled: bool,
    sound: Option<crate::startup_audio::Playback>,
}
impl Intro {
    pub fn new(fixed: Option<f32>) -> Self {
        Self {
            clock: Clock::default(),
            assets: None,
            fixed: fixed
                .filter(|n| n.is_finite())
                .map(|n| n.clamp(0., DURATION - 0.001)),
            elapsed: 0.,
            sound_enabled: false,
            sound: None,
        }
    }
    pub fn set_active(&mut self, now: Instant, active: bool) {
        // Asset decoding and the first GPU submission must not consume the intro.
        if self.clock.started() {
            self.elapsed = self.clock.tick(now, active);
            if let Some(sound) = &mut self.sound {
                sound.set_running(active);
            }
        }
    }
    /// Returns false after the final frame, allowing the owner to drop all textures.
    pub fn paint(&mut self, ctx: &Context, now: Instant, active: bool) -> bool {
        if self.assets.is_none() {
            self.assets = Some(Assets::new(ctx));
        }
        if self.clock.started() {
            self.elapsed = self.clock.tick(now, active);
        }
        if let Some(sound) = &mut self.sound {
            sound.set_running(active);
        }
        let time = self.fixed.unwrap_or(self.elapsed);
        if time >= DURATION {
            return false;
        }
        paint(
            ctx,
            ctx.viewport_rect(),
            Scene::at(time),
            self.assets.as_ref().unwrap(),
        );
        if active && self.fixed.is_none() {
            ctx.request_repaint();
        }
        true
    }
    /// Start time and audio only after a frame has actually reached the surface.
    pub fn presented(&mut self, now: Instant, active: bool) {
        if !self.clock.started() {
            self.clock.tick(now, active);
            if self.sound_enabled && self.fixed.is_none() {
                self.sound = crate::startup_audio::Playback::open();
                if let Some(sound) = &mut self.sound {
                    sound.set_running(active);
                }
            }
        }
    }
    pub fn covers_library(&self) -> bool {
        self.fixed.unwrap_or(self.elapsed) < 2.10
    }
    /// Deterministic capture only; the normal startup uses the active clock.
    pub fn seek(&mut self, time: f32) {
        self.fixed = Some(time.max(0.));
    }
    pub fn with_sound(mut self, enabled: bool) -> Self {
        self.sound_enabled = enabled;
        self
    }
}
fn glow(p: &Painter, center: Pos2, radius: Vec2, color: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(center, color);
    for i in 0..=32 {
        let a = i as f32 * std::f32::consts::TAU / 32.;
        mesh.colored_vertex(
            center + vec2(a.cos() * radius.x, a.sin() * radius.y),
            Color32::TRANSPARENT,
        );
        if i > 0 {
            mesh.add_triangle(0, i, i + 1);
        }
    }
    p.add(mesh);
}
fn line(p: &Painter, points: Vec<Pos2>, width: f32, opacity: f32) {
    p.add(Shape::line(
        points.clone(),
        Stroke::new(width * 5., cyan(opacity * 0.07)),
    ));
    p.add(Shape::line(
        points.clone(),
        Stroke::new(width * 2., cyan(opacity * 0.23)),
    ));
    p.add(Shape::line(
        points,
        Stroke::new(
            width,
            Color32::from_rgba_unmultiplied(190, 241, 255, (opacity.clamp(0., 1.) * 255.) as u8),
        ),
    ));
}
fn paint(ctx: &Context, rect: Rect, s: Scene, a: &Assets) {
    let mut p = ctx
        .layer_painter(LayerId::new(Order::Tooltip, Id::new("hitdrol-startup")))
        .with_clip_rect(rect);
    p.set_opacity(s.cover);
    p.rect_filled(rect, 0, Color32::BLACK);
    p.image(a.floor.id(), rect, UV, alpha(s.floor));
    let h = rect.height();
    let w = rect.width();
    let origin = pos2(rect.center().x, rect.top() + h * 0.842);
    let scale = h / 800.;
    let since = s.time - IMPACT;
    let shake = if since >= 0. {
        (since * 53.).sin() * (-since * 12.).exp() * 3. * scale
    } else {
        0.
    };
    let settle = if since >= 0. {
        (since * 19.).sin() * (-since * 14.).exp() * 5. * scale
    } else {
        0.
    };
    let sword_h = h * 0.81;
    let sword_size = vec2(sword_h * a.sword.aspect_ratio(), sword_h);
    let offset = -h * 1.13 * (1. - s.drop) + settle;
    let sword = Rect::from_min_size(
        pos2(origin.x - sword_size.x * 0.5, origin.y - sword_h + offset),
        sword_size,
    );
    let sword_alpha = smooth(0.30, 0.44, s.time);
    let word_w = (w * 0.66).min(h * 1.08);
    let word_size = vec2(word_w, word_w / a.word.aspect_ratio());
    let word = Rect::from_center_size(
        pos2(origin.x, rect.top() + h * 0.60) + vec2(shake, shake * 0.35),
        word_size * (0.97 + 0.03 * s.word),
    );
    glow(
        &p,
        origin,
        vec2(h * 0.44, h * 0.055),
        cyan((0.14 + 0.35 * s.cut) * s.floor),
    );
    // The reflection is clipped to the ice plane and fades towards the viewer.
    if s.drop > 0.96 {
        let reflection = Rect::from_min_size(pos2(sword.left(), origin.y), sword_size);
        let reflection_p =
            p.with_clip_rect(Rect::from_min_max(pos2(rect.left(), origin.y), rect.max));
        reflection_p.image(
            a.sword.id(),
            reflection,
            Rect::from_min_max(pos2(0., 1.), pos2(1., 0.)),
            alpha(0.11 * sword_alpha),
        );
    }
    if s.time > IMPACT {
        cracks(&p, origin, h, s.burst);
    }
    if s.drop < 1. && sword_alpha > 0. {
        for i in 1..=3 {
            let tail = sword.translate(vec2(0., -(i as f32) * 13. * scale));
            p.image(a.sword.id(), tail, UV, alpha(0.045 * sword_alpha));
        }
    }
    p.image(a.sword.id(), sword, UV, alpha(sword_alpha));
    // Split by the strike, just a few pixels: retain the whole readable wordmark.
    let split = s.cut * 2.2 * scale;
    for (left, uv, shift) in [
        (
            true,
            Rect::from_min_max(pos2(0., 0.), pos2(0.5, 1.)),
            -split,
        ),
        (
            false,
            Rect::from_min_max(pos2(0.5, 0.), pos2(1., 1.)),
            split,
        ),
    ] {
        let half = if left {
            Rect::from_min_max(word.min, pos2(word.center().x, word.bottom()))
        } else {
            Rect::from_min_max(pos2(word.center().x, word.top()), word.max)
        };
        p.image(
            a.word.id(),
            half.translate(vec2(shift, 0.)),
            uv,
            alpha(s.word),
        );
    }
    if s.cut > 0. {
        let top = pos2(
            word.center().x - 2. * scale,
            word.top() + word.height() * 0.25,
        );
        let bottom = pos2(
            word.center().x + 3. * scale,
            word.top() + word.height() * (0.25 + 0.47 * s.cut),
        );
        let slit = Rect::from_min_max(top - vec2(2.5 * scale, 0.), bottom + vec2(2.5 * scale, 0.));
        p.with_clip_rect(slit)
            .image(a.sword.id(), sword, UV, alpha(0.85 * s.cut));
        line(
            &p,
            vec![
                top,
                top.lerp(bottom, 0.35) + vec2(-3. * scale, 0.),
                top.lerp(bottom, 0.65) + vec2(2. * scale, 0.),
                bottom,
            ],
            0.85 * scale,
            0.8 * s.cut,
        );
        for direction in [-1., 1.] {
            let center = top.lerp(bottom, 0.45);
            line(
                &p,
                vec![
                    center,
                    center + vec2(10. * direction, -5.) * scale,
                    center + vec2(21. * direction, 2.) * scale,
                    center + vec2(29. * direction, -2.) * scale,
                ],
                0.65 * scale,
                0.65 * s.cut,
            );
        }
        let glint = 1. - smooth(0., 0.40, (s.time - 1.02).max(0.));
        glow(&p, bottom, vec2(35., 25.) * scale, cyan(glint * 0.8));
    }
    if since >= 0. {
        let flash = (-since * 13.).exp();
        glow(
            &p,
            origin,
            vec2(95., 29.) * scale,
            Color32::from_rgba_unmultiplied(170, 237, 255, (flash * 190.) as u8),
        );
        shards(&p, origin, word.center(), h, since);
    }
}
fn cracks(p: &Painter, origin: Pos2, h: f32, time: f32) {
    let growth = smooth(0., 0.26, time);
    for i in 0..13 {
        let angle = seed(i + 11) * std::f32::consts::TAU;
        let distance = h * (0.10 + seed(i + 28) * 0.45) * growth;
        let delta = vec2(angle.cos() * distance, angle.sin() * distance * 0.16);
        let points = vec![
            origin,
            origin + delta * 0.26 + vec2(0., seed(i + 42) * 6. - 3.),
            origin + delta * 0.58 + vec2(seed(i + 61) * 12. - 6., 0.),
            origin + delta,
        ];
        line(
            p,
            points,
            0.7 * (h / 800.),
            (1. - smooth(0.4, 1.2, time)) * 0.8,
        );
    }
}
fn shards(p: &Painter, floor: Pos2, word: Pos2, h: f32, time: f32) {
    let scale = h / 800.;
    for i in 0..44 {
        let r = seed(i + 94);
        let side = seed(i + 153) * 2. - 1.;
        let life = 0.60 + seed(i + 200) * 0.7;
        if time >= life {
            continue;
        }
        let opacity = 1. - smooth(life * 0.55, life, time);
        let start = if i < 32 { floor } else { word };
        let velocity = vec2(side * (160. + r * 480.), -130. - seed(i + 301) * 410.) * scale;
        let center = start + velocity * time + vec2(0., 350. * time * time * scale);
        let size = (3. + seed(i + 371) * 12.) * scale;
        let angle = seed(i + 440) * std::f32::consts::TAU + time * side * 7.;
        let v = vec2(angle.cos(), angle.sin());
        let normal = vec2(-v.y, v.x);
        let tip = center + v * size;
        let back = center - v * size * 0.65;
        let c = Color32::from_rgba_unmultiplied(71, 184, 250, (opacity * 240.) as u8);
        p.add(Shape::convex_polygon(
            vec![
                tip,
                center + normal * size * 0.35,
                back,
                center - normal * size * 0.27,
            ],
            c,
            Stroke::new(
                0.65 * scale,
                Color32::from_white_alpha((opacity * 210.) as u8),
            ),
        ));
        p.add(Shape::convex_polygon(
            vec![tip, center + normal * size * 0.35, back],
            Color32::from_rgba_unmultiplied(174, 235, 255, (opacity * 155.) as u8),
            Stroke::NONE,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_gpu_submission_cannot_consume_the_intro_clock() {
        let ctx = Context::default();
        let mut intro = Intro::new(None);
        let now = Instant::now();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280., 800.))),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            assert!(intro.paint(ctx, now, true));
        });
        // Covers decoding/shader compilation, surface retries and delayed focus.
        intro.set_active(now + Duration::from_secs(10), true);
        assert_eq!(intro.elapsed, 0.);
        intro.presented(now + Duration::from_secs(10), true);
        intro.set_active(now + Duration::from_millis(10500), true);
        assert!((intro.elapsed - 0.5).abs() < 0.001);
        intro.presented(now + Duration::from_secs(11), true);
        intro.set_active(now + Duration::from_secs(11), false);
        assert!((intro.elapsed - 1.).abs() < 0.001);
        intro.set_active(now + Duration::from_secs(30), false);
        assert!((intro.elapsed - 1.).abs() < 0.001);
        assert!(Scene::at(0.).floor > 0.);
        assert!(intro.covers_library());
    }
    #[test]
    fn ending_intro_releases_all_three_textures() {
        let ctx = Context::default();
        let mut intro = Intro::new(Some(1.35));
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280., 800.))),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            assert!(intro.paint(ctx, Instant::now(), true));
        });
        let during = ctx.tex_manager().read().num_allocated();
        assert!(during >= 3);
        drop(intro);
        assert_eq!(ctx.tex_manager().read().num_allocated(), during - 3);
    }
    #[test]
    fn only_deliberate_press_dismisses_and_never_pointer_motion() {
        assert!(!dismissal(&[egui::Event::PointerMoved(pos2(30., 40.))]));
        let press = egui::Event::PointerButton {
            pos: pos2(30., 40.),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        };
        assert!(dismissal(&[press]));
        assert!(!dismissal(&[egui::Event::PointerButton {
            pos: pos2(30., 40.),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default()
        }]));
    }
    #[test]
    fn timeline_orders_drop_cut_and_exact_fade_end() {
        assert_eq!(Scene::at(0.).drop, 0.);
        assert_eq!(Scene::at(0.).word, 0.);
        assert!(Scene::at(0.8).drop > 0. && Scene::at(0.8).drop < 1.);
        assert_eq!(Scene::at(0.8).cut, 0.);
        assert_eq!(Scene::at(1.3).drop, 1.);
        assert_eq!(Scene::at(1.3).cut, 1.);
        assert_eq!(Scene::at(2.0).cover, 1.);
        assert_eq!(Scene::at(DURATION).cover, 0.);
        assert!(Scene::at(2.3).cover > 0. && Scene::at(2.3).cover < 1.);
    }
    #[test]
    fn inactive_time_does_not_advance_intro() {
        let now = Instant::now();
        let mut clock = Clock::default();
        clock.tick(now, true);
        clock.tick(now + Duration::from_millis(600), false);
        assert_eq!(clock.tick(now + Duration::from_secs(20), false), 0.6);
        clock.tick(now + Duration::from_secs(21), true);
        assert!((clock.tick(now + Duration::from_millis(21800), true) - 1.4).abs() < 0.001);
    }
    #[test]
    fn invalid_preview_times_are_bounded() {
        assert_eq!(Scene::at(f32::NAN).cover, 0.);
        assert_eq!(Scene::at(-50.).time, 0.);
        assert!(Intro::new(Some(100.)).fixed.unwrap() < DURATION);
    }
}
