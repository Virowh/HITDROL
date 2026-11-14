//! A private Wallpaper Engine pop-out supplies frames to the normal egui renderer.
//! No desktop capture, global pause/play commands, or changes to WE's config.
use crate::{
    discovery::{self, Vdf},
    model::{BackgroundMode, Settings},
};
use anyhow::{Context, Result, ensure};
use std::{
    ffi::OsString,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{CloseHandle, HWND, LPARAM, WPARAM},
    System::{Diagnostics::ToolHelp::*, Threading::*},
    UI::WindowsAndMessaging::*,
};
use windows_capture::{
    capture::{CaptureControl, Context as CaptureContext, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::{GraphicsCaptureApi, InternalCaptureControl},
    settings::*,
    window::Window,
};

pub fn steam_libraries() -> Vec<PathBuf> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let Some(root) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Valve\\Steam")
        .ok()
        .and_then(|k| k.get_value::<String, _>("SteamPath").ok())
        .map(PathBuf::from)
    else {
        return vec![];
    };
    let mut roots = vec![root.clone()];
    if let Ok(v) = discovery::read_text(&root.join("steamapps/libraryfolders.vdf"))
        .and_then(|s| discovery::parse_vdf(&s))
        && let Some(Vdf::Object(folders)) =
            v.get("libraryfolders").or_else(|| v.get("LibraryFolders"))
    {
        for (key, value) in folders {
            if key.chars().all(|c| c.is_ascii_digit())
                && let Some(path) = value.str("path").or(value.text())
            {
                roots.push(PathBuf::from(path));
            }
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

pub fn find_engine() -> Option<PathBuf> {
    for root in steam_libraries() {
        let apps = root.join("steamapps");
        let folder = discovery::read_text(&apps.join("appmanifest_431960.acf"))
            .ok()
            .and_then(|s| discovery::parse_vdf(&s).ok())
            .and_then(|v| {
                v.get("AppState")
                    .and_then(|v| v.str("installdir"))
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| "wallpaper_engine".into());
        let install = apps.join("common").join(folder);
        for relative in [
            "wallpaper64.exe",
            "distribution/wallpaper64.exe",
            "wallpaper32.exe",
            "distribution/wallpaper32.exe",
        ] {
            let exe = install.join(relative);
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

pub fn validate_engine(exe: &Path) -> Result<()> {
    ensure!(
        exe.is_file(),
        "Wallpaper Engine не найден. Укажите wallpaper64.exe или wallpaper32.exe"
    );
    let name = exe
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    ensure!(
        ["wallpaper32.exe", "wallpaper64.exe"].contains(&name.as_str()),
        "Выберите wallpaper64.exe или wallpaper32.exe из папки Wallpaper Engine"
    );
    Ok(())
}

pub fn validate_project(path: &Path) -> Result<()> {
    ensure!(
        path.is_file(),
        "Файл обоев недоступен. Проверьте диск или выберите другие обои"
    );
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if ext == "json" {
        let value: serde_json::Value = serde_json::from_str(&discovery::read_text(path)?)?;
        let kind = value["type"].as_str().unwrap_or("").to_lowercase();
        ensure!(
            ["scene", "video", "web"].contains(&kind.as_str()),
            "Поддерживаются обои типа Scene, Video и Web"
        );
        let file = value["file"]
            .as_str()
            .context("В проекте обоев не указан файл")?;
        ensure!(
            path.parent().unwrap_or(Path::new(".")).join(file).is_file()
                || (kind == "scene"
                    && path
                        .parent()
                        .unwrap_or(Path::new("."))
                        .join("scene.pkg")
                        .is_file()),
            "Файл проекта обоев отсутствует. Дождитесь загрузки в Wallpaper Engine"
        );
    } else {
        ensure!(
            ["mp4", "webm", "html", "htm"].contains(&ext.as_str()),
            "Выберите project.json, MP4, WebM или HTML"
        );
    }
    Ok(())
}

/// Installed projects only: never recursively walk a user's disks or download Workshop items.
pub fn projects(exe: Option<&Path>) -> Vec<(String, PathBuf)> {
    let mut folders: Vec<PathBuf> = steam_libraries()
        .into_iter()
        .map(|p| p.join("steamapps/workshop/content/431960"))
        .collect();
    if let Some(exe) = exe
        && let Some(parent) = exe.parent()
    {
        let root = if parent.file_name().is_some_and(|n| n == "distribution") {
            parent.parent().unwrap_or(parent)
        } else {
            parent
        };
        folders.push(root.join("projects/myprojects"));
        folders.push(root.join("projects/defaultprojects"));
    }
    let mut result = vec![];
    for folder in folders {
        if let Ok(entries) = std::fs::read_dir(folder) {
            for entry in entries.flatten() {
                let path = entry.path().join("project.json");
                if let Ok(text) = discovery::read_text(&path)
                    && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
                    && ["scene", "video", "web"]
                        .contains(&v["type"].as_str().unwrap_or("").to_lowercase().as_str())
                {
                    let title = v["title"].as_str().unwrap_or("Обои").to_owned();
                    result.push((title, path));
                }
            }
        }
    }
    result.sort_by_key(|(name, _)| name.to_lowercase());
    result.dedup_by(|a, b| a.1 == b.1);
    result
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    exe: PathBuf,
    project: PathBuf,
    size: (u32, u32),
    fps: u32,
    revision: u64,
}
impl Request {
    pub fn for_state(
        settings: &Settings,
        visible: bool,
        focused: bool,
        launching: bool,
        size: (u32, u32),
        revision: u64,
    ) -> Option<Self> {
        if settings.background_mode != BackgroundMode::WallpaperEngine
            || !visible
            || !focused
            || launching
        {
            return None;
        }
        let scale = (1280. / size.0.max(1) as f64)
            .min(800. / size.1.max(1) as f64)
            .min(1.);
        Some(Self {
            exe: settings.wallpaper_engine.clone()?,
            project: settings.wallpaper_project.clone()?,
            size: (
                (size.0 as f64 * scale).round().max(32.) as u32,
                (size.1 as f64 * scale).round().max(32.) as u32,
            ),
            fps: settings.wallpaper_fps.clamp(15, 60).min(settings.fps),
            revision,
        })
    }
}

#[derive(Default)]
struct Shared {
    frame: Option<(PathBuf, egui::ColorImage)>,
    status: Option<String>,
}
pub struct Service {
    desired: Arc<Mutex<Option<Request>>>,
    shared: Arc<Mutex<Shared>>,
    wake: mpsc::SyncSender<()>,
    shutdown: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Service {
    pub fn new(ctx: egui::Context) -> Self {
        let desired = Arc::new(Mutex::new(None));
        let shared = Arc::new(Mutex::new(Shared::default()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let (wake, rx) = mpsc::sync_channel(1);
        let (d, s, stop) = (desired.clone(), shared.clone(), shutdown.clone());
        let worker = thread::spawn(move || worker(d, s, stop, rx, ctx));
        Self {
            desired,
            shared,
            wake,
            shutdown,
            worker: Some(worker),
        }
    }
    pub fn update(&self, request: Option<Request>) {
        let mut desired = self.desired.lock().unwrap();
        if *desired != request {
            *desired = request;
            let _ = self.wake.try_send(());
        }
    }
    pub fn drain(&self) -> (Option<(PathBuf, egui::ColorImage)>, Option<String>) {
        let mut shared = self.shared.lock().unwrap();
        (shared.frame.take(), shared.status.take())
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

type CaptureError = Box<dyn std::error::Error + Send + Sync>;
struct CaptureFlags {
    shared: Arc<Mutex<Shared>>,
    ctx: egui::Context,
    project: PathBuf,
    interval: Duration,
}
struct Capture {
    flags: CaptureFlags,
    last: Option<Instant>,
    scratch: Vec<u8>,
}
impl GraphicsCaptureApiHandler for Capture {
    type Flags = CaptureFlags;
    type Error = CaptureError;
    fn new(ctx: CaptureContext<Self::Flags>) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            flags: ctx.flags,
            last: None,
            scratch: vec![],
        })
    }
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _: InternalCaptureControl,
    ) -> std::result::Result<(), Self::Error> {
        if self.last.is_some_and(|t| t.elapsed() < self.flags.interval)
            || self.flags.shared.lock().unwrap().frame.is_some()
        {
            return Ok(());
        }
        let (w, h) = (frame.width() as usize, frame.height() as usize);
        if w == 0 || h == 0 || w > 2560 || h > 1600 {
            return Ok(());
        }
        let buffer = frame.buffer()?;
        let bytes = buffer.as_nopadding_buffer(&mut self.scratch);
        let image = egui::ColorImage::from_rgba_unmultiplied([w, h], bytes);
        self.flags.shared.lock().unwrap().frame = Some((self.flags.project.clone(), image));
        self.last = Some(Instant::now());
        self.flags.ctx.request_repaint();
        Ok(())
    }
}

fn engine_running(exe: &Path) -> bool {
    // Match the full installation path, not a similarly named process.
    let wanted = exe.to_string_lossy().replace('/', "\\").to_lowercase();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return false;
        };
        let mut item = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = false;
        let mut more = Process32FirstW(snapshot, &mut item).is_ok();
        while more {
            let name = String::from_utf16_lossy(
                &item.szExeFile[..item
                    .szExeFile
                    .iter()
                    .position(|v| *v == 0)
                    .unwrap_or(item.szExeFile.len())],
            );
            if name.eq_ignore_ascii_case(&exe.file_name().unwrap_or_default().to_string_lossy())
                && let Ok(process) =
                    OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, item.th32ProcessID)
            {
                let mut path = vec![0u16; 32768];
                let mut size = path.len() as u32;
                if QueryFullProcessImageNameW(
                    process,
                    PROCESS_NAME_WIN32,
                    windows::core::PWSTR(path.as_mut_ptr()),
                    &mut size,
                )
                .is_ok()
                {
                    found =
                        String::from_utf16_lossy(&path[..size as usize]).to_lowercase() == wanted;
                }
                let _ = CloseHandle(process);
                if found {
                    break;
                }
            }
            more = Process32NextW(snapshot, &mut item).is_ok();
        }
        let _ = CloseHandle(snapshot);
        found
    }
}

fn running_engine(exe: &Path) -> Option<PathBuf> {
    // Steam's launcher installs runtime files from distribution on first launch.
    // The running 32-bit edition can also receive commands from a 64-bit HITDROL.
    let parent = exe.parent()?;
    let mut folders = vec![parent.to_path_buf()];
    if parent.file_name().is_some_and(|n| n == "distribution") {
        folders.insert(0, parent.parent()?.to_path_buf());
    }
    for folder in folders {
        for name in ["wallpaper64.exe", "wallpaper32.exe"] {
            let candidate = folder.join(name);
            if engine_running(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn open_args(request: &Request, name: &str) -> Vec<OsString> {
    [
        OsString::from("-control"),
        "openWallpaper".into(),
        "-file".into(),
        request.project.as_os_str().into(),
        "-playInWindow".into(),
        name.into(),
        "-width".into(),
        request.size.0.to_string().into(),
        "-height".into(),
        request.size.1.to_string().into(),
        "-x".into(),
        "-10000".into(),
        "-y".into(),
        "-10000".into(),
        "-borderless".into(),
        "true".into(),
        "-activate".into(),
        "false".into(),
    ]
    .into()
}
fn cli(exe: &Path, args: &[OsString]) -> Result<()> {
    let parent = exe.parent().context("Папка Wallpaper Engine недоступна")?;
    let root = if parent.file_name().is_some_and(|n| n == "distribution") {
        parent.parent().unwrap_or(parent)
    } else {
        parent
    };
    let mut child = Command::new(exe)
        .args(args)
        .current_dir(root)
        .creation_flags(0x08000000)
        .spawn()
        .context("Не удалось передать команду Wallpaper Engine")?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "Wallpaper Engine отклонил команду");
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(3) {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Wallpaper Engine не отвечает");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

struct Session {
    control: Option<CaptureControl<Capture, CaptureError>>,
    window: Window,
    exe: PathBuf,
    name: String,
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.stop();
        }
        // A location is mandatory: omitting it would close the user's desktop wallpapers.
        if engine_running(&self.exe) {
            let _ = cli(
                &self.exe,
                &[
                    "-control".into(),
                    "closeWallpaper".into(),
                    "-location".into(),
                    self.name.clone().into(),
                ],
            );
        }
        if let Ok(window) = Window::from_name(&self.name)
            && window.as_raw_hwnd() == self.window.as_raw_hwnd()
        {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(window.as_raw_hwnd())),
                    WM_CLOSE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }
}

fn open(
    request: &Request,
    name: &str,
    shared: Arc<Mutex<Shared>>,
    ctx: egui::Context,
    desired: &Mutex<Option<Request>>,
    shutdown: &AtomicBool,
) -> Result<Option<Session>> {
    validate_engine(&request.exe)?;
    validate_project(&request.project)?;
    let exe = running_engine(&request.exe).context(
        "Сначала запустите Wallpaper Engine через Steam, затем нажмите «Повторить подключение»",
    )?;
    cli(&exe, &open_args(request, name))?;
    let start = Instant::now();
    let window = loop {
        if let Ok(window) = Window::from_name(name) {
            break window;
        }
        if start.elapsed() > Duration::from_secs(8)
            || shutdown.load(Ordering::Acquire)
            || desired.lock().unwrap().as_ref() != Some(request)
        {
            let _ = cli(
                &exe,
                &[
                    "-control".into(),
                    "closeWallpaper".into(),
                    "-location".into(),
                    name.into(),
                ],
            );
            ensure!(
                start.elapsed() <= Duration::from_secs(8),
                "Wallpaper Engine не открыл фон. Проверьте проект и повторите подключение"
            );
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(50));
    };
    let mut session = Session {
        control: None,
        window,
        exe,
        name: name.into(),
    };
    unsafe {
        let hwnd = HWND(window.as_raw_hwnd());
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            (style | WS_EX_TOOLWINDOW.0 as isize | WS_EX_NOACTIVATE.0 as isize)
                & !(WS_EX_APPWINDOW.0 as isize),
        );
    }
    let interval = Duration::from_secs_f64(1. / request.fps as f64);
    // Ask newer Windows versions to avoid producing unused frames. Older Windows 11
    // versions fall back to dropping frames before readback, never failing to start.
    let os_throttle = GraphicsCaptureApi::is_minimum_update_interval_supported().unwrap_or(false);
    let settings = windows_capture::settings::Settings::new(
        window,
        CursorCaptureSettings::WithoutCursor,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        if os_throttle {
            MinimumUpdateIntervalSettings::Custom(interval)
        } else {
            MinimumUpdateIntervalSettings::Default
        },
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        CaptureFlags {
            shared,
            ctx,
            project: request.project.clone(),
            interval: if os_throttle {
                Duration::ZERO
            } else {
                interval
            },
        },
    );
    session.control = Some(
        Capture::start_free_threaded(settings)
            .map_err(|e| anyhow::anyhow!("Захват фона недоступен: {e}"))?,
    );
    Ok(Some(session))
}

fn worker(
    desired: Arc<Mutex<Option<Request>>>,
    shared: Arc<Mutex<Shared>>,
    shutdown: Arc<AtomicBool>,
    wake: mpsc::Receiver<()>,
    ctx: egui::Context,
) {
    let initialized = unsafe {
        windows::Win32::System::WinRT::RoInitialize(
            windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
        )
    }
    .is_ok();
    let name = format!(
        "HITDROL-background-{}-{}",
        std::process::id(),
        crate::model::timestamp()
    );
    let mut previous = None;
    let mut session: Option<Session> = None;
    while !shutdown.load(Ordering::Acquire) {
        let request = desired.lock().unwrap().clone();
        if request != previous {
            drop(session.take());
            shared.lock().unwrap().frame = None;
            previous = request.clone();
            let status = if let Some(request) = request {
                match open(
                    &request,
                    &name,
                    shared.clone(),
                    ctx.clone(),
                    &desired,
                    &shutdown,
                ) {
                    Ok(value) => {
                        session = value;
                        if session.is_some() {
                            "Анимированный фон включён".into()
                        } else {
                            "Фон приостановлен".into()
                        }
                    }
                    Err(error) => format!("{error:#}"),
                }
            } else {
                "Фон приостановлен · ресурсы воспроизведения освобождены".into()
            };
            shared.lock().unwrap().status = Some(status);
            ctx.request_repaint();
        }
        if session
            .as_ref()
            .is_some_and(|s| s.control.as_ref().is_some_and(|c| c.is_finished()))
        {
            drop(session.take());
            shared.lock().unwrap().status =
                Some("Воспроизведение завершено. Нажмите «Повторить подключение»".into());
            ctx.request_repaint();
        }
        if session.is_some() {
            let _ = wake.recv_timeout(Duration::from_millis(250));
        } else if wake.recv().is_err() {
            break;
        }
    }
    drop(session);
    if initialized {
        unsafe {
            windows::Win32::System::WinRT::RoUninitialize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> Settings {
        Settings {
            background_mode: BackgroundMode::WallpaperEngine,
            wallpaper_engine: Some("C:/WE/wallpaper64.exe".into()),
            wallpaper_project: Some("D:/Мои обои/project.json".into()),
            ..Default::default()
        }
    }
    #[test]
    fn lifecycle_releases_background_on_game_focus_loss_and_minimize() {
        let s = settings();
        assert!(Request::for_state(&s, true, true, false, (1920, 1200), 0).is_some());
        for (visible, focus, launch) in [
            (false, true, false),
            (true, false, false),
            (true, true, true),
            (false, false, true),
        ] {
            assert!(Request::for_state(&s, visible, focus, launch, (1920, 1200), 0).is_none());
        }
        let r = Request::for_state(&s, true, true, false, (1920, 1200), 0).unwrap();
        assert_eq!(r.size, (1280, 800));
        assert_eq!(r.fps, 30);
    }
    #[test]
    fn command_preserves_unicode_and_spaces_without_shell() {
        let r = Request::for_state(&settings(), true, true, false, (1280, 720), 0).unwrap();
        let args = open_args(&r, "HITDROL private window");
        assert_eq!(args[3], OsString::from("D:/Мои обои/project.json"));
        assert_eq!(args[5], OsString::from("HITDROL private window"));
        assert!(
            !args
                .iter()
                .any(|s| s == "pause" || s == "stop" || s == "play")
        );
    }
    #[test]
    fn rejects_application_and_incomplete_projects() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.json");
        std::fs::write(&path, r#"{"type":"application","file":"game.exe"}"#).unwrap();
        assert!(validate_project(&path).is_err());
        std::fs::write(&path, r#"{"type":"scene","file":"scene.pkg"}"#).unwrap();
        assert!(validate_project(&path).is_err());
        std::fs::write(dir.path().join("scene.pkg"), b"fixture").unwrap();
        assert!(validate_project(&path).is_ok());
    }
    #[test]
    fn old_settings_stay_on_default_background() {
        let s: Settings = serde_json::from_str(r#"{"fps":40,"fullscreen":false}"#).unwrap();
        assert_eq!(s.background_mode, BackgroundMode::Default);
        assert_eq!(s.fps, 40);
        assert_eq!(s.background_dim, 65);
        let mut s = settings();
        s.wallpaper_fps = 500;
        s.background_dim = 0;
        s.sanitize();
        assert_eq!(s.wallpaper_fps, 60);
        assert_eq!(s.background_dim, 30);
    }
}
