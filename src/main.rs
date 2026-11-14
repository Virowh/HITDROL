#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use anyhow::Result;
use hitdrol_launcher::{
    input::{Action, Controller, InputMethod},
    intro::Intro,
    pacing::FramePacer,
    platform,
    storage::Database,
    ui::UiApp,
};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Fullscreen, Window, WindowId},
};

#[derive(Clone, Debug)]
enum Wake {
    Repaint(Duration),
}
#[derive(Default)]
struct Options {
    demo: usize,
    windowed: bool,
    size: Option<(u32, u32)>,
    scale: Option<f32>,
    fps: Option<u32>,
    data: Option<PathBuf>,
    screenshot: Option<PathBuf>,
    quit_after: Option<f64>,
    page: Option<String>,
    benchmark: bool,
    intro: bool,
    no_intro: bool,
    intro_at: Option<f32>,
    screenshot_after: Option<f64>,
    record_intro: Option<PathBuf>,
    clear_store_cache: bool,
    set_autostart: Option<bool>,
    autostart: bool,
    early_autostart: bool,
    startup_delay: Option<f64>,
}
impl Options {
    fn startup_enabled(&self, settings: &hitdrol_launcher::model::Settings) -> bool {
        !self.no_intro
            && (self.intro
                || self.record_intro.is_some()
                || self.intro_at.is_some()
                || (self.page.is_none() && settings.startup_animation && !settings.reduced_motion))
    }
    fn parse() -> Self {
        let mut result = Self::default();
        let mut args = std::env::args().skip(1).peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--demo" => {
                    result.demo = if args.peek().is_some_and(|s| s.parse::<usize>().is_ok()) {
                        args.next().and_then(|s| s.parse().ok()).unwrap_or(12)
                    } else {
                        12
                    };
                }
                "--windowed" => result.windowed = true,
                "--autostart" => result.autostart = true,
                "--early-autostart" => {
                    result.autostart = true;
                    result.early_autostart = true;
                }
                "--startup-delay" => {
                    result.startup_delay = args
                        .next()
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|s| s.is_finite() && (0.0..=30.0).contains(s))
                }
                "--clear-store-cache" => result.clear_store_cache = true,
                "--enable-autostart" => result.set_autostart = Some(true),
                "--disable-autostart" => result.set_autostart = Some(false),
                "--size" => {
                    result.size = args.next().and_then(|s| {
                        s.split_once('x')
                            .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                    });
                }
                "--scale" => result.scale = args.next().and_then(|s| s.parse().ok()),
                "--fps" => result.fps = args.next().and_then(|s| s.parse().ok()),
                "--data-dir" => result.data = args.next().map(PathBuf::from),
                "--screenshot" => result.screenshot = args.next().map(PathBuf::from),
                "--quit-after" => result.quit_after = args.next().and_then(|s| s.parse().ok()),
                "--page" => result.page = args.next(),
                "--benchmark" => result.benchmark = true,
                "--intro" => result.intro = true,
                "--record-intro" => result.record_intro = args.next().map(PathBuf::from),
                "--no-intro" => result.no_intro = true,
                "--intro-at" => {
                    result.intro_at = args
                        .next()
                        .and_then(|s| s.parse::<f32>().ok())
                        .filter(|n| n.is_finite())
                }
                "--screenshot-after" => {
                    result.screenshot_after = args
                        .next()
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|n| n.is_finite() && *n >= 0.)
                }
                _ => {}
            }
        }
        result
    }
}
struct Graphics {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: egui_wgpu::Renderer,
    adapter_name: String,
}
impl Graphics {
    async fn new(window: Arc<Window>) -> Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("HITDROL GPU"),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                ..Default::default()
            })
            .await?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | if caps.usages.contains(wgpu::TextureUsages::COPY_SRC) {
                    wgpu::TextureUsages::COPY_SRC
                } else {
                    wgpu::TextureUsages::empty()
                },
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 1,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        let renderer =
            egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
        Ok(Self {
            surface,
            device,
            queue,
            config,
            renderer,
            adapter_name: adapter.get_info().name,
        })
    }
    fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        if size.width > 0 && size.height > 0 {
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }
    fn render(
        &mut self,
        ctx: &egui::Context,
        output: egui::FullOutput,
        capture: Option<&PathBuf>,
    ) -> Result<bool> {
        // Texture deltas must survive a temporary surface loss; egui sends them only once.
        for (id, delta) in &output.textures_delta.set {
            self.renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return Ok(false);
            }
            Err(wgpu::SurfaceError::Timeout) => return Ok(false),
            Err(e) => return Err(e.into()),
        };
        let target = &frame.texture;
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: output.pixels_per_point,
        };
        let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let buffers =
            self.renderer
                .update_buffers(&self.device, &self.queue, &mut encoder, &jobs, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("HITDROL UI"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.047,
                                g: 0.067,
                                b: 0.094,
                                a: 1.,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
            self.renderer.render(&mut pass, &jobs, &screen);
        }
        let row_bytes = (self.config.width * 4).div_ceil(256) * 256;
        let readback = if capture.is_some() {
            anyhow::ensure!(
                self.config.usage.contains(wgpu::TextureUsages::COPY_SRC),
                "GPU не поддерживает захват кадра"
            );
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Screenshot"),
                size: row_bytes as u64 * self.config.height as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: target,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row_bytes),
                        rows_per_image: Some(self.config.height),
                    },
                },
                wgpu::Extent3d {
                    width: self.config.width,
                    height: self.config.height,
                    depth_or_array_layers: 1,
                },
            );
            Some(buffer)
        } else {
            None
        };
        self.queue
            .submit(buffers.into_iter().chain([encoder.finish()]));
        frame.present();
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
        if let (Some(buffer), Some(path)) = (readback, capture) {
            let (tx, rx) = std::sync::mpsc::channel();
            buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            self.device.poll(wgpu::PollType::wait_indefinitely())?;
            rx.recv_timeout(Duration::from_secs(10))??;
            let mapped = buffer.slice(..).get_mapped_range();
            let mut pixels =
                Vec::with_capacity((self.config.width * self.config.height * 4) as usize);
            for row in mapped.chunks(row_bytes as usize) {
                pixels.extend_from_slice(&row[..(self.config.width * 4) as usize]);
            }
            if matches!(
                self.config.format,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
            ) {
                for pixel in pixels.as_chunks_mut::<4>().0 {
                    pixel.swap(0, 2);
                }
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            image::save_buffer(
                path,
                &pixels,
                self.config.width,
                self.config.height,
                image::ColorType::Rgba8,
            )?;
            drop(mapped);
            buffer.unmap();
        }
        Ok(true)
    }
}
struct Runtime {
    options: Options,
    ui: UiApp,
    ctx: egui::Context,
    window: Option<Arc<Window>>,
    integration: Option<egui_winit::State>,
    graphics: Option<Graphics>,
    pacer: FramePacer,
    controller: Controller,
    next_poll: Instant,
    started: Instant,
    frames: u64,
    first_frame: Option<f64>,
    last_frame: Option<Instant>,
    intervals: Vec<f64>,
    capture_done: bool,
    occluded: bool,
    fullscreen: bool,
    failure: Option<String>,
    last_touch: Option<Instant>,
    intro: Option<Intro>,
    intro_enabled: bool,
    intro_duration: Option<f32>,
    intro_skipped: bool,
    intro_foreground_denied: bool,
    capture_after: f64,
    recorded_frames: u32,
    consume_intro_input: bool,
    window_shown: bool,
    session: hitdrol_launcher::session::StartupGate,
    next_session_check: Instant,
    session_wait_seconds: f64,
}
impl Runtime {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl std::fmt::Display) {
        self.failure = Some(error.to_string());
        event_loop.exit();
    }
    fn draw(&mut self, event_loop: &ActiveEventLoop) {
        self.draw_frame(event_loop);
    }
    fn refresh_session(&mut self, now: Instant) {
        // Diagnostic delay only defers readiness in an isolated demo profile.
        let delay = if self.options.demo > 0 && self.options.data.is_some() {
            self.options.startup_delay.unwrap_or(0.)
        } else {
            0.
        };
        let ready = self.started.elapsed().as_secs_f64() >= delay
            && hitdrol_launcher::session::interactive();
        if self.session.update(now, ready) {
            self.pacer.request();
        }
        self.next_session_check = now
            + if self.window_shown {
                hitdrol_launcher::session::POLL_INTERVAL
            } else {
                hitdrol_launcher::session::STARTUP_POLL_INTERVAL
            };
    }
    fn draw_frame(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if !self.window_shown {
            self.refresh_session(now);
        }
        if !self.session.interactive {
            if let Some(intro) = &mut self.intro {
                intro.set_active(now, false);
            }
            self.pacer.visible = false;
            return;
        }
        if !self.pacer.ready(now) {
            return;
        }
        let Some(window) = &self.window else {
            return;
        };
        let Some(integration) = &mut self.integration else {
            return;
        };
        let mut input = integration.take_egui_input(window);
        // A dismissal gesture is consumed here, never delivered to a game underneath.
        if self.intro.is_some() || std::mem::take(&mut self.consume_intro_input) {
            let dismiss = hitdrol_launcher::intro::dismissal(&input.events);
            input.events.clear();
            if dismiss {
                self.intro = None;
                self.intro_skipped = true;
                self.controller.reset();
            }
        }
        input.time = Some(self.started.elapsed().as_secs_f64());
        if self.options.record_intro.is_some()
            && let Some(intro) = &mut self.intro
        {
            intro.seek(self.recorded_frames as f32 / 30.);
        }
        // Keep handheld layouts usable even at a large Windows DPI setting.
        let physical = window.inner_size();
        let native = window.scale_factor() as f32;
        let requested = self.options.scale.unwrap_or(self.ui.settings.scale);
        let scale = (native * requested)
            .min(physical.width as f32 / 760.)
            .min(physical.height as f32 / 540.)
            .max(0.5);
        self.ctx.set_pixels_per_point(scale);
        self.ctx.style_mut(|s| {
            s.animation_time = if !self.ui.focused || self.ui.settings.reduced_motion {
                0.
            } else {
                0.13
            }
        });
        let mut finished = false;
        let covered = self.intro.as_ref().is_some_and(Intro::covers_library);
        let output = self.ctx.run(input, |ctx| {
            // Library layout, fonts and covers are unnecessary under an opaque intro.
            if !covered {
                self.ui.frame(ctx);
            }
            if let Some(intro) = &mut self.intro {
                finished = !intro.paint(ctx, now, self.ui.focused && self.pacer.visible);
            }
        });
        if finished {
            self.intro_duration = self.intro.as_ref().map(|i| {
                if self.options.record_intro.is_some() {
                    hitdrol_launcher::intro::DURATION
                } else {
                    i.elapsed
                }
            });
            self.intro = None;
            self.controller.reset();
            self.ctx.request_repaint();
        }
        integration.handle_platform_output(window, output.platform_output.clone());
        if std::mem::take(&mut self.ui.paste_requested) {
            if let Some(text) = integration.clipboard_text() {
                self.ui.paste_text(text);
            }
            self.pacer.request();
        }
        self.pacer.limit = if self.ui.settings.display_rate {
            self.ui.refresh_hz.clamp(30, 240)
        } else {
            self.ui.settings.fps
        };
        let changed_fullscreen = self.ui.settings.fullscreen != self.fullscreen;
        if changed_fullscreen {
            self.fullscreen = self.ui.settings.fullscreen;
            window.set_fullscreen(if self.fullscreen {
                Some(Fullscreen::Borderless(window.current_monitor()))
            } else {
                None
            });
        }
        let record_capture = self
            .options
            .record_intro
            .as_ref()
            .map(|path| path.join(format!("{:04}.png", self.recorded_frames)));
        let capture = if let Some(path) = &record_capture {
            Some(path)
        } else if !self.capture_done && self.started.elapsed().as_secs_f64() > self.capture_after {
            self.options.screenshot.as_ref()
        } else {
            None
        };
        let delay = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.repaint_delay)
            .unwrap_or(Duration::MAX);
        let result = self
            .graphics
            .as_mut()
            .unwrap()
            .render(&self.ctx, output, capture);
        match result {
            Ok(true) => {
                if !self.window_shown {
                    window.set_visible(true);
                    let foreground = request_foreground(window);
                    if !foreground
                        && self.intro.is_some()
                        && self.options.intro_at.is_none()
                        && self.options.record_intro.is_none()
                    {
                        // A paused first frame is nearly black. Windows may
                        // legitimately deny foreground activation at logon;
                        // show the library instead of trapping it under an intro.
                        self.intro = None;
                        self.intro_skipped = true;
                        self.intro_foreground_denied = true;
                    }
                    self.window_shown = true;
                }
                if let Some(intro) = &mut self.intro {
                    intro.presented(Instant::now(), self.ui.focused && self.pacer.visible);
                }
                self.pacer.presented(now);
                if self.intro_foreground_denied && self.frames == 0 {
                    // The just-presented opaque intro must be replaced even
                    // when there are no input/background events to wake us.
                    self.pacer.request();
                }
                if self.ui.focused || self.intro.is_some() {
                    self.pacer.request_after(Instant::now(), delay);
                }
                self.frames += 1;
                if self.first_frame.is_none() {
                    self.first_frame = Some(self.started.elapsed().as_secs_f64());
                }
                if self.options.benchmark {
                    if let Some(last) = self.last_frame {
                        self.intervals
                            .push(now.duration_since(last).as_secs_f64() * 1000.);
                    }
                    self.last_frame = Some(now);
                    self.pacer.request();
                }
                if capture.is_some() {
                    self.capture_done = true;
                }
                if self.options.record_intro.is_some() {
                    self.recorded_frames += 1;
                    if self.recorded_frames >= 90 {
                        event_loop.exit();
                    } else {
                        self.ctx.request_repaint();
                    }
                }
            }
            Ok(false) => {
                // Some drivers cannot acquire a hidden swapchain. Show the window
                // on a retry, but keep the intro clock stopped until presentation.
                if !self.window_shown {
                    window.set_visible(true);
                    request_foreground(window);
                    self.window_shown = true;
                }
                self.pacer.request();
            }
            Err(e) => self.fail(event_loop, e),
        }
        if self.ui.exit {
            event_loop.exit();
        }
    }
    fn report(&self) {
        let report = serde_json::json!({"version":hitdrol_launcher::branding::RELEASE,"build_version":env!("CARGO_PKG_VERSION"),"autostart":self.options.autostart,"session_wait_seconds":self.session_wait_seconds,"graphics_before_session":false,"frames":self.frames,"elapsed_seconds":self.started.elapsed().as_secs_f64(),"first_frame_seconds":self.first_frame,"last_frame_seconds":self.last_frame.map(|i| i.duration_since(self.started).as_secs_f64()),"frame_intervals_ms":self.intervals,"adapter":self.graphics.as_ref().map(|g|&g.adapter_name),"size":self.graphics.as_ref().map(|g|[g.config.width,g.config.height]),"fps_limit":self.pacer.limit,"capture":self.capture_done,"error":self.failure,"intro_enabled":self.intro_enabled,"intro_duration_seconds":self.intro_duration,"intro_skipped":self.intro_skipped,"intro_foreground_denied":self.intro_foreground_denied});
        let _ = std::fs::write(
            self.ui.db.root.join("last-run.json"),
            serde_json::to_vec_pretty(&report).unwrap_or_default(),
        );
    }
}
impl ApplicationHandler<Wake> for Runtime {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let size = self.options.size.unwrap_or((1280, 800));
        let attributes = Window::default_attributes()
            .with_title(hitdrol_launcher::branding::TITLE)
            .with_window_icon(Some(hitdrol_launcher::branding::window_icon()))
            .with_inner_size(winit::dpi::PhysicalSize::new(size.0, size.1))
            .with_min_inner_size(winit::dpi::LogicalSize::new(760., 540.))
            .with_active(false)
            .with_visible(false);
        let window = match event_loop.create_window(attributes) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.fail(event_loop, e);
                return;
            }
        };
        self.ui.refresh_hz = window
            .current_monitor()
            .and_then(|m| m.refresh_rate_millihertz())
            .map(|n| n / 1000)
            .unwrap_or(60);
        self.ui.focused = window.has_focus();
        if self.fullscreen {
            window.set_fullscreen(Some(Fullscreen::Borderless(window.current_monitor())));
        }
        match pollster::block_on(Graphics::new(window.clone())) {
            Ok(g) => self.graphics = Some(g),
            Err(e) => {
                self.fail(event_loop, e);
                return;
            }
        }
        self.integration = Some(egui_winit::State::new(
            self.ctx.clone(),
            egui::ViewportId::ROOT,
            event_loop,
            Some(window.scale_factor() as f32),
            None,
            None,
        ));
        self.window = Some(window);
        self.pacer.request();
        // Windows suppresses WM_PAINT for hidden windows. Submit the warm-up
        // frame directly; draw() then shows the window (or does so on a retry).
        self.draw(event_loop);
    }
    fn user_event(&mut self, _: &ActiveEventLoop, event: Wake) {
        match event {
            Wake::Repaint(delay) => {
                if self.ui.focused || delay == Duration::ZERO {
                    self.pacer.request_after(Instant::now(), delay);
                }
            }
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let Some(w) = &self.window
            && let Some(s) = &mut self.integration
        {
            let response = s.on_window_event(w, &event);
            if response.repaint {
                self.pacer.request();
            }
        }
        // Windows may deliver a first press before egui knows the cursor position.
        // Handle it at the window boundary as well, and consume its queued egui events.
        if self.intro.is_some()
            && matches!(
                &event,
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    ..
                } | WindowEvent::Touch(winit::event::Touch {
                    phase: winit::event::TouchPhase::Started,
                    ..
                })
            )
        {
            self.intro = None;
            self.intro_skipped = true;
            self.consume_intro_input = true;
            self.controller.reset();
            self.pacer.request();
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.draw(event_loop),
            WindowEvent::Resized(size) => {
                self.pacer.visible = !self.occluded && size.width > 0 && size.height > 0;
                if let Some(g) = &mut self.graphics {
                    g.resize(size);
                }
                self.pacer.request();
            }
            WindowEvent::Occluded(value) => {
                self.occluded = value;
                self.pacer.visible = !value;
                if !value {
                    self.pacer.request();
                }
            }
            WindowEvent::Focused(focused) => {
                self.ui.focused = focused;
                self.controller.reset();
                if focused {
                    self.ui.gamepad_suspended = false;
                    self.ui.resumed_focus();
                    self.pacer.request();
                }
            }
            WindowEvent::Touch(_) => {
                self.ui.method = InputMethod::Touch;
                self.last_touch = Some(Instant::now());
            }
            WindowEvent::CursorMoved { .. } | WindowEvent::MouseInput { .. } => {
                if self
                    .last_touch
                    .is_none_or(|t| t.elapsed() > Duration::from_millis(300))
                {
                    self.ui.method = InputMethod::Mouse;
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.ui.method = InputMethod::Keyboard;
                if let Key::Named(code) = event.logical_key {
                    if code == NamedKey::F11 && !event.repeat {
                        self.ui.settings.fullscreen = !self.ui.settings.fullscreen;
                        let _ = self.ui.db.save_settings(&self.ui.settings);
                    } else if self.intro.is_some() {
                        if matches!(code, NamedKey::Escape | NamedKey::Enter | NamedKey::Space)
                            && !event.repeat
                        {
                            self.intro = None;
                            self.intro_skipped = true;
                            self.consume_intro_input = true;
                            self.controller.reset();
                        }
                    } else if code == NamedKey::Escape {
                        self.ui.action(Action::Back);
                    } else if !self.ctx.wants_keyboard_input() {
                        let action = match code {
                            NamedKey::ArrowLeft => Some(Action::Left),
                            NamedKey::ArrowRight => Some(Action::Right),
                            NamedKey::ArrowUp => Some(Action::Up),
                            NamedKey::ArrowDown => Some(Action::Down),
                            NamedKey::Enter | NamedKey::Space if !event.repeat => {
                                Some(Action::Accept)
                            }
                            NamedKey::PageUp => Some(Action::PreviousTab),
                            NamedKey::PageDown => Some(Action::NextTab),
                            NamedKey::ContextMenu => Some(Action::Menu),
                            _ => None,
                        };
                        if let Some(a) = action {
                            self.ui.action(a);
                        }
                    }
                }
                self.pacer.request();
            }
            WindowEvent::ScaleFactorChanged { .. } | WindowEvent::Moved(_) => {
                if let Some(w) = &self.window {
                    self.ui.refresh_hz = w
                        .current_monitor()
                        .and_then(|m| m.refresh_rate_millihertz())
                        .map(|n| n / 1000)
                        .unwrap_or(60);
                }
                self.pacer.request();
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_session_check {
            self.refresh_session(now);
        }
        if self
            .options
            .quit_after
            .is_some_and(|s| self.started.elapsed().as_secs_f64() >= s)
        {
            event_loop.exit();
            return;
        }
        if let Some(w) = &self.window {
            let minimized = w.is_minimized().unwrap_or(false);
            self.pacer.visible = self.session.interactive
                && !minimized
                && !self.occluded
                && w.inner_size().width > 0
                && w.inner_size().height > 0;
            if let Some(intro) = &mut self.intro {
                intro.set_active(now, self.pacer.visible && self.ui.focused);
            }
            self.ui
                .wallpaper_service
                .update(hitdrol_launcher::wallpaper::Request::for_state(
                    &self.ui.settings,
                    self.pacer.visible,
                    self.ui.focused,
                    self.ui.gamepad_suspended || self.intro.is_some(),
                    (w.inner_size().width, w.inner_size().height),
                    self.ui.wallpaper_revision,
                ));
        }
        if now >= self.next_poll {
            if self.ui.focused && self.pacer.visible && !self.ui.gamepad_suspended {
                let actions = self.controller.poll(now);
                let intro_was_active = self.intro.is_some();
                for action in actions {
                    self.ui.method = InputMethod::Gamepad;
                    if intro_was_active {
                        if matches!(action, Action::Accept | Action::Back) {
                            self.intro = None;
                            self.intro_skipped = true;
                            self.consume_intro_input = true;
                        }
                    } else {
                        self.ui.action(action);
                    }
                    self.pacer.request();
                }
                if intro_was_active && self.intro.is_none() {
                    self.controller.reset();
                }
            }
            self.next_poll = now
                + if self.ui.focused && self.controller.connected() {
                    Duration::from_millis(8)
                } else {
                    Duration::from_millis(500)
                };
        }
        if !self.capture_done
            && self.options.screenshot.is_some()
            && self.started.elapsed().as_secs_f64() > self.capture_after
        {
            self.pacer.request();
        }
        if self.pacer.ready(now) {
            if !self.window_shown {
                self.draw(event_loop);
            } else if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        let deadline = self
            .pacer
            .deadline(now)
            .unwrap_or(self.next_poll)
            .min(self.next_poll)
            .min(self.next_session_check);
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            deadline.max(now + Duration::from_micros(200)),
        ));
    }
    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.ui.exit = true;
        self.ui.flush_events();
        self.report();
    }
}
// Use the ordinary OS foreground request. Never call winit::focus_window:
// its Windows implementation injects Alt to bypass the foreground policy.
fn request_foreground(window: &Window) -> bool {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if let Ok(handle) = window.window_handle()
        && let RawWindowHandle::Win32(handle) = handle.as_raw()
    {
        unsafe {
            let hwnd = windows::Win32::Foundation::HWND(handle.hwnd.get() as *mut _);
            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd);
            return windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() == hwnd;
        }
    }
    false
}
fn run() -> Result<()> {
    let started = Instant::now();
    let options = Options::parse();
    if let Some(enabled) = options.set_autostart {
        hitdrol_launcher::autostart::set(enabled)?;
        return Ok(());
    }
    // Retired command must never initialize a UI, GPU or sound, even if an old
    // scheduled task could not be removed during an update.
    if options.early_autostart {
        return Ok(());
    }
    // Claim the profile before waiting, so an ordinary second launch cannot
    // overtake a deferred autostart and consume its intro or open a second UI.
    let root = match options.data.clone() {
        Some(root) => root,
        None => platform::profile_path()?,
    };
    let _instance = if options.clear_store_cache {
        None
    } else {
        let Some(instance) = hitdrol_launcher::startup_instance::claim(&root)? else {
            return Ok(());
        };
        Some(instance)
    };
    let mut startup = hitdrol_launcher::session::StartupGate::for_startup(options.autostart);
    let diagnostic_delay = if options.demo > 0 && options.data.is_some() {
        options.startup_delay.unwrap_or(0.)
    } else {
        0.
    };
    loop {
        startup.update(
            Instant::now(),
            started.elapsed().as_secs_f64() >= diagnostic_delay
                && hitdrol_launcher::session::interactive(),
        );
        if startup.interactive {
            break;
        }
        if options
            .quit_after
            .is_some_and(|s| started.elapsed().as_secs_f64() >= s)
            || (options.autostart && started.elapsed() >= Duration::from_secs(300))
        {
            return Ok(());
        }
        std::thread::sleep(hitdrol_launcher::session::POLL_INTERVAL);
    }
    let session_wait_seconds = started.elapsed().as_secs_f64();
    if options.data.is_none() {
        platform::data_dir()?;
    }
    let mut db = Database::open(&root)?;
    if options.clear_store_cache {
        let token_present =
            platform::library_token(hitdrol_launcher::model::Source::Epic)?.is_some();
        platform::save_library_token(hitdrol_launcher::model::Source::Epic, "")?;
        let removed = db.clear_store_cache(&[
            hitdrol_launcher::model::Source::Steam,
            hitdrol_launcher::model::Source::Epic,
        ])?;
        let mut settings = db.settings()?;
        settings.show_uninstalled = false;
        db.save_settings(&settings)?;
        let report = serde_json::json!({"steam_epic_uninstalled_cache_removed":removed,"epic_token_was_present":token_present,"epic_token_cleared":true,"steam_auth_tokens_stored_by_hitdrol":false,"installed_games_and_edits_preserved":true});
        std::fs::write(
            root.join("cache-cleanup.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        return Ok(());
    }
    let event_loop = EventLoop::<Wake>::with_user_event().build()?;
    let ctx = egui::Context::default();
    let proxy = event_loop.create_proxy();
    ctx.set_request_repaint_callback(move |info| {
        let _ = proxy.send_event(Wake::Repaint(info.delay));
    });
    let mut ui = UiApp::new(db, ctx.clone(), options.demo)?;
    if options.windowed {
        ui.settings.fullscreen = false;
    }
    if let Some(fps) = options.fps {
        ui.settings.fps = fps.clamp(30, 240);
        ui.settings.display_rate = false;
    }
    if let Some(page) = &options.page {
        ui.preview(page);
    }
    let fullscreen = ui.settings.fullscreen;
    let fps = ui.settings.fps;
    if let Some(path) = &options.record_intro {
        std::fs::create_dir_all(path)?;
    }
    let intro_enabled = options.startup_enabled(&ui.settings);
    let intro = if intro_enabled {
        Some(
            Intro::new(options.intro_at)
                .with_sound(ui.settings.startup_sound && options.record_intro.is_none()),
        )
    } else {
        None
    };
    let capture_after =
        options
            .screenshot_after
            .unwrap_or(if intro_enabled && options.intro_at.is_none() {
                4.0
            } else {
                1.8
            });
    let mut runtime = Runtime {
        options,
        ui,
        ctx,
        window: None,
        integration: None,
        graphics: None,
        pacer: FramePacer::new(fps),
        controller: Controller::default(),
        next_poll: Instant::now(),
        started,
        frames: 0,
        first_frame: None,
        last_frame: None,
        intervals: vec![],
        capture_done: false,
        occluded: false,
        fullscreen,
        failure: None,
        last_touch: None,
        intro,
        intro_enabled,
        intro_duration: None,
        intro_skipped: false,
        intro_foreground_denied: false,
        capture_after,
        recorded_frames: 0,
        consume_intro_input: false,
        window_shown: false,
        session: Default::default(),
        next_session_check: Instant::now(),
        session_wait_seconds,
    };
    event_loop.run_app(&mut runtime)?;
    if let Some(e) = runtime.failure {
        anyhow::bail!(e);
    }
    Ok(())
}
fn main() {
    unsafe {
        let id: Vec<u16> = hitdrol_launcher::branding::APP_ID
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let _ = windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(
            windows::core::PCWSTR(id.as_ptr()),
        );
    }
    if let Err(error) = run() {
        let message = format!("Не удалось открыть HITDROL:\n{error:#}");
        if let Ok(root) = platform::profile_path() {
            let _ = std::fs::create_dir_all(&root);
            let _ = std::fs::write(root.join("startup-error.txt"), &message);
        }
        eprintln!("{message}");
        // Automatic startup errors are recorded for later inspection. Never
        // raise an error dialog (or its system sound) over the sign-in screen.
        if std::env::args().any(|arg| arg == "--autostart" || arg == "--early-autostart") {
            return;
        }
        unsafe {
            use windows::{Win32::UI::WindowsAndMessaging::*, core::PCWSTR};
            let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
            let title: Vec<u16> = "HITDROL".encode_utf16().chain(Some(0)).collect();
            MessageBoxW(
                None,
                PCWSTR(message.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_respects_saved_preferences_and_diagnostic_pages() {
        let mut settings: hitdrol_launcher::model::Settings =
            serde_json::from_str(r#"{"fps":40}"#).unwrap();
        assert!(settings.startup_animation);
        let mut options = Options::default();
        assert!(options.startup_enabled(&settings));
        settings.reduced_motion = true;
        assert!(!options.startup_enabled(&settings));
        settings.reduced_motion = false;
        settings.startup_animation = false;
        assert!(!options.startup_enabled(&settings));
        options.intro = true;
        assert!(options.startup_enabled(&settings));
        options.no_intro = true;
        assert!(!options.startup_enabled(&settings));
        options = Options {
            page: Some("settings".into()),
            ..Default::default()
        };
        settings.startup_animation = true;
        assert!(!options.startup_enabled(&settings));
    }
}
