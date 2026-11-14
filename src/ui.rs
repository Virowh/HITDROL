use crate::{
    artwork::{self, ArtGame, ArtImage, SteamGrid, TextureCache},
    discovery::{self, Scan},
    input::{Action, InputMethod, NavItem, Navigation},
    model::*,
    platform,
    storage::Database,
    wallpaper,
};
use anyhow::Result;
use egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

const BG: Color32 = Color32::from_rgb(12, 17, 24);
const PANEL: Color32 = Color32::from_rgb(23, 30, 40);
const MUTED: Color32 = Color32::from_rgb(142, 155, 174);
const TEXT: Color32 = Color32::from_rgb(236, 241, 248);
const ACCENT: Color32 = Color32::from_rgb(101, 210, 255);
const PLAY_HOVER: Color32 = Color32::from_rgb(94, 220, 132);
const PLAY_PRESSED: Color32 = Color32::from_rgb(64, 184, 104);
const STOP: Color32 = Color32::from_rgb(183, 39, 52);
const STOP_HOVER: Color32 = Color32::from_rgb(205, 49, 64);
const STOP_PRESSED: Color32 = Color32::from_rgb(146, 29, 41);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Home,
    Library,
    Add,
    Settings,
    Detail,
}
#[derive(Clone)]
enum Purpose {
    Add,
    Cover(String),
    Hero(String),
    Wallpaper,
    EngineProject,
    EngineExecutable,
}
#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    directory: bool,
}
struct Browser {
    purpose: Purpose,
    path: Option<PathBuf>,
    entries: Vec<Entry>,
    busy: bool,
    token: u64,
    error: Option<String>,
}
#[derive(Clone)]
enum TextPurpose {
    Search,
    Name,
    Path,
    Args,
    Cwd,
    Key,
    Fps,
    Account(Source),
}
struct Keyboard {
    title: String,
    text: String,
    purpose: TextPurpose,
    russian: bool,
    shift: bool,
}
struct Edit {
    game: Game,
    name: String,
    path: String,
    args: String,
    cwd: String,
    kind: LaunchKind,
}
enum ArtPanel {
    Searching,
    Games(Vec<ArtGame>),
    Images(Vec<(ArtImage, Option<PathBuf>)>),
    Loading,
}
struct ArtDialog {
    token: u64,
    game: String,
    hero: bool,
    panel: ArtPanel,
}
enum Event {
    Scan(Scan, bool),
    Desktop(Scan),
    File(Box<Result<Game, String>>),
    Launch(String, Result<(), String>),
    Played(String, u64),
    Running(Vec<crate::activity::RunningGame>),
    Closed(String, Result<usize, String>),
    InstallSent(Result<(), String>),
    AccountOpened(Source, Result<(), String>),
    AccountProgress(Source, usize, usize),
    AccountLibrary(Source, Result<Vec<Game>, String>),
    Directory(u64, Result<Vec<Entry>, String>),
    ArtGames(u64, Result<Vec<ArtGame>, String>),
    ArtImages(u64, Result<Vec<(ArtImage, Option<PathBuf>)>, String>),
    ArtSaved(String, bool, Result<PathBuf, String>),
    AutoArt(String, Option<PathBuf>, Option<PathBuf>),
    AutoDone(usize, Option<String>),
    ImageImported(String, bool, Result<PathBuf, String>),
    WallpaperImported(u64, Result<PathBuf, String>),
    EngineDetected(Option<PathBuf>),
    ProjectValidated(u64, Result<PathBuf, String>),
}
#[derive(Clone)]
enum Command {
    Page(Page),
    Open(String),
    Back,
    Favorite(String),
    Launch(String),
    CloseGame(String),
    Install(String),
    LibraryMode(bool),
    Connect(Source),
    Disconnect(Source),
    Edit(String),
    SaveEdit,
    Keyboard(TextPurpose),
    ApplyText,
    Scan,
    Desktop,
    ToggleDesktop(String),
    AddSelected,
    Browse(Purpose),
    BrowseTo(Option<PathBuf>),
    Pick(PathBuf),
    Art(String, bool),
    ArtGame(u64),
    ArtImage(String),
    AutoArt,
    StoreArt,
    Fps(u32),
    DisplayRate,
    Fullscreen,
    Scale(f32),
    Motion,
    StartupAnimation,
    StartupSound,
    Autostart,
    StartupSettings,
    Background(BackgroundMode),
    BackgroundDim(u8),
    WallpaperFps(u32),
    RetryWallpaper,
    SaveKey,
    ClearKey,
    Exit,
}

pub struct UiApp {
    pub db: Database,
    pub games: Vec<Game>,
    pub settings: Settings,
    pub page: Page,
    pub selected: Option<String>,
    pub method: InputMethod,
    pub nav: Navigation,
    textures: TextureCache,
    logo: egui::TextureHandle,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    ctx: egui::Context,
    commands: Vec<Command>,
    accept: bool,
    pub focused: bool,
    pub gamepad_suspended: bool,
    pub exit: bool,
    pub refresh_hz: u32,
    pub measured_fps: f32,
    pub paste_requested: bool,
    autostart_registered: bool,
    search: String,
    filter: Option<Source>,
    favorites: bool,
    desktop: Vec<Game>,
    desktop_selection: HashSet<String>,
    scanning: bool,
    desktop_busy: bool,
    browser: Option<Browser>,
    edit: Option<Edit>,
    keyboard: Option<Keyboard>,
    art: Option<ArtDialog>,
    notice: Option<(String, Instant)>,
    warnings: Vec<String>,
    api_key: String,
    key_present: bool,
    auto_busy: bool,
    art_pending: bool,
    launching: Option<String>,
    running: Vec<crate::activity::RunningGame>,
    closing: HashSet<String>,
    demo: bool,
    home_focus: Option<String>,
    library_focus: Option<String>,
    previous_page: Page,
    nav_scope: String,
    request_epoch: u64,
    wallpaper_epoch: u64,
    pub wallpaper_revision: u64,
    pub wallpaper_status: String,
    pub wallpaper_service: wallpaper::Service,
    wallpaper_texture: Option<egui::TextureHandle>,
    wallpaper_texture_project: Option<PathBuf>,
    activity: crate::activity::Monitor,
    last_scan: Instant,
    scan_pending: Option<bool>,
    account_busy: HashSet<Source>,
    account_connected: HashSet<Source>,
    account_status: std::collections::HashMap<Source, String>,
    last_accounts: Option<Instant>,
}
impl UiApp {
    /// Deterministic preview routes used by the screenshot/QA CLI.
    pub fn preview(&mut self, name: &str) {
        match name {
            "library" => self.change_page(Page::Library),
            "settings" => self.change_page(Page::Settings),
            "all-settings" => {
                self.settings.show_uninstalled = true;
                self.change_page(Page::Settings);
            }
            "account" => self.open_keyboard(TextPurpose::Account(Source::Epic)),
            "install" => {
                if let Some(game) = self.games.first_mut() {
                    game.installed = false;
                    game.available = false;
                    game.owned = true;
                    game.install_target = Some(LaunchTarget::uri("steam://install/42"));
                }
                self.change_page(Page::Detail);
            }
            "recent-empty" => {
                for game in &mut self.games {
                    game.history_verified = false;
                }
                self.change_page(Page::Home);
            }
            "add" => self.change_page(Page::Add),
            "detail" => self.change_page(Page::Detail),
            "keyboard" => self.open_keyboard(TextPurpose::Search),
            "browser" => {
                self.commands.push(Command::Browse(Purpose::Add));
                self.process_commands();
            }
            "edit" => {
                if let Some(id) = self.selected.clone() {
                    self.commands.push(Command::Edit(id));
                    self.process_commands();
                }
            }
            "empty" => self.games.clear(),
            _ => self.change_page(Page::Home),
        }
    }
    pub fn new(db: Database, ctx: egui::Context, demo: usize) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let settings = db.settings()?;
        let mut games = db.games()?;
        if demo > 0 {
            games = demo_games(demo);
        }
        configure(&ctx);
        let key_present = platform::read_key().ok().flatten().is_some();
        let activity_tx = tx.clone();
        let activity_ctx = ctx.clone();
        let running_tx = tx.clone();
        let running_ctx = ctx.clone();
        let activity = crate::activity::Monitor::with_running(
            move |id, at| {
                let _ = activity_tx.send(Event::Played(id, at));
                activity_ctx.request_repaint();
            },
            move |running| {
                let _ = running_tx.send(Event::Running(running));
                running_ctx.request_repaint();
            },
        );
        let mut app = Self {
            db,
            games,
            settings,
            page: Page::Home,
            selected: None,
            method: InputMethod::Mouse,
            nav: Navigation::default(),
            textures: TextureCache::new(ctx.clone()),
            logo: crate::branding::texture(&ctx),
            tx,
            rx,
            ctx: ctx.clone(),
            commands: vec![],
            accept: false,
            focused: true,
            gamepad_suspended: false,
            exit: false,
            refresh_hz: 60,
            measured_fps: 0.,
            paste_requested: false,
            autostart_registered: crate::autostart::registered().unwrap_or(false),
            search: String::new(),
            filter: None,
            favorites: false,
            desktop: vec![],
            desktop_selection: HashSet::new(),
            scanning: false,
            desktop_busy: false,
            browser: None,
            edit: None,
            keyboard: None,
            art: None,
            notice: None,
            warnings: vec![],
            api_key: String::new(),
            key_present,
            auto_busy: false,
            art_pending: false,
            launching: None,
            running: vec![],
            closing: HashSet::new(),
            demo: demo > 0,
            home_focus: None,
            library_focus: None,
            previous_page: Page::Home,
            nav_scope: String::new(),
            request_epoch: 0,
            wallpaper_epoch: 0,
            wallpaper_revision: 0,
            wallpaper_status: String::new(),
            wallpaper_service: wallpaper::Service::new(ctx.clone()),
            wallpaper_texture: None,
            wallpaper_texture_project: None,
            activity,
            last_scan: Instant::now(),
            scan_pending: None,
            account_busy: HashSet::new(),
            account_connected: [Source::Epic, Source::Gog]
                .into_iter()
                .filter(|s| platform::library_token(*s).ok().flatten().is_some())
                .collect(),
            account_status: Default::default(),
            last_accounts: None,
        };
        app.selected = app.games.first().map(|g| g.id.clone());
        if app.settings.wallpaper_engine.is_none() {
            app.background(|| Event::EngineDetected(wallpaper::find_engine()));
        }
        if demo == 0 {
            app.activity.update(&app.games);
            app.start_scan();
        }
        Ok(app)
    }
    fn background<F>(&self, work: F)
    where
        F: FnOnce() -> Event + Send + 'static,
    {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let event = work();
            let _ = tx.send(event);
            ctx.request_repaint();
        });
    }
    fn notify(&mut self, message: impl Into<String>) {
        self.notice = Some((message.into(), Instant::now()));
        self.ctx.request_repaint();
    }
    pub fn paste_text(&mut self, text: String) {
        if let Some(k) = &mut self.keyboard {
            k.text
                .push_str(&text.chars().take(8192).collect::<String>());
        }
    }
    pub fn action(&mut self, action: Action) {
        if !self.focused {
            return;
        }
        match action {
            Action::Accept => self.accept = true,
            Action::Back => self.commands.push(Command::Back),
            Action::PreviousTab | Action::NextTab
                if self.browser.is_none()
                    && self.keyboard.is_none()
                    && self.edit.is_none()
                    && self.art.is_none() =>
            {
                let pages = [Page::Home, Page::Library, Page::Add, Page::Settings];
                let i = pages.iter().position(|p| *p == self.page).unwrap_or(0);
                self.commands.push(Command::Page(
                    pages[(i + if action == Action::NextTab { 1 } else { 3 }) % 4],
                ));
            }
            Action::Menu => {
                if self.keyboard.is_none()
                    && self.browser.is_none()
                    && self.art.is_none()
                    && let Some(id) = self.selected.clone()
                {
                    self.commands.push(Command::Edit(id));
                }
            }
            _ => self.nav.move_focus(action),
        }
        if let Some(id) = self
            .nav
            .focused
            .as_ref()
            .and_then(|s| s.strip_prefix("game:"))
        {
            self.selected = Some(id.into());
        }
        self.ctx.request_repaint();
    }
    fn start_scan(&mut self) {
        self.request_scan(false);
    }
    fn request_scan(&mut self, notify: bool) {
        if self.demo || self.exit {
            return;
        }
        if self.scanning {
            self.scan_pending = Some(self.scan_pending.unwrap_or(false) || notify);
            return;
        }
        self.scanning = true;
        self.last_scan = Instant::now();
        self.textures.retry_failed();
        let all = self.settings.show_uninstalled;
        let previous = self.games.clone();
        self.background(move || Event::Scan(discovery::scan_library(all, &previous), notify));
    }
    fn start_accounts(&mut self, force: bool) {
        if self.demo || self.exit || !self.settings.show_uninstalled {
            return;
        }
        if !force
            && self
                .last_accounts
                .is_some_and(|t| t.elapsed() < Duration::from_secs(300))
        {
            return;
        }
        self.last_accounts = Some(Instant::now());
        for source in [Source::Epic, Source::Gog] {
            if self.account_connected.contains(&source) {
                self.sync_account(source, None);
            }
        }
    }
    fn sync_account(&mut self, source: Source, code: Option<String>) {
        if self.demo || self.exit || !self.account_busy.insert(source) {
            return;
        }
        self.account_status
            .insert(source, "Получаем библиотеку…".into());
        let cached = self.games.clone();
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        self.background(move || {
            let result = crate::accounts::sync(source, code.as_deref(), &cached, |done, total| {
                if done % 10 == 0 || done == total {
                    let _ = tx.send(Event::AccountProgress(source, done, total));
                    ctx.request_repaint();
                }
            });
            Event::AccountLibrary(source, result.map_err(|e| format!("{e:#}")))
        });
    }
    pub fn resumed_focus(&mut self) {
        self.autostart_registered = crate::autostart::registered().unwrap_or(false);
        self.activity.update(&self.games);
        if self.last_scan.elapsed() > Duration::from_secs(5) {
            self.start_scan();
        }
    }
    pub fn flush_events(&mut self) {
        self.process_events();
    }
    fn load_desktop(&mut self) {
        if self.desktop_busy {
            return;
        }
        self.desktop_busy = true;
        let cache = self.db.root.join("icons");
        self.background(move || Event::Desktop(discovery::desktop(&cache)));
    }
    fn save_game(&mut self, game: Game) {
        if !self.demo
            && let Err(e) = self.db.save(&game)
        {
            self.notify(format!("Не удалось сохранить: {e}"));
            return;
        }
        if let Some(g) = self.games.iter_mut().find(|g| g.id == game.id) {
            let track_changed = g.target() != game.target() || g.process_path != game.process_path;
            *g = game;
            if track_changed && !self.demo {
                self.activity.update(&self.games);
            }
        }
    }
    fn merge(&mut self, games: Vec<Game>) {
        if self.demo {
            self.games.extend(games);
            return;
        }
        match self.db.merge(games) {
            Ok(g) => self.games = g,
            Err(e) => self.notify(format!("Ошибка библиотеки: {e}")),
        }
        self.activity.update(&self.games);
    }
    fn process_events(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            let animated = matches!(event, Event::ProjectValidated(..));
            match event {
                Event::Scan(scan, notify) => {
                    self.scanning = false;
                    self.warnings = scan.warnings;
                    let count = scan.games.iter().filter(|g|g.installed).count();
                    let mut incoming=scan.games;
                    for game in &mut incoming { if matches!(game.source,Source::Epic|Source::Gog) {game.owned=false;} }
                    match self.db.merge_library(incoming,&scan.owned_sources,false) {
                        Ok(games)=>{
                            self.games=games;
                            if notify {self.notify(format!("Сканирование завершено · установлено {count}"));}
                        },
                        Err(e)=>self.notify(format!("Ошибка библиотеки: {e}")),
                    }
                    self.activity.update(&self.games);
                    self.start_auto_art();
                    self.start_accounts(false);
                    if let Some(notify) = self.scan_pending.take() {self.request_scan(notify);}
                }
                Event::AccountOpened(source,result)=>match result {
                    Ok(())=>self.open_keyboard(TextPurpose::Account(source)),
                    Err(e)=>self.notify(e),
                },
                Event::AccountProgress(source,done,total)=>{self.account_status.insert(source,format!("Получаем библиотеку: {done} / {total}"));},
                Event::AccountLibrary(source,result)=>{
                    self.account_busy.remove(&source);
                    if platform::library_token(source).ok().flatten().is_some() {self.account_connected.insert(source);}
                    match result {
                        Ok(games)=>{
                            let count=games.len();
                            match self.db.merge_library(games,&[source],true) {
                                Ok(games)=>self.games=games,
                                Err(e)=>{self.account_status.insert(source,format!("Не удалось сохранить: {e}"));continue;}
                            }
                            self.activity.update(&self.games);
                            self.account_status.insert(source,format!("Синхронизировано · {count} игр"));
                            self.notify(format!("{} · в библиотеке {count} игр",source.label()));
                            self.start_auto_art();
                        }
                        Err(e)=>{self.account_status.insert(source,e.clone());self.notify(format!("{}: {e}",source.label()));}
                    }
                }
                Event::Desktop(scan) => {
                    self.desktop_busy = false;
                    self.desktop = scan.games;
                    self.warnings.extend(scan.warnings);
                }
                Event::File(result) => match *result {
                    Ok(g) => {
                        let id = g.id.clone();
                        self.merge(vec![g]);
                        self.selected = Some(id);
                        self.browser = None;
                        self.change_page(Page::Library);
                        self.notify("Игра добавлена в библиотеку");
                        self.start_auto_art();
                    }
                    Err(e) => self.notify(e),
                },
                Event::Launch(id, result) => {
                    self.launching = None;
                    match result {
                        Ok(()) => {
                            let _=id;
                            self.notify("Команда запуска отправлена. Ожидаем процесс игры.");
                        }
                        Err(e) => {
                            self.gamepad_suspended = false;
                            self.notify(format!("Не удалось запустить: {e}"));
                        }
                    }
                }
                Event::Running(running) => self.running = running,
                Event::Closed(id, result) => {
                    self.closing.remove(&id);
                    self.activity.update(&self.games);
                    match result {
                        Ok(_) => self.notify("Запрос закрытия отправлен. Если игра показывает подтверждение, вернитесь в неё. Если она не отвечает, закройте её через меню игры или диспетчер задач."),
                        Err(e) => self.notify(format!("Не удалось закрыть игру: {e}")),
                    }
                }
                Event::Played(id,at)=>{
                    if let Some(mut game)=self.games.iter().find(|g|g.id==id).cloned()
                        && valid_played(at) && (!game.history_verified || !valid_played(game.last_played) || at>game.last_played) {
                        game.record_played(at);self.save_game(game);
                    }
                }
                Event::InstallSent(result)=>match result {
                    Ok(())=>self.notify("Установка открыта в клиенте магазина. После установки библиотека обновится."),
                    Err(e)=>{self.gamepad_suspended=false;self.notify(format!("Не удалось открыть установку: {e}"));},
                },
                Event::Directory(token, result) => {
                    if let Some(b) = &mut self.browser
                        && b.token == token
                    {
                        b.busy = false;
                        match result {
                            Ok(entries) => {
                                b.entries = entries;
                                b.error = None;
                            }
                            Err(e) => b.error = Some(e),
                        }
                    }
                }
                Event::ArtGames(token, result) => {
                    if self.art.as_ref().is_none_or(|a| a.token != token) {
                        continue;
                    }
                    match result {
                        Ok(games) => {
                            if let Some(a) = &mut self.art {
                                a.panel = ArtPanel::Games(games);
                            }
                        }
                        Err(e) => {
                            self.art = None;
                            self.notify(e);
                        }
                    }
                }
                Event::ArtImages(token, result) => {
                    if self.art.as_ref().is_none_or(|a| a.token != token) {
                        continue;
                    }
                    match result {
                        Ok(images) => {
                            if let Some(a) = &mut self.art {
                                a.panel = ArtPanel::Images(images);
                            }
                        }
                        Err(e) => {
                            self.art = None;
                            self.notify(e);
                        }
                    }
                }
                Event::EngineDetected(path) => {
                    if self.settings.wallpaper_engine.is_none() {
                        self.settings.wallpaper_engine = path;
                        let _ = self.db.save_settings(&self.settings);
                    }
                }
                Event::WallpaperImported(epoch, result)
                | Event::ProjectValidated(epoch, result) => {
                    if epoch != self.wallpaper_epoch {
                        continue;
                    }
                    match result {
                        Ok(path) => {
                            if animated {
                                self.settings.wallpaper_project = Some(path);
                                self.settings.background_mode = BackgroundMode::WallpaperEngine;
                                self.wallpaper_revision += 1;
                            } else {
                                self.settings.background_image = Some(path);
                                self.settings.background_mode = BackgroundMode::Image;
                            }
                            self.browser = None;
                            if let Err(e) = self.db.save_settings(&self.settings) {
                                self.notify(format!("Настройки не сохранены: {e}"));
                            } else {
                                self.notify("Обои выбраны");
                            }
                        }
                        Err(e) => self.notify(e),
                    }
                }
                Event::ArtSaved(id, hero, result) | Event::ImageImported(id, hero, result) => {
                    match result {
                        Ok(path) => {
                            if let Some(mut g) = self.games.iter().find(|g| g.id == id).cloned() {
                                if hero {
                                    g.user_hero = Some(path);
                                } else {
                                    g.user_cover = Some(path);
                                }
                                self.save_game(g);
                            }
                            self.art = None;
                            self.browser = None;
                            self.notify("Изображение сохранено");
                        }
                        Err(e) => self.notify(e),
                    }
                }
                Event::AutoArt(id, cover, hero) => {
                    if let Some(mut g) = self.games.iter().find(|g| g.id == id).cloned() {
                        if let Some(path) = cover {
                            self.textures.invalidate(&path);
                            g.cover = Some(path);
                        }
                        if let Some(path) = hero {
                            self.textures.invalidate(&path);
                            g.hero = Some(path);
                        }
                        self.save_game(g);
                    }
                }
                Event::AutoDone(n, error) => {
                    self.auto_busy = false;
                    if let Some(error) = error {
                        self.notify(format!(
                            "Загружены изображения для {n} игр. Не всё удалось загрузить: {error}. Для повтора обновите библиотеку."
                        ));
                    } else if n > 0 {
                        self.notify(format!("Загружены изображения для {n} игр"));
                    }
                    if self.art_pending {
                        self.art_pending = false;
                        self.start_auto_art();
                    }
                }
            }
        }
    }
    fn change_page(&mut self, page: Page) {
        if self.page == Page::Home {
            self.home_focus = self.nav.focused.clone();
        }
        if self.page == Page::Library {
            self.library_focus = self.nav.focused.clone();
        }
        self.page = page;
        self.nav.reset();
        self.nav.focused = match page {
            Page::Home => self.home_focus.clone(),
            Page::Library => self.library_focus.clone(),
            _ => None,
        };
        if page == Page::Add && self.desktop.is_empty() {
            self.load_desktop();
        }
    }
    fn back(&mut self) {
        if self.keyboard.is_none() && self.browser.is_some() {
            self.wallpaper_epoch += 1;
        }
        // Short-circuiting closes only the topmost surface.
        if self.keyboard.take().is_some()
            || self.browser.take().is_some()
            || self.art.take().is_some()
            || self.edit.take().is_some()
        {
            self.nav.reset();
        } else if self.page == Page::Detail {
            self.change_page(self.previous_page);
        } else {
            self.change_page(Page::Home);
        }
    }
    fn start_auto_art(&mut self) {
        if self.demo || (!self.settings.store_art && !self.key_present) {
            return;
        }
        if self.auto_busy {
            self.art_pending = true;
            return;
        }
        let games: Vec<_> = self
            .games
            .iter()
            .filter(|g| self.key_present || (self.settings.store_art && g.source != Source::Manual))
            .cloned()
            .collect();
        if games.is_empty() {
            return;
        }
        self.auto_busy = true;
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let cache = self.db.root.join("artwork");
        let (official_enabled, grid_enabled) = (self.settings.store_art, self.key_present);
        std::thread::spawn(move || {
            let official = if official_enabled {
                crate::store_art::OfficialArt::new().ok()
            } else {
                None
            };
            let mut grid = if grid_enabled {
                SteamGrid::new().ok()
            } else {
                None
            };
            let mut completed = 0;
            let mut error = None;
            let mut offline = HashSet::new();
            for mut game in games {
                let mut changed = false;
                if let Some(service) = &official
                    && !offline.contains(&game.source)
                {
                    match service.fetch(&game, &cache) {
                        Ok(paths) => {
                            changed = paths.cover.is_some() || paths.hero.is_some();
                            if let Some(path) = &paths.cover {
                                game.cover = Some(path.clone());
                            }
                            if let Some(path) = &paths.hero {
                                game.hero = Some(path.clone());
                            }
                            if changed {
                                let _ = tx.send(Event::AutoArt(
                                    game.id.clone(),
                                    paths.cover,
                                    paths.hero,
                                ));
                                ctx.request_repaint();
                            }
                        }
                        Err(e) => {
                            if e.downcast_ref::<reqwest::Error>()
                                .is_some_and(|e| e.is_connect() || e.is_timeout())
                            {
                                offline.insert(game.source);
                            }
                            error = Some(format!("{}: {e}", game.source.label()));
                        }
                    }
                }
                if let Some(service) = &grid
                    && (crate::store_art::needs_cover(&game) || crate::store_art::needs_hero(&game))
                {
                    let result = (|| -> Result<()> {
                        let matched = if game.source == Source::Steam {
                            Some(service.steam_game(&game.source_id)?)
                        } else {
                            let exact: Vec<_> = service
                                .search(game.name())?
                                .into_iter()
                                .filter(|g| {
                                    artwork::normalized_title(&g.name)
                                        == artwork::normalized_title(game.name())
                                })
                                .collect();
                            if exact.len() == 1 {
                                exact.into_iter().next()
                            } else {
                                None
                            }
                        };
                        if let Some(matched) = matched {
                            let cover = if crate::store_art::needs_cover(&game) {
                                service
                                    .images(matched.id, false)?
                                    .first()
                                    .map(|a| service.download(&a.url, &cache))
                                    .transpose()?
                            } else {
                                None
                            };
                            // The hero is optional; a failed hero must not discard a successful cover.
                            let hero = if crate::store_art::needs_hero(&game) {
                                service.images(matched.id, true).ok().and_then(|v| {
                                    v.first()
                                        .and_then(|a| service.download(&a.url, &cache).ok())
                                })
                            } else {
                                None
                            };
                            if cover.is_some() || hero.is_some() {
                                changed = true;
                                let _ = tx.send(Event::AutoArt(game.id.clone(), cover, hero));
                                ctx.request_repaint();
                            }
                        }
                        Ok(())
                    })();
                    if let Err(e) = result {
                        let message = e.to_string();
                        if message.contains("API-ключ не принят")
                            || message.contains("слишком много запросов")
                            || e.downcast_ref::<reqwest::Error>()
                                .is_some_and(|e| e.is_connect() || e.is_timeout())
                        {
                            grid = None;
                        }
                        error = Some(message);
                    }
                }
                if changed {
                    completed += 1;
                }
            }
            let _ = tx.send(Event::AutoDone(completed, error));
            ctx.request_repaint();
        });
    }
    fn open_keyboard(&mut self, purpose: TextPurpose) {
        let (title, text) = match purpose {
            TextPurpose::Search => ("Поиск игры", self.search.clone()),
            TextPurpose::Name => (
                "Название игры",
                self.edit
                    .as_ref()
                    .map(|e| e.name.clone())
                    .unwrap_or_default(),
            ),
            TextPurpose::Path => (
                "Путь или ссылка",
                self.edit
                    .as_ref()
                    .map(|e| e.path.clone())
                    .unwrap_or_default(),
            ),
            TextPurpose::Args => (
                "Аргументы запуска",
                self.edit
                    .as_ref()
                    .map(|e| e.args.clone())
                    .unwrap_or_default(),
            ),
            TextPurpose::Cwd => (
                "Рабочая папка",
                self.edit
                    .as_ref()
                    .map(|e| e.cwd.clone())
                    .unwrap_or_default(),
            ),
            TextPurpose::Key => ("Ключ SteamGridDB", String::new()),
            TextPurpose::Fps => ("Предел кадров · 30–240", self.settings.fps.to_string()),
            TextPurpose::Account(source) => (
                if source == Source::Epic {
                    "Epic Games · код из браузера"
                } else {
                    "GOG · адрес страницы после входа"
                },
                String::new(),
            ),
        };
        let russian = !matches!(
            purpose,
            TextPurpose::Search | TextPurpose::Account(_) | TextPurpose::Key
        );
        self.keyboard = Some(Keyboard {
            title: title.into(),
            text,
            purpose,
            russian,
            shift: false,
        });
        self.nav.reset();
    }
    fn apply_text(&mut self) {
        let Some(k) = self.keyboard.take() else {
            return;
        };
        match k.purpose {
            TextPurpose::Search => self.search = k.text,
            TextPurpose::Name => {
                if let Some(e) = &mut self.edit {
                    e.name = k.text;
                }
            }
            TextPurpose::Path => {
                if let Some(e) = &mut self.edit {
                    e.path = k.text;
                }
            }
            TextPurpose::Args => {
                if let Some(e) = &mut self.edit {
                    e.args = k.text;
                }
            }
            TextPurpose::Cwd => {
                if let Some(e) = &mut self.edit {
                    e.cwd = k.text;
                }
            }
            TextPurpose::Key => {
                self.api_key = k.text;
                self.commands.push(Command::SaveKey);
            }
            TextPurpose::Account(source) => {
                match crate::accounts::authorization_code(source, &k.text) {
                    Ok(code) => {
                        self.settings.show_uninstalled = true;
                        self.sync_account(source, Some(code));
                        self.start_scan();
                    }
                    Err(e) => self.notify(e.to_string()),
                }
            }
            TextPurpose::Fps => match k.text.parse::<u32>() {
                Ok(n) if (30..=240).contains(&n) => {
                    self.settings.fps = n;
                    self.settings.display_rate = false;
                }
                _ => self.notify("Введите число от 30 до 240"),
            },
        }
        self.nav.reset();
    }
    fn directory(&mut self, path: Option<PathBuf>) {
        let Some(b) = &mut self.browser else {
            return;
        };
        b.path = path.clone();
        b.busy = true;
        self.request_epoch += 1;
        b.token = self.request_epoch;
        b.entries.clear();
        let token = b.token;
        let purpose = b.purpose.clone();
        let engine = self.settings.wallpaper_engine.clone();
        self.nav.reset();
        self.background(move || {
            Event::Directory(
                token,
                (|| -> Result<Vec<Entry>> {
                    let mut entries = vec![];
                    if let Some(path) = path {
                        for entry in std::fs::read_dir(&path)
                            .with_context(|| format!("Не удалось открыть {}", path.display()))?
                            .flatten()
                        {
                            let p = entry.path();
                            let dir = p.is_dir();
                            let ext = p
                                .extension()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_lowercase();
                            if dir
                                || match purpose {
                                    Purpose::Add => ["exe", "lnk", "url"].contains(&ext.as_str()),
                                    Purpose::EngineExecutable => {
                                        ["wallpaper32.exe", "wallpaper64.exe"].contains(
                                            &p.file_name()
                                                .unwrap_or_default()
                                                .to_string_lossy()
                                                .to_lowercase()
                                                .as_str(),
                                        )
                                    }
                                    Purpose::EngineProject => {
                                        ["json", "mp4", "webm", "html", "htm"]
                                            .contains(&ext.as_str())
                                    }
                                    _ => ["png", "jpg", "jpeg", "webp", "ico"]
                                        .contains(&ext.as_str()),
                                }
                            {
                                entries.push(Entry {
                                    path: p,
                                    name: entry.file_name().to_string_lossy().into(),
                                    directory: dir,
                                });
                            }
                        }
                    } else {
                        entries = platform::drives()
                            .into_iter()
                            .map(|p| Entry {
                                name: p.display().to_string(),
                                path: p,
                                directory: true,
                            })
                            .collect();
                        if matches!(purpose, Purpose::EngineProject) {
                            entries.extend(wallpaper::projects(engine.as_deref()).into_iter().map(
                                |(name, path)| Entry {
                                    path,
                                    name,
                                    directory: false,
                                },
                            ));
                        }
                    }
                    entries.sort_by(|a, b| {
                        b.directory
                            .cmp(&a.directory)
                            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                    });
                    Ok(entries)
                })()
                .map_err(|e| e.to_string()),
            )
        });
    }
    fn process_commands(&mut self) {
        let commands = std::mem::take(&mut self.commands);
        if commands.is_empty() {
            return;
        }
        self.ctx.request_repaint();
        for cmd in commands {
            match cmd {
                Command::Page(p) => self.change_page(p),
                Command::Back => self.back(),
                Command::Open(id) => {
                    self.previous_page = if self.page == Page::Home {
                        Page::Home
                    } else {
                        Page::Library
                    };
                    self.selected = Some(id);
                    self.change_page(Page::Detail);
                }
                Command::Favorite(id) => {
                    if let Some(mut g) = self.games.iter().find(|g| g.id == id).cloned() {
                        g.favorite = !g.favorite;
                        self.save_game(g);
                    }
                }
                Command::Launch(id) => {
                    if self.demo {
                        self.notify("Демонстрационная игра. Добавьте настоящую игру для запуска.");
                    } else if self.running.iter().any(|game| game.id == id) {
                        self.notify(
                            "Игра уже запущена. Вернитесь в неё или нажмите «Закрыть игру».",
                        );
                    } else if self.launching.is_none()
                        && let Some(g) = self.games.iter().find(|g| g.id == id)
                    {
                        if !g.installed {
                            self.commands.push(Command::Install(id));
                            continue;
                        }
                        if !g.available {
                            self.notify(
                                "Файлы игры недоступны. Подключите диск и обновите библиотеку.",
                            );
                            continue;
                        }
                        let target = g.target().clone();
                        let game = g.clone();
                        let played_tx = self.tx.clone();
                        let played_ctx = self.ctx.clone();
                        self.gamepad_suspended = true;
                        self.wallpaper_service.update(None);
                        self.launching = Some(id.clone());
                        self.background(move || {
                            let result = platform::launch_receipt(&target).map(|receipt| {
                                if crate::activity::receipt_is_game(&game, &receipt) {
                                    let _ = played_tx
                                        .send(Event::Played(game.id.clone(), receipt.started));
                                    played_ctx.request_repaint();
                                }
                            });
                            Event::Launch(id, result.map_err(|e| format!("{e:#}")))
                        });
                    }
                }
                Command::CloseGame(id) => {
                    if self.closing.contains(&id) {
                        continue;
                    }
                    if let Some(game) = self.games.iter().find(|g| g.id == id).cloned()
                        && let Some(running) = self.running.iter().find(|r| r.id == id).cloned()
                    {
                        self.closing.insert(id.clone());
                        self.background(move || {
                            Event::Closed(
                                id,
                                crate::activity::close_game(&game, &running.processes)
                                    .map_err(|e| format!("{e:#}")),
                            )
                        });
                    } else {
                        self.notify("Игра уже закрыта или её процесс ещё не обнаружен");
                    }
                }
                Command::Install(id) => {
                    if let Some(target) = self
                        .games
                        .iter()
                        .find(|g| g.id == id)
                        .and_then(|g| g.install_target.clone())
                    {
                        self.gamepad_suspended = true;
                        self.wallpaper_service.update(None);
                        self.background(move || {
                            Event::InstallSent(
                                platform::launch(&target).map_err(|e| format!("{e:#}")),
                            )
                        });
                    } else {
                        self.notify("Для этой игры нет установщика магазина");
                    }
                }
                Command::LibraryMode(all) => {
                    self.settings.show_uninstalled = all;
                    self.nav.reset();
                    self.start_scan();
                    if all {
                        self.start_accounts(true);
                    }
                }
                Command::Connect(source) => {
                    if self.demo {
                        self.notify("Подключение аккаунтов доступно в обычном режиме");
                    } else if !self.account_busy.contains(&source) {
                        self.background(move || {
                            Event::AccountOpened(
                                source,
                                platform::open_account_login(source).map_err(|e| e.to_string()),
                            )
                        });
                    }
                }
                Command::Disconnect(source) => {
                    if !self.account_busy.contains(&source) && !self.demo {
                        match platform::save_library_token(source, "") {
                            Ok(()) => {
                                self.account_connected.remove(&source);
                                self.account_status.remove(&source);
                                match self.db.merge_library(vec![], &[source], true) {
                                    Ok(games) => self.games = games,
                                    Err(e) => self.notify(e.to_string()),
                                }
                            }
                            Err(e) => self.notify(format!("Не удалось отключить: {e}")),
                        }
                    }
                }
                Command::Edit(id) => {
                    if let Some(g) = self.games.iter().find(|g| g.id == id) {
                        let target = g.target();
                        self.edit = Some(Edit {
                            game: g.clone(),
                            name: g.name().into(),
                            path: target.path.clone(),
                            args: target.args.clone(),
                            cwd: target.cwd.clone(),
                            kind: target.kind.clone(),
                        });
                        self.nav.reset();
                    }
                }
                Command::SaveEdit => {
                    if let Some(e) = self.edit.take() {
                        if e.name.trim().is_empty() || e.path.trim().is_empty() {
                            self.notify("Название и путь не могут быть пустыми");
                            self.edit = Some(e);
                        } else {
                            let mut g = e.game;
                            g.user_title = Some(e.name.trim().into());
                            let kind = if e.path.contains("://") {
                                LaunchKind::Uri
                            } else if e.path.to_lowercase().ends_with(".lnk") {
                                LaunchKind::ShellLink
                            } else {
                                LaunchKind::Executable
                            };
                            g.user_launch = Some(LaunchTarget {
                                kind,
                                path: e.path,
                                args: e.args,
                                cwd: e.cwd,
                                resolved_key: None,
                            });
                            g.process_path = match g.target().kind {
                                LaunchKind::Executable => Some(PathBuf::from(&g.target().path)),
                                LaunchKind::ShellLink => {
                                    platform::read_shortcut(Path::new(&g.target().path))
                                        .ok()
                                        .and_then(|s| s.executable)
                                }
                                LaunchKind::Uri => None,
                            };
                            self.save_game(g);
                            self.nav.reset();
                            self.notify("Изменения сохранены");
                        }
                    }
                }
                Command::Keyboard(p) => self.open_keyboard(p),
                Command::ApplyText => self.apply_text(),
                Command::Scan => {
                    self.request_scan(true);
                    self.start_accounts(true);
                }
                Command::Desktop => self.load_desktop(),
                Command::ToggleDesktop(id) => {
                    if !self.desktop_selection.remove(&id) {
                        self.desktop_selection.insert(id);
                    }
                }
                Command::AddSelected => {
                    let chosen: Vec<_> = self
                        .desktop
                        .iter()
                        .filter(|g| self.desktop_selection.contains(&g.id))
                        .cloned()
                        .collect();
                    let n = chosen.len();
                    self.merge(chosen);
                    self.desktop_selection.clear();
                    self.change_page(Page::Library);
                    self.notify(format!("Добавлено игр: {n}"));
                    self.start_auto_art();
                }
                Command::Browse(purpose) => {
                    self.browser = Some(Browser {
                        purpose,
                        path: None,
                        entries: vec![],
                        busy: false,
                        token: 0,
                        error: None,
                    });
                    self.directory(None);
                }
                Command::BrowseTo(p) => self.directory(p),
                Command::Pick(path) => {
                    if let Some(b) = &self.browser {
                        let purpose = b.purpose.clone();
                        let cache = self.db.root.join("artwork");
                        match purpose {
                            Purpose::Wallpaper => {
                                self.wallpaper_epoch += 1;
                                let epoch = self.wallpaper_epoch;
                                let cache = self.db.root.join("wallpapers");
                                self.background(move || {
                                    Event::WallpaperImported(
                                        epoch,
                                        artwork::import_image(&path, &cache)
                                            .map_err(|e| e.to_string()),
                                    )
                                });
                            }
                            Purpose::EngineProject => {
                                self.wallpaper_epoch += 1;
                                let epoch = self.wallpaper_epoch;
                                self.background(move || {
                                    Event::ProjectValidated(
                                        epoch,
                                        wallpaper::validate_project(&path)
                                            .map(|()| path)
                                            .map_err(|e| format!("{e:#}")),
                                    )
                                });
                            }
                            Purpose::EngineExecutable => match wallpaper::validate_engine(&path) {
                                Ok(()) => {
                                    self.settings.wallpaper_engine = Some(path);
                                    self.wallpaper_revision += 1;
                                    self.browser = None;
                                }
                                Err(e) => self.notify(e.to_string()),
                            },
                            Purpose::Add => {
                                let cache = self.db.root.join("icons");
                                self.background(move || {
                                    Event::File(Box::new(
                                        discovery::from_file(&path, &cache)
                                            .map_err(|e| format!("{e:#}")),
                                    ))
                                });
                            }
                            Purpose::Cover(id) | Purpose::Hero(id) => {
                                let hero = matches!(b.purpose, Purpose::Hero(_));
                                self.background(move || {
                                    Event::ImageImported(
                                        id,
                                        hero,
                                        artwork::import_image(&path, &cache)
                                            .map_err(|e| e.to_string()),
                                    )
                                });
                            }
                        }
                    }
                }
                Command::Art(id, hero) => {
                    if let Some(g) = self.games.iter().find(|g| g.id == id) {
                        let name = g.name().to_owned();
                        self.request_epoch += 1;
                        let token = self.request_epoch;
                        self.art = Some(ArtDialog {
                            token,
                            game: id,
                            hero,
                            panel: ArtPanel::Searching,
                        });
                        self.nav.reset();
                        self.background(move || {
                            Event::ArtGames(
                                token,
                                SteamGrid::new()
                                    .and_then(|s| s.search(&name))
                                    .map_err(|e| e.to_string()),
                            )
                        });
                    }
                }
                Command::ArtGame(id) => {
                    if let Some(a) = &mut self.art {
                        self.request_epoch += 1;
                        a.token = self.request_epoch;
                        let token = a.token;
                        let hero = a.hero;
                        a.panel = ArtPanel::Loading;
                        let cache = self.db.root.join("artwork");
                        self.background(move || {
                            Event::ArtImages(
                                token,
                                (|| -> Result<Vec<_>> {
                                    let s = SteamGrid::new()?;
                                    s.images(id, hero)?
                                        .into_iter()
                                        .take(16)
                                        .map(|a| {
                                            let thumb = s.download(&a.thumb, &cache).ok();
                                            Ok((a, thumb))
                                        })
                                        .collect()
                                })()
                                .map_err(|e| e.to_string()),
                            )
                        });
                        self.nav.reset();
                    }
                }
                Command::ArtImage(url) => {
                    if let Some(a) = &mut self.art {
                        let id = a.game.clone();
                        let hero = a.hero;
                        a.panel = ArtPanel::Loading;
                        let cache = self.db.root.join("artwork");
                        self.background(move || {
                            Event::ArtSaved(
                                id,
                                hero,
                                SteamGrid::new()
                                    .and_then(|s| s.download(&url, &cache))
                                    .map_err(|e| e.to_string()),
                            )
                        });
                    }
                }
                Command::AutoArt => self.start_auto_art(),
                Command::StoreArt => {
                    self.settings.store_art = !self.settings.store_art;
                    if self.settings.store_art {
                        self.start_auto_art();
                    }
                }
                Command::Fps(n) => {
                    self.settings.fps = n;
                    self.settings.display_rate = false;
                }
                Command::DisplayRate => self.settings.display_rate = true,
                Command::Fullscreen => self.settings.fullscreen = !self.settings.fullscreen,
                Command::Scale(n) => self.settings.scale = n,
                Command::Motion => self.settings.reduced_motion = !self.settings.reduced_motion,
                Command::StartupSound => self.settings.startup_sound = !self.settings.startup_sound,
                Command::Autostart => match crate::autostart::set(!self.autostart_registered) {
                    Ok(()) => {
                        self.autostart_registered = crate::autostart::registered().unwrap_or(false);
                        self.notify(if self.autostart_registered {
                            "HITDROL добавлен в автозагрузку Windows"
                        } else {
                            "HITDROL убран из автозагрузки Windows"
                        });
                    }
                    Err(e) => {
                        self.autostart_registered = crate::autostart::registered().unwrap_or(false);
                        self.notify(format!("Автозапуск: {e}"));
                    }
                },
                Command::StartupSettings => {
                    if let Err(e) = crate::autostart::open_windows_settings() {
                        self.notify(e.to_string());
                    }
                }
                Command::StartupAnimation => {
                    self.settings.startup_animation = !self.settings.startup_animation
                }
                Command::Background(mode) => {
                    self.wallpaper_epoch += 1;
                    self.settings.background_mode = mode;
                    self.wallpaper_revision += 1;
                }
                Command::BackgroundDim(dim) => self.settings.background_dim = dim,
                Command::WallpaperFps(fps) => self.settings.wallpaper_fps = fps,
                Command::RetryWallpaper => self.wallpaper_revision += 1,
                Command::SaveKey => match platform::store_key(&self.api_key) {
                    Ok(()) => {
                        self.key_present = !self.api_key.trim().is_empty();
                        self.api_key.clear();
                        self.notify("Ключ сохранён в Windows");
                        self.start_auto_art();
                    }
                    Err(e) => self.notify(e.to_string()),
                },
                Command::ClearKey => match platform::store_key("") {
                    Ok(()) => {
                        self.key_present = false;
                        self.notify("Ключ удалён");
                    }
                    Err(e) => self.notify(e.to_string()),
                },
                Command::Exit => self.exit = true,
            }
        }
        if let Err(e) = self.db.save_settings(&self.settings) {
            self.notify(format!("Настройки не сохранены: {e}"));
        }
    }
}

use anyhow::Context;

impl UiApp {
    pub fn frame(&mut self, ctx: &egui::Context) {
        self.process_events();
        self.process_commands();
        self.textures.drain(ctx);
        let (frame, status) = self.wallpaper_service.drain();
        if let Some(status) = status {
            self.wallpaper_status = status;
        }
        if self.settings.background_mode != BackgroundMode::WallpaperEngine {
            self.wallpaper_texture = None;
            self.wallpaper_texture_project = None;
        } else if let Some((project, image)) = frame
            && self.settings.wallpaper_project.as_ref() == Some(&project)
        {
            if let Some(texture) = &mut self.wallpaper_texture {
                texture.set(image, egui::TextureOptions::LINEAR);
            } else {
                self.wallpaper_texture = Some(ctx.load_texture(
                    "animated-wallpaper",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
            self.wallpaper_texture_project = Some(project);
        }
        let background = match self.settings.background_mode {
            BackgroundMode::Default => None,
            BackgroundMode::Image => self.textures.get(self.settings.background_image.as_ref()),
            BackgroundMode::WallpaperEngine => self
                .wallpaper_texture
                .as_ref()
                .filter(|_| self.wallpaper_texture_project == self.settings.wallpaper_project)
                .map(|t| (t.id(), t.size_vec2()))
                .or_else(|| self.textures.get(self.settings.background_image.as_ref())),
        };
        let panel_fill = if let Some((texture, size)) = background {
            let painter = ctx.layer_painter(egui::LayerId::background());
            let rect = ctx.content_rect();
            painter.image(texture, rect, cover_uv(size, rect.size()), Color32::WHITE);
            painter.rect_filled(
                rect,
                0,
                Color32::from_rgba_unmultiplied(
                    9,
                    14,
                    22,
                    (self.settings.background_dim as u16 * 255 / 100) as u8,
                ),
            );
            Color32::TRANSPARENT
        } else {
            BG
        };
        let scope = if self.keyboard.is_some() {
            "keyboard".to_owned()
        } else if self.browser.is_some() {
            "browser".into()
        } else if let Some(a) = &self.art {
            format!("art:{:?}", std::mem::discriminant(&a.panel))
        } else if self.edit.is_some() {
            "edit".into()
        } else {
            format!("{:?}", self.page)
        };
        if scope != self.nav_scope {
            self.nav_scope = scope;
            self.nav.items.clear();
            self.accept = false;
        }
        self.nav.items.clear();
        let modal = self.keyboard.is_some()
            || self.browser.is_some()
            || self.edit.is_some()
            || self.art.is_some();
        let margin = if ctx.content_rect().width() > 1100. {
            36
        } else {
            22
        };
        egui::TopBottomPanel::top("header")
            .exact_height(82.)
            .frame(
                egui::Frame::new()
                    .fill(panel_fill)
                    .inner_margin(egui::Margin::symmetric(margin, 16)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(160., 44.), Sense::hover());
                    ui.painter().image(
                        self.logo.id(),
                        Rect::from_min_size(rect.min, vec2(44., 44.)),
                        Rect::from_min_max(Pos2::ZERO, pos2(1., 1.)),
                        Color32::WHITE,
                    );
                    ui.painter().text(
                        rect.left_center() + vec2(51., -7.),
                        Align2::LEFT_CENTER,
                        "HITDROL",
                        FontId::proportional(21.),
                        TEXT,
                    );
                    ui.painter().text(
                        rect.left_center() + vec2(51., 13.),
                        Align2::LEFT_CENTER,
                        crate::branding::RELEASE,
                        FontId::proportional(12.),
                        MUTED,
                    );
                    if !modal {
                        for (p, title) in [
                            (Page::Home, "Главная"),
                            (Page::Library, "Библиотека"),
                            (Page::Add, "Добавить"),
                            (Page::Settings, "Настройки"),
                        ] {
                            if self.button(
                                ui,
                                &format!("tab:{p:?}"),
                                title,
                                vec2(if p == Page::Library { 119. } else { 103. }, 44.),
                                self.page == p,
                            ) {
                                self.commands.push(Command::Page(p));
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !modal && self.button(ui, "exit", "×", vec2(44., 44.), false) {
                            self.commands.push(Command::Exit);
                        }
                        if self.scanning || self.auto_busy {
                            ui.label(egui::RichText::new("Обновление…").size(13.).color(MUTED));
                        }
                    });
                });
            });
        egui::TopBottomPanel::bottom("footer").exact_height(48.).frame(egui::Frame::new().fill(panel_fill).inner_margin(egui::Margin::symmetric(margin,10))).show(ctx,|ui|{
            ui.horizontal(|ui|{
                let hint=match self.method{InputMethod::Gamepad=>"A  Выбрать     B  Назад     LB / RB  Разделы     ≡  Действия",InputMethod::Touch=>"Нажмите, чтобы выбрать · Проведите, чтобы прокрутить",_=>"Enter  Выбрать     Esc  Назад     ← ↑ ↓ →  Навигация     F11  Полный экран"};
                ui.label(egui::RichText::new(hint).size(13.).color(MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{ui.label(egui::RichText::new(if self.demo{"ДЕМО"}else{"ВАШИ ИГРЫ. ВАШ РИТМ."}).size(11.).color(MUTED));});
            });
        });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(panel_fill)
                    .inner_margin(egui::Margin::symmetric(margin, 8)),
            )
            .show(ctx, |ui| {
                if self.keyboard.is_some() {
                    self.keyboard_view(ui);
                } else if self.browser.is_some() {
                    self.browser_view(ui);
                } else if self.art.is_some() {
                    self.art_view(ui);
                } else if self.edit.is_some() {
                    self.edit_view(ui);
                } else {
                    // Keep active games reachable from every normal page.
                    let running: Vec<_> = self
                        .running
                        .iter()
                        .filter_map(|r| {
                            self.games.iter().find(|g| g.id == r.id).map(|g| {
                                (g.id.clone(), format!("В игре: {}", ellipsize(g.name(), 28)))
                            })
                        })
                        .collect();
                    if !running.is_empty() && matches!(self.page, Page::Home | Page::Library) {
                        ui.horizontal_wrapped(|ui| {
                            for (id, title) in running {
                                if self.button(
                                    ui,
                                    &format!("running:{id}"),
                                    &title,
                                    vec2(290., 46.),
                                    false,
                                ) {
                                    self.commands.push(Command::Open(id));
                                }
                            }
                        });
                        ui.add_space(8.);
                    }
                    match self.page {
                        Page::Home => self.home_view(ui),
                        Page::Library => self.library_view(ui),
                        Page::Add => self.add_view(ui),
                        Page::Settings => self.settings_view(ui),
                        Page::Detail => self.detail_view(ui),
                    }
                }
            });
        if let Some((message, since)) = &self.notice
            && since.elapsed() < Duration::from_secs(6)
        {
            egui::Area::new(egui::Id::new("notice"))
                .anchor(Align2::RIGHT_BOTTOM, vec2(-24., -62.))
                .interactable(false)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(35, 49, 59))
                        .corner_radius(12)
                        .inner_margin(16)
                        .show(ui, |ui| {
                            ui.set_max_width(520.);
                            ui.label(egui::RichText::new(message).color(TEXT).size(15.));
                        });
                });
            if self.focused {
                ctx.request_repaint_after(Duration::from_secs(6).saturating_sub(since.elapsed()));
            }
        }
        self.nav.ensure_valid();
        self.nav.reveal = false;
        self.accept = false;
        self.process_commands();
    }
    fn register(&mut self, id: &str, rect: Rect) {
        self.nav.items.push(NavItem {
            id: id.into(),
            rect,
        });
    }
    fn button(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        label: &str,
        size: Vec2,
        primary: bool,
    ) -> bool {
        let selected = self.nav.focused.as_deref() == Some(id)
            && matches!(self.method, InputMethod::Gamepad | InputMethod::Keyboard);
        let stop = id == "play"
            && self
                .selected
                .as_ref()
                .is_some_and(|id| self.running.iter().any(|r| &r.id == id));
        let button = egui::Button::new(
            egui::RichText::new(label)
                .size(16.)
                .color(if primary && !stop { BG } else { TEXT }),
        )
        .sense(Sense::CLICK)
        .corner_radius(10)
        .stroke(if selected {
            Stroke::new(2_f32, if stop { STOP_HOVER } else { ACCENT })
        } else {
            Stroke::NONE
        });
        let fill = if stop {
            STOP
        } else if primary {
            ACCENT
        } else if selected {
            Color32::from_rgb(43, 62, 70)
        } else {
            PANEL
        };
        let response = if id == "play" {
            // Explicit Button::fill overrides egui's hover/pressed colours.
            // Let this button use its real interaction state instead.
            ui.scope(|ui| {
                let widgets = &mut ui.visuals_mut().widgets;
                let hover = if stop { STOP_HOVER } else { PLAY_HOVER };
                widgets.inactive.weak_bg_fill = if selected { hover } else { fill };
                widgets.hovered.weak_bg_fill = hover;
                widgets.active.weak_bg_fill = if stop { STOP_PRESSED } else { PLAY_PRESSED };
                ui.add_sized(size, button)
            })
            .inner
        } else {
            ui.add_sized(size, button.fill(fill))
        };
        // Navigation owns button focus; egui text fields retain their editing focus.
        response.surrender_focus();
        self.register(id, response.rect);
        if selected && self.nav.reveal {
            response.scroll_to_me(Some(egui::Align::Center));
        }
        let accepted = selected && self.accept;
        if accepted {
            self.accept = false;
        }
        if response.clicked() {
            self.nav.focused = Some(id.into());
        }
        response.clicked() || accepted
    }
    fn heading(ui: &mut egui::Ui, title: &str, subtitle: &str) {
        ui.label(egui::RichText::new(title).size(32.).color(TEXT));
        if !subtitle.is_empty() {
            ui.label(egui::RichText::new(subtitle).size(15.).color(MUTED));
        }
        ui.add_space(16.);
    }
    fn back_button(&mut self, ui: &mut egui::Ui) {
        if self.button(ui, "back", "‹  Назад", vec2(112., 44.), false) {
            self.commands.push(Command::Back);
        }
        ui.add_space(12.);
    }
    fn visible_games(&self) -> Vec<Game> {
        let q = self.search.to_lowercase();
        let mut games: Vec<_> = self
            .games
            .iter()
            .filter(|g| {
                (self.settings.show_uninstalled || g.installed)
                    && (g.installed || g.owned || g.source == Source::Manual)
                    && (!self.favorites || g.favorite)
                    && self.filter.is_none_or(|s| g.source == s)
                    && (q.is_empty() || g.name().to_lowercase().contains(&q))
            })
            .cloned()
            .collect();
        games.sort_by(|a, b| {
            (if b.history_verified { b.last_played } else { 0 })
                .cmp(&(if a.history_verified { a.last_played } else { 0 }))
                .then_with(|| a.name().to_lowercase().cmp(&b.name().to_lowercase()))
        });
        games
    }
    fn home_view(&mut self, ui: &mut egui::Ui) {
        let games = recent_games(&self.games, self.settings.show_uninstalled);
        if games.is_empty() {
            if self.games.is_empty() {
                self.empty_view(ui);
            } else {
                Self::heading(
                    ui,
                    "Пока нет недавних запусков",
                    "Здесь появятся действительно запущенные игры. История Steam импортируется автоматически.",
                );
                if self.button(ui, "all-games", "Открыть библиотеку", vec2(250., 56.), true)
                {
                    self.commands.push(Command::Page(Page::Library));
                }
            }
            return;
        }
        let selected = self
            .selected
            .as_ref()
            .and_then(|id| games.iter().find(|g| &g.id == id))
            .unwrap_or(&games[0])
            .clone();
        let hero_h = (ui.available_height() * 0.28).clamp(115., 205.);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), hero_h), Sense::hover());
        self.paint_backdrop(ui, rect, &selected);
        let p = ui.painter();
        p.text(
            rect.min + vec2(24., 24.),
            Align2::LEFT_TOP,
            "С ВОЗВРАЩЕНИЕМ",
            FontId::proportional(12.),
            ACCENT,
        );
        let title = ellipsize(selected.name(), 48);
        p.text(
            rect.min + vec2(22., 52.),
            Align2::LEFT_TOP,
            title,
            FontId::proportional(if rect.width() > 900. { 38. } else { 30. }),
            TEXT,
        );
        p.text(
            rect.min + vec2(24., hero_h - 40.),
            Align2::LEFT_TOP,
            format!(
                "{}    ·    Последний запуск: {}",
                selected.source.label(),
                played_label(selected.last_played)
            ),
            FontId::proportional(15.),
            MUTED,
        );
        ui.add_space(12.);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Недавние игры").size(22.).color(TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.button(ui, "all-games", "Вся библиотека  →", vec2(188., 42.), false)
                {
                    self.commands.push(Command::Page(Page::Library));
                }
            });
        });
        ui.add_space(12.);
        self.grid(ui, &games, "home-grid", true);
    }
    fn empty_view(&mut self, ui: &mut egui::Ui) {
        ui.add_space((ui.available_height() * 0.16).max(20.));
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("Место для ваших игр.")
                    .size(42.)
                    .color(TEXT),
            );
            ui.add_space(16.);
            ui.label(
                egui::RichText::new("Steam, Epic, GOG и любимые игры с рабочего стола.")
                    .size(18.)
                    .color(MUTED),
            );
            ui.add_space(28.);
            if self.button(ui, "empty-add", "+  Добавить игру", vec2(240., 58.), true) {
                self.commands.push(Command::Page(Page::Add));
            }
            ui.add_space(12.);
            if self.button(
                ui,
                "empty-scan",
                if self.scanning {
                    "Поиск игр…"
                } else {
                    "Обновить библиотеку"
                },
                vec2(240., 52.),
                false,
            ) {
                self.commands.push(Command::Scan);
            }
        });
    }
    fn library_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            Self::heading(
                ui,
                "Библиотека",
                &format!(
                    "{} · {}",
                    self.visible_games().len(),
                    if self.settings.show_uninstalled {
                        "вся библиотека"
                    } else {
                        "установленные игры"
                    }
                ),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.button(ui, "refresh", "Обновить", vec2(115., 46.), false) {
                    self.commands.push(Command::Scan);
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            let label = if self.search.is_empty() {
                "Поиск игры…".into()
            } else {
                format!("Поиск: {}", ellipsize(&self.search, 22))
            };
            if self.button(ui, "search", &label, vec2(235., 46.), false) {
                self.commands.push(Command::Keyboard(TextPurpose::Search));
            }
            if !self.search.is_empty()
                && self.button(ui, "clear-search", "×", vec2(44., 46.), false)
            {
                self.search.clear();
            }
            if self.button(
                ui,
                "filter-all",
                "Все",
                vec2(68., 46.),
                self.filter.is_none() && !self.favorites,
            ) {
                self.filter = None;
                self.favorites = false;
            }
            if self.button(
                ui,
                "favorites",
                "Избранное",
                vec2(120., 46.),
                self.favorites,
            ) {
                self.favorites = !self.favorites;
            }
            for source in Source::ALL {
                if self.button(
                    ui,
                    &format!("source:{source:?}"),
                    source.label(),
                    vec2(105., 46.),
                    self.filter == Some(source),
                ) {
                    self.filter = if self.filter == Some(source) {
                        None
                    } else {
                        Some(source)
                    };
                }
            }
        });
        ui.add_space(20.);
        let games = self.visible_games();
        if games.is_empty() {
            ui.label(
                egui::RichText::new("Ничего не найдено. Измените поиск или добавьте игру.")
                    .size(18.)
                    .color(MUTED),
            );
        } else {
            self.grid(ui, &games, "library-grid", false);
        }
    }
    fn grid(&mut self, ui: &mut egui::Ui, games: &[Game], salt: &str, home: bool) {
        let width = ui.available_width();
        let columns = ((width + 18.) / if home { 190. } else { 180. })
            .floor()
            .max(2.) as usize;
        let card_w = (width - 18. * (columns - 1) as f32) / columns as f32;
        let card_h = if home {
            (ui.available_height() - 62.).clamp(100., card_w * 1.35)
        } else {
            card_w * 1.35
        };
        let stride = card_h + 62.;
        let rows = games.len().div_ceil(columns);
        egui::ScrollArea::vertical()
            .id_salt(salt)
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let (_, whole) = ui.allocate_space(vec2(width, rows as f32 * stride));
                for (i, game) in games.iter().enumerate() {
                    let row = i / columns;
                    let col = i % columns;
                    let rect = Rect::from_min_size(
                        whole.min + vec2(col as f32 * (card_w + 18.), row as f32 * stride),
                        vec2(card_w, card_h),
                    );
                    let id = format!("game:{}", game.id);
                    self.register(&id, rect);
                    let selected = self.nav.focused.as_deref() == Some(&id);
                    if selected && self.nav.reveal {
                        ui.scroll_to_rect(rect.expand(8.), Some(egui::Align::Center));
                    }
                    if row as f32 * stride > viewport.max.y + stride
                        || (row as f32 + 1.) * stride < viewport.min.y - stride
                    {
                        continue;
                    }
                    let response = ui.interact(rect, egui::Id::new((salt, &game.id)), Sense::CLICK);
                    response.surrender_focus();
                    self.paint_card(ui, rect, game, selected || response.hovered());
                    let p = ui.painter();
                    p.text(
                        rect.left_bottom() + vec2(1., 12.),
                        Align2::LEFT_TOP,
                        ellipsize(game.name(), (card_w / 8.) as usize),
                        FontId::proportional(16.),
                        TEXT,
                    );
                    p.text(
                        rect.left_bottom() + vec2(1., 35.),
                        Align2::LEFT_TOP,
                        if !game.installed {
                            "Не установлена"
                        } else if game.available {
                            game.source.label()
                        } else {
                            "Диск недоступен"
                        },
                        FontId::proportional(12.),
                        MUTED,
                    );
                    if response.clicked() || selected && self.accept {
                        self.nav.focused = Some(id.clone());
                        self.accept = false;
                        self.commands.push(Command::Open(game.id.clone()));
                    }
                    if response.hovered() && self.method == InputMethod::Mouse {
                        self.selected = Some(game.id.clone());
                    }
                }
            });
    }
    fn paint_card(&mut self, ui: &egui::Ui, rect: Rect, game: &Game, selected: bool) {
        let p = ui.painter();
        let hash = stable_hash(&game.id);
        let hue = u8::from_str_radix(&hash[..2], 16).unwrap_or(90);
        let tone = Color32::from_rgb(24 + hue / 5, 38 + hue / 7, 54 + hue / 8);
        p.rect_filled(rect, 12, tone);
        if let Some((id, size, index)) = self.textures.get_any([
            game.user_cover.as_ref(),
            game.cover.as_ref(),
            game.icon.as_ref(),
        ]) {
            if index == 2 {
                let r = Rect::from_center_size(rect.center() - vec2(0., 20.), vec2(64., 64.));
                p.image(
                    id,
                    r,
                    Rect::from_min_max(Pos2::ZERO, pos2(1., 1.)),
                    Color32::WHITE,
                );
                p.text(
                    rect.center() + vec2(0., 38.),
                    Align2::CENTER_CENTER,
                    ellipsize(game.name(), 18),
                    FontId::proportional(18.),
                    TEXT,
                );
            } else {
                p.image(id, rect, cover_uv(size, rect.size()), Color32::WHITE);
            }
        } else {
            let clipped = p.with_clip_rect(rect.shrink(1.));
            let center = rect.center() + vec2(rect.width() * 0.24, -rect.height() * 0.12);
            clipped.circle_stroke(
                center,
                rect.width() * 0.65,
                Stroke::new(1.5_f32, Color32::from_white_alpha(35)),
            );
            clipped.circle_stroke(
                center,
                rect.width() * 0.44,
                Stroke::new(1_f32, Color32::from_white_alpha(25)),
            );
            clipped.line_segment(
                [
                    rect.left_bottom() - vec2(0., 30.),
                    rect.right_top() + vec2(0., 40.),
                ],
                Stroke::new(1_f32, Color32::from_white_alpha(25)),
            );
            clipped.text(
                rect.min + vec2(17., 18.),
                Align2::LEFT_TOP,
                format!("{:02}", hue % 99),
                FontId::proportional(11.),
                ACCENT,
            );
            let title = game
                .name()
                .split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join("\n");
            clipped.text(
                rect.left_bottom() + vec2(16., -22.),
                Align2::LEFT_BOTTOM,
                title,
                FontId::proportional((rect.width() / 8.).clamp(16., 26.)),
                TEXT,
            );
        }
        if game.favorite {
            p.circle_filled(rect.right_top() + vec2(-20., 20.), 13., BG);
            p.text(
                rect.right_top() + vec2(-20., 20.),
                Align2::CENTER_CENTER,
                "★",
                FontId::proportional(15.),
                ACCENT,
            );
        }
        if selected {
            p.rect_stroke(
                rect.shrink(1.25),
                10,
                Stroke::new(2.5_f32, ACCENT),
                egui::StrokeKind::Inside,
            );
        }
    }
    fn paint_backdrop(&mut self, ui: &egui::Ui, rect: Rect, game: &Game) {
        let p = ui.painter();
        p.rect_filled(rect, 18, PANEL);
        if let Some((id, size, _)) = self.textures.get_any([
            game.user_hero.as_ref(),
            game.hero.as_ref(),
            game.user_cover.as_ref(),
            game.cover.as_ref(),
        ]) {
            p.with_clip_rect(rect).image(
                id,
                rect,
                cover_uv(size, rect.size()),
                Color32::from_white_alpha(105),
            );
        } else {
            let paint = p.with_clip_rect(rect);
            for i in 0..5 {
                paint.circle_stroke(
                    rect.right_center() - vec2(80., 0.),
                    90. + i as f32 * 43.,
                    Stroke::new(1_f32, Color32::from_rgba_unmultiplied(101, 210, 255, 15)),
                );
            }
        }
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(
            rect.left_top(),
            Color32::from_rgba_unmultiplied(15, 23, 32, 245),
        );
        mesh.colored_vertex(
            rect.right_top(),
            Color32::from_rgba_unmultiplied(15, 23, 32, 40),
        );
        mesh.colored_vertex(
            rect.right_bottom(),
            Color32::from_rgba_unmultiplied(15, 23, 32, 100),
        );
        mesh.colored_vertex(
            rect.left_bottom(),
            Color32::from_rgba_unmultiplied(15, 23, 32, 245),
        );
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        p.add(egui::Shape::mesh(mesh));
    }
    fn detail_view(&mut self, ui: &mut egui::Ui) {
        self.back_button(ui);
        let Some(game) = self
            .selected
            .as_ref()
            .and_then(|id| self.games.iter().find(|g| &g.id == id))
            .cloned()
        else {
            self.empty_view(ui);
            return;
        };
        egui::ScrollArea::vertical()
            .id_salt("detail")
            .show(ui, |ui| {
                let banner_h = (ui.available_height() * 0.45).clamp(150., 290.);
                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), banner_h), Sense::hover());
                self.paint_backdrop(ui, rect, &game);
                ui.painter().text(
                    rect.min + vec2(28., 30.),
                    Align2::LEFT_TOP,
                    game.source.label().to_uppercase(),
                    FontId::proportional(13.),
                    ACCENT,
                );
                ui.painter().text(
                    rect.left_bottom() + vec2(26., -30.),
                    Align2::LEFT_BOTTOM,
                    ellipsize(game.name(), 45),
                    FontId::proportional(38.),
                    TEXT,
                );
                ui.add_space(24.);
                let running = self.running.iter().any(|r| r.id == game.id);
                ui.horizontal_wrapped(|ui| {
                    if self.button(
                        ui,
                        "play",
                        if running {
                            if self.closing.contains(&game.id) { "Закрываем…" } else { "Закрыть игру" }
                        } else if self.launching.is_some() {
                            "Запускается…"
                        } else if !game.installed {
                            "↓  Установить"
                        } else if !game.available {
                            "Файлы недоступны"
                        } else {
                            "▶  Играть"
                        },
                        vec2(215., 60.),
                        true,
                    ) {
                        self.commands.push(if running { Command::CloseGame(game.id.clone()) }
                            else { Command::Launch(game.id.clone()) });
                    }
                    if self.button(
                        ui,
                        "favorite",
                        if game.favorite {
                            "★  В избранном"
                        } else {
                            "☆  В избранное"
                        },
                        vec2(184., 60.),
                        false,
                    ) {
                        self.commands.push(Command::Favorite(game.id.clone()));
                    }
                    if self.button(ui, "edit", "Изменить", vec2(142., 60.), false) {
                        self.commands.push(Command::Edit(game.id.clone()));
                    }
                });
                ui.add_space(25.);
                ui.label(egui::RichText::new("Оформление").size(20.).color(TEXT));
                ui.add_space(12.);
                ui.horizontal_wrapped(|ui| {
                    for (id, label, command) in [
                        (
                            "cover-file",
                            "Обложка из файла",
                            Command::Browse(Purpose::Cover(game.id.clone())),
                        ),
                        (
                            "hero-file",
                            "Фон из файла",
                            Command::Browse(Purpose::Hero(game.id.clone())),
                        ),
                        (
                            "cover-online",
                            "Найти обложку",
                            Command::Art(game.id.clone(), false),
                        ),
                        (
                            "hero-online",
                            "Найти фон",
                            Command::Art(game.id.clone(), true),
                        ),
                    ] {
                        if self.button(ui, id, label, vec2(190., 48.), false) {
                            self.commands.push(command);
                        }
                    }
                });
                if !game.available {
                    ui.add_space(18.);
                    ui.colored_label(
                        ACCENT,
                        if !game.installed {"Игра ещё не установлена. Нажмите «Установить», чтобы открыть её в клиенте магазина."} else {"Папка игры недоступна. Подключите диск и обновите библиотеку."},
                    );
                }
            });
    }
}

fn configure(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(data) = std::fs::read("C:/Windows/Fonts/segoeui.ttf") {
        fonts
            .font_data
            .insert("segoe".into(), egui::FontData::from_owned(data).into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "segoe".into());
    }
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.panel_fill = BG;
    style.visuals.selection.bg_fill = ACCENT;
    style.visuals.selection.stroke = Stroke::new(1_f32, BG);
    style.spacing.item_spacing = vec2(10., 10.);
    style.spacing.button_padding = vec2(14., 10.);
    style.spacing.interact_size = vec2(48., 48.);
    style.animation_time = 0.13;
    ctx.set_style(style);
}
fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.into()
    } else {
        format!(
            "{}…",
            s.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}
fn cover_uv(image: Vec2, target: Vec2) -> Rect {
    let ir = image.x / image.y;
    let tr = target.x / target.y;
    if ir > tr {
        let w = tr / ir;
        Rect::from_min_max(pos2((1. - w) / 2., 0.), pos2((1. + w) / 2., 1.))
    } else {
        let h = ir / tr;
        Rect::from_min_max(pos2(0., (1. - h) / 2.), pos2(1., (1. + h) / 2.))
    }
}

pub fn demo_games(count: usize) -> Vec<Game> {
    let names = [
        "Hollow Knight",
        "No Man’s Sky",
        "Forza Horizon 5",
        "Celeste",
        "The Witcher 3",
        "Hades",
        "Outer Wilds",
        "Dead Cells",
        "Stray",
        "Tunic",
        "Ori and the Will of the Wisps",
        "Disco Elysium",
    ];
    (0..count)
        .map(|i| {
            let name = if i < names.len() {
                names[i].into()
            } else {
                format!("Игра {:04}", i + 1)
            };
            let mut g = Game::new(
                Source::ALL[(i + 1) % 4],
                format!("demo-{i}"),
                name,
                LaunchTarget::uri("steam://run/0"),
            );
            g.favorite = i % 4 == 0;
            g.last_played = timestamp().saturating_sub(i as u64 * 3600);
            g.history_verified = true;
            g
        })
        .collect()
}

impl UiApp {
    fn add_view(&mut self, ui: &mut egui::Ui) {
        Self::heading(
            ui,
            "Добавить игры",
            "Выберите игры с рабочего стола. Можно добавить несколько сразу.",
        );
        let count = self.desktop_selection.len();
        let bottom = ui.max_rect().bottom();
        let footer = Rect::from_min_max(
            pos2(ui.max_rect().left(), bottom - 65.),
            ui.max_rect().right_bottom(),
        );
        let list_height = (ui.available_height() - 82.).max(60.);
        if self.desktop_busy {
            ui.label(egui::RichText::new("Читаем ярлыки рабочего стола…").color(MUTED));
        } else if self.desktop.is_empty() {
            ui.label(
                egui::RichText::new("На рабочем столе нет подходящих ярлыков. Нажмите «Другое».")
                    .color(MUTED),
            );
        }
        let items = self.desktop.clone();
        egui::ScrollArea::vertical()
            .id_salt("desktop-list")
            .max_height(list_height)
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let width = ui.available_width();
                let (_, whole) = ui.allocate_space(vec2(width, items.len() as f32 * 70.));
                for (i, game) in items.iter().enumerate() {
                    let rect =
                        Rect::from_min_size(whole.min + vec2(0., i as f32 * 70.), vec2(width, 60.));
                    let id = format!("desktop:{}", game.id);
                    self.register(&id, rect);
                    let focused = self.nav.focused.as_deref() == Some(&id);
                    if focused && self.nav.reveal {
                        ui.scroll_to_rect(rect, Some(egui::Align::Center));
                    }
                    if i as f32 * 70. > viewport.max.y + 70.
                        || (i + 1) as f32 * 70. < viewport.min.y - 70.
                    {
                        continue;
                    }
                    let checked = self.desktop_selection.contains(&game.id);
                    let response = ui.interact(rect, egui::Id::new(&id), Sense::CLICK);
                    response.surrender_focus();
                    ui.painter().rect_filled(
                        rect,
                        10,
                        if checked {
                            Color32::from_rgb(34, 59, 61)
                        } else {
                            PANEL
                        },
                    );
                    ui.painter().text(
                        rect.left_center() + vec2(18., 0.),
                        Align2::LEFT_CENTER,
                        if checked { "✓" } else { "+" },
                        FontId::proportional(23.),
                        ACCENT,
                    );
                    ui.painter().text(
                        rect.left_center() + vec2(54., 0.),
                        Align2::LEFT_CENTER,
                        ellipsize(game.name(), (width / 9.) as usize - 8),
                        FontId::proportional(18.),
                        TEXT,
                    );
                    if focused {
                        ui.painter().rect_stroke(
                            rect,
                            10,
                            Stroke::new(2_f32, ACCENT),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if response.clicked() || focused && self.accept {
                        self.nav.focused = Some(id.clone());
                        self.accept = false;
                        self.commands.push(Command::ToggleDesktop(game.id.clone()));
                    }
                }
            });
        ui.scope_builder(egui::UiBuilder::new().max_rect(footer), |ui| {
            ui.horizontal(|ui| {
                if self.button(
                    ui,
                    "desktop-refresh",
                    "Обновить список",
                    vec2(180., 54.),
                    false,
                ) {
                    self.commands.push(Command::Desktop);
                }
                if count > 0
                    && self.button(
                        ui,
                        "desktop-add",
                        &format!("Добавить выбранные · {count}"),
                        vec2(270., 54.),
                        true,
                    )
                {
                    self.commands.push(Command::AddSelected);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.button(ui, "other", "Другое  →", vec2(155., 54.), false) {
                        self.commands.push(Command::Browse(Purpose::Add));
                    }
                });
            });
        });
    }
    fn browser_view(&mut self, ui: &mut egui::Ui) {
        self.back_button(ui);
        let Some(b) = &self.browser else {
            return;
        };
        let path = b.path.clone();
        let entries = b.entries.clone();
        let busy = b.busy;
        let error = b.error.clone();
        let (title, hint) = match b.purpose {
            Purpose::Add => (
                "Найдём вашу игру",
                "Выберите исполняемый файл или ярлык игры",
            ),
            Purpose::EngineExecutable => (
                "Где установлен Wallpaper Engine?",
                "Выберите wallpaper64.exe или wallpaper32.exe; в новых версиях — папка distribution",
            ),
            Purpose::EngineProject => (
                "Обои Wallpaper Engine",
                "Установленные обои ниже. Через диски можно выбрать project.json, MP4, WebM или HTML",
            ),
            _ => ("Выберите изображение", "PNG, JPEG, WebP или ICO"),
        };
        Self::heading(ui, title, hint);
        ui.horizontal(|ui| {
            if self.button(ui, "drives", "Диски", vec2(100., 46.), path.is_none()) {
                self.commands.push(Command::BrowseTo(None));
            }
            if let Some(p) = &path {
                if self.button(ui, "parent", "↑  Выше", vec2(105., 46.), false) {
                    self.commands
                        .push(Command::BrowseTo(p.parent().map(Path::to_path_buf)));
                }
                ui.label(
                    egui::RichText::new(ellipsize(&p.display().to_string(), 70))
                        .size(15.)
                        .color(MUTED),
                );
            } else {
                ui.label(egui::RichText::new("Этот компьютер").color(MUTED));
            }
        });
        ui.add_space(14.);
        if busy {
            ui.label("Открываем папку…");
            return;
        }
        if let Some(e) = error {
            ui.colored_label(ACCENT, e);
            return;
        }
        if entries.is_empty() {
            ui.label(egui::RichText::new("В этой папке нет подходящих файлов.").color(MUTED));
        }
        egui::ScrollArea::vertical()
            .id_salt(("browser", &path))
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let width = ui.available_width();
                let (_, whole) = ui.allocate_space(vec2(width, entries.len() as f32 * 66.));
                for (i, e) in entries.iter().enumerate() {
                    let rect =
                        Rect::from_min_size(whole.min + vec2(0., i as f32 * 66.), vec2(width, 56.));
                    let id = format!("file:{}", e.path.display());
                    self.register(&id, rect);
                    let focus = self.nav.focused.as_deref() == Some(&id);
                    if focus && self.nav.reveal {
                        ui.scroll_to_rect(rect, Some(egui::Align::Center));
                    }
                    if i as f32 * 66. > viewport.max.y + 66.
                        || (i + 1) as f32 * 66. < viewport.min.y - 66.
                    {
                        continue;
                    }
                    let response = ui.interact(rect, egui::Id::new(&id), Sense::CLICK);
                    response.surrender_focus();
                    ui.painter().rect_filled(
                        rect,
                        9,
                        if focus {
                            Color32::from_rgb(34, 59, 61)
                        } else {
                            PANEL
                        },
                    );
                    ui.painter().text(
                        rect.left_center() + vec2(18., 0.),
                        Align2::LEFT_CENTER,
                        if e.directory { "›" } else { "+" },
                        FontId::proportional(23.),
                        ACCENT,
                    );
                    ui.painter().text(
                        rect.left_center() + vec2(53., 0.),
                        Align2::LEFT_CENTER,
                        ellipsize(&e.name, (width / 9.) as usize - 7),
                        FontId::proportional(17.),
                        TEXT,
                    );
                    if response.clicked() || focus && self.accept {
                        self.accept = false;
                        self.commands.push(if e.directory {
                            Command::BrowseTo(Some(e.path.clone()))
                        } else {
                            Command::Pick(e.path.clone())
                        });
                    }
                }
            });
    }
    fn settings_view(&mut self, ui: &mut egui::Ui) {
        Self::heading(ui, "Настройки", "Устройтесь поудобнее.");
        egui::ScrollArea::vertical()
            .id_salt("settings")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                ui.label(egui::RichText::new("Библиотека").size(22.));
                ui.horizontal_wrapped(|ui| {
                    for (all,label,width) in [(false,"Только установленные",245.),(true,"Вся библиотека",200.)] {
                        if self.button(ui,&format!("library-mode:{all}"),label,vec2(width,50.),self.settings.show_uninstalled==all){self.commands.push(Command::LibraryMode(all));}
                    }
                });
                ui.add(egui::Label::new(egui::RichText::new("У неустановленной игры появится кнопка «Установить». Загрузкой и выбором диска управляет клиент магазина.").color(MUTED)).wrap());
                ui.label(egui::RichText::new("Steam: библиотека текущего аккаунта читается из локального кэша лицензий. Войдите в Steam и откройте его библиотеку хотя бы один раз.").color(MUTED));
                for warning in self.warnings.iter().filter(|s|s.contains("Кэш Steam")||s.starts_with("Вся библиотека Steam")) {ui.colored_label(ACCENT,warning);}
                if self.settings.show_uninstalled {
                    for source in [Source::Epic,Source::Gog] {
                        let connected=self.account_connected.contains(&source);
                        let busy=self.account_busy.contains(&source);
                        ui.add_space(8.);
                        ui.label(egui::RichText::new(source.label()).size(18.));
                        let status=self.account_status.get(&source).cloned().unwrap_or_else(||if connected {"Аккаунт подключён".into()}else{"Для всей библиотеки подключите аккаунт".into()});
                        ui.label(egui::RichText::new(status).color(MUTED));
                        if !busy {
                            ui.horizontal_wrapped(|ui| {
                                if self.button(ui,&format!("connect:{source:?}"),if connected{"Войти заново"}else{"Подключить аккаунт"},vec2(220.,48.),false){self.commands.push(Command::Connect(source));}
                                if connected && self.button(ui,&format!("disconnect:{source:?}"),"Отключить",vec2(155.,48.),false){self.commands.push(Command::Disconnect(source));}
                            });
                        }
                    }
                    ui.add_space(8.);
                    ui.label(egui::RichText::new("Вход выполняется на сайте магазина. После входа вернитесь сюда: Epic — скопируйте authorizationCode из результата; GOG — адрес открывшейся страницы. Пароль вводится только в браузере. Для установки нужен клиент того же магазина и аккаунта.").color(MUTED));
                }
                ui.add_space(24.);
                ui.label(egui::RichText::new("Обои лаунчера").size(22.).color(TEXT));
                ui.add_space(10.);
                ui.horizontal_wrapped(|ui| {
                    for (mode,label,width) in [
                        (BackgroundMode::Default,"Стандартный фон",190.),
                        (BackgroundMode::Image,"Свои обои",150.),
                        (BackgroundMode::WallpaperEngine,"Wallpaper Engine",210.),
                    ] {
                        if self.button(ui,&format!("background:{mode:?}"),label,vec2(width,50.),self.settings.background_mode == mode) {
                            self.commands.push(Command::Background(mode));
                            if mode == BackgroundMode::Image && self.settings.background_image.is_none() { self.commands.push(Command::Browse(Purpose::Wallpaper)); }
                            if mode == BackgroundMode::WallpaperEngine && self.settings.wallpaper_project.is_none() { self.commands.push(Command::Browse(Purpose::EngineProject)); }
                        }
                    }
                });
                ui.add_space(8.);
                if self.settings.background_mode == BackgroundMode::Image {
                    if self.button(ui,"wallpaper-image","Выбрать изображение",vec2(245.,50.),false) { self.commands.push(Command::Browse(Purpose::Wallpaper)); }
                    ui.label(egui::RichText::new("Изображение сохраняется в профиле HITDROL. Исходный файл можно переместить.").color(MUTED));
                }
                if self.settings.background_mode == BackgroundMode::WallpaperEngine {
                    ui.horizontal_wrapped(|ui| {
                        if self.button(ui,"wallpaper-project","Выбрать живые обои",vec2(230.,50.),false) { self.commands.push(Command::Browse(Purpose::EngineProject)); }
                        if self.button(ui,"wallpaper-retry","Повторить подключение",vec2(245.,50.),false) { self.commands.push(Command::RetryWallpaper); }
                        if self.button(ui,"wallpaper-exe","Папка Wallpaper Engine",vec2(240.,50.),false) { self.commands.push(Command::Browse(Purpose::EngineExecutable)); }
                    });
                    if self.settings.wallpaper_engine.is_none() { ui.colored_label(ACCENT,"Wallpaper Engine не найден. Установите его в Steam или укажите папку."); }
                    if let Some(path) = &self.settings.wallpaper_project {
                        ui.label(egui::RichText::new(ellipsize(&path.display().to_string(),90)).size(13.).color(MUTED));
                    }
                    ui.label(egui::RichText::new(&self.wallpaper_status).color(ACCENT));
                    ui.label(egui::RichText::new("Запустите Wallpaper Engine перед подключением. При уходе из HITDROL фон замирает, а воспроизведение выгружается. После возвращения обои запускаются заново.").color(MUTED));
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Частота фона:");
                        for fps in [15,30,60] {
                            if self.button(ui,&format!("wallpaper-fps:{fps}"),&format!("{fps} FPS"),vec2(100.,46.),self.settings.wallpaper_fps == fps) { self.commands.push(Command::WallpaperFps(fps)); }
                        }
                    });
                }
                if self.settings.background_mode != BackgroundMode::Default {
                    ui.add_space(8.);
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Затемнение:");
                        for dim in [40,65,80] {
                            if self.button(ui,&format!("background-dim:{dim}"),&format!("{dim}%"),vec2(85.,46.),self.settings.background_dim == dim) { self.commands.push(Command::BackgroundDim(dim)); }
                        }
                    });
                }
                ui.add_space(26.);
                ui.label(
                    egui::RichText::new("Плавность интерфейса")
                        .size(22.)
                        .color(TEXT),
                );
                ui.label(
                    egui::RichText::new("В простое лаунчер обновляет экран только при изменениях.")
                        .color(MUTED),
                );
                ui.add_space(12.);
                ui.horizontal_wrapped(|ui| {
                    for fps in [30, 40, 60, 90, 120, 144] {
                        if self.button(
                            ui,
                            &format!("fps:{fps}"),
                            &format!("{fps}"),
                            vec2(75., 48.),
                            !self.settings.display_rate && self.settings.fps == fps,
                        ) {
                            self.commands.push(Command::Fps(fps));
                        }
                    }
                    if self.button(
                        ui,
                        "fps-display",
                        &format!("Экран · {}", self.refresh_hz),
                        vec2(140., 48.),
                        self.settings.display_rate,
                    ) {
                        self.commands.push(Command::DisplayRate);
                    }
                    if self.button(ui, "fps-custom", "Своё…", vec2(100., 48.), false) {
                        self.commands.push(Command::Keyboard(TextPurpose::Fps));
                    }
                });
                ui.add_space(8.);
                ui.label(
                    egui::RichText::new(format!(
                        "Предел: {} FPS · относится только к интерфейсу",
                        if self.settings.display_rate {
                            self.refresh_hz
                        } else {
                            self.settings.fps
                        }
                    ))
                    .size(13.)
                    .color(MUTED),
                );
                ui.add_space(26.);
                ui.label(egui::RichText::new("Экран и оформление").size(22.));
                ui.horizontal_wrapped(|ui|{
                    if self.button(ui,"autostart",if self.autostart_registered {"Убрать из автозагрузки"}else{"Добавить в автозагрузку"},vec2(285.,50.),self.autostart_registered){self.commands.push(Command::Autostart);}
                    if self.button(ui,"windows-startup","Автозагрузка Windows",vec2(250.,50.),false){self.commands.push(Command::StartupSettings);}
                });
                ui.label(egui::RichText::new("Обычный запуск после входа в Windows. Включайте или отключайте HITDROL в диспетчере задач → Автозагрузка приложений. При установке автозапуск выключен, если вы его не выбрали.").color(MUTED));
                ui.add_space(10.);
                ui.horizontal_wrapped(|ui| {
                    if self.button(
                        ui,
                        "fullscreen",
                        if self.settings.fullscreen {
                            "✓  Полный экран"
                        } else {
                            "Оконный режим"
                        },
                        vec2(210., 52.),
                        false,
                    ) {
                        self.commands.push(Command::Fullscreen);
                    }
                    if self.button(
                        ui,
                        "motion",
                        if self.settings.reduced_motion {
                            "Анимации выключены"
                        } else {
                            "Короткие анимации"
                        },
                        vec2(230., 52.),
                        false,
                    ) {
                        self.commands.push(Command::Motion);
                    }
                    if self.button(ui,"startup-animation",if self.settings.startup_animation {"Заставка при запуске: вкл."} else {"Заставка при запуске: выкл."},vec2(295.,52.),false) {
                        self.commands.push(Command::StartupAnimation);
                    }
                    if self.button(ui,"startup-sound",if self.settings.startup_sound {"Звук заставки: вкл."} else {"Звук заставки: выкл."},vec2(255.,52.),false) {
                        self.commands.push(Command::StartupSound);
                    }
                });
                ui.add_space(10.);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Масштаб:");
                    for scale in [0.8, 1.0, 1.2, 1.4] {
                        if self.button(
                            ui,
                            &format!("scale:{scale}"),
                            &format!("{}%", (scale * 100.) as u32),
                            vec2(84., 48.),
                            (self.settings.scale - scale).abs() < 0.01,
                        ) {
                            self.commands.push(Command::Scale(scale));
                        }
                    }
                });
                ui.add_space(26.);
                ui.label(egui::RichText::new("Обложки и фоны").size(22.));
                ui.add_space(10.);
                if self.button(ui,"store-art",if self.settings.store_art { "✓  Обложки магазинов" } else { "Обложки магазинов выключены" },vec2(320.,50.),self.settings.store_art) {
                    self.commands.push(Command::StoreArt);
                }
                ui.add(egui::Label::new(egui::RichText::new("Steam, Epic и GOG: автоматическая загрузка официальных изображений без API-ключа. Кэш доступен без сети.").color(MUTED)).wrap());
                if self.button(ui,"refresh-art",if self.auto_busy { "Обложки загружаются…" } else { "Обновить обложки" },vec2(255.,50.),false) { self.commands.push(Command::Scan); }
                ui.add_space(12.);
                ui.label(
                    egui::RichText::new(if self.key_present {
                        "SteamGridDB подключён. Ключ хранится в хранилище Windows."
                    } else {
                        "Необязательный ключ SteamGridDB включает онлайн-подбор изображений."
                    })
                    .color(MUTED),
                );
                ui.add_space(10.);
                ui.horizontal_wrapped(|ui| {
                    if self.button(
                        ui,
                        "api-key",
                        if self.key_present {
                            "Изменить ключ"
                        } else {
                            "Добавить API-ключ"
                        },
                        vec2(215., 50.),
                        false,
                    ) {
                        self.commands.push(Command::Keyboard(TextPurpose::Key));
                    }
                    if self.key_present {
                        if self.button(
                            ui,
                            "auto-art",
                            if self.auto_busy {
                                "Загрузка…"
                            } else {
                                "Заполнить обложки"
                            },
                            vec2(225., 50.),
                            false,
                        ) {
                            self.commands.push(Command::AutoArt);
                        }
                        if self.button(ui, "clear-key", "Удалить ключ", vec2(160., 50.), false)
                        {
                            self.commands.push(Command::ClearKey);
                        }
                    }
                });
                ui.add_space(8.);
                ui.label(
                    egui::RichText::new("Ключ: steamgriddb.com → Profile → Preferences → API")
                        .size(13.)
                        .color(MUTED),
                );
                ui.add_space(26.);
                ui.label(egui::RichText::new("О приложении").size(22.));
                ui.add(egui::Image::new((self.logo.id(),vec2(192.,192.))));
                ui.label(
                    egui::RichText::new(format!("{} · {} игр", crate::branding::TITLE, self.games.len()))
                        .color(MUTED),
                );
                ui.label(
                    egui::RichText::new("Библиотека работает локально. Автозапуск настраивается выше.")
                        .color(MUTED),
                );
                if !self.warnings.is_empty() {
                    ui.add_space(16.);
                    ui.label(
                        egui::RichText::new(format!(
                            "Не удалось прочитать некоторые записи ({})",
                            self.warnings.len()
                        ))
                        .color(ACCENT),
                    );
                    for warning in self.warnings.iter().take(10) {
                        ui.label(egui::RichText::new(warning).size(12.).color(MUTED));
                    }
                }
                ui.add_space(30.);
            });
    }
    fn edit_view(&mut self, ui: &mut egui::Ui) {
        self.back_button(ui);
        Self::heading(
            ui,
            "Изменить игру",
            "Настройки сохранятся при следующем обновлении библиотеки.",
        );
        let Some(e) = &self.edit else {
            return;
        };
        let fields = [
            ("Название", e.name.clone(), TextPurpose::Name),
            ("Путь или ссылка", e.path.clone(), TextPurpose::Path),
            ("Аргументы", e.args.clone(), TextPurpose::Args),
            ("Рабочая папка", e.cwd.clone(), TextPurpose::Cwd),
        ];
        let shell = e.kind == LaunchKind::ShellLink;
        egui::ScrollArea::vertical().id_salt("edit").show(ui,|ui|{for (i,(title,value,purpose)) in fields.into_iter().enumerate(){ui.label(egui::RichText::new(title).color(MUTED));let label=if value.is_empty(){"Нажмите, чтобы указать".into()}else{ellipsize(&value,90)};if self.button(ui,&format!("edit-field:{i}"),&label,vec2(ui.available_width().min(900.),50.),false){self.commands.push(Command::Keyboard(purpose));}ui.add_space(8.);}if shell{ui.label(egui::RichText::new("Ярлык .lnk запускается с его собственными параметрами. Для изменения аргументов укажите путь к .exe.").size(13.).color(MUTED));}ui.add_space(18.);if self.button(ui,"save-edit","Сохранить",vec2(210.,54.),true){self.commands.push(Command::SaveEdit);}});
    }
    fn keyboard_view(&mut self, ui: &mut egui::Ui) {
        let Some(mut k) = self.keyboard.take() else {
            return;
        };
        ui.spacing_mut().item_spacing = vec2(6., 6.);
        ui.spacing_mut().interact_size = vec2(36., 36.);
        ui.spacing_mut().button_padding = vec2(6., 4.);
        ui.horizontal(|ui| {
            if self.button(ui, "back", "‹  Назад", vec2(112., 44.), false) {
                self.commands.push(Command::Back);
            }
            ui.label(egui::RichText::new(&k.title).size(26.));
        });
        let password = matches!(k.purpose, TextPurpose::Key | TextPurpose::Account(_));
        ui.add_sized(
            vec2(ui.available_width(), 44.),
            egui::TextEdit::singleline(&mut k.text)
                .password(password)
                .font(egui::TextStyle::Heading)
                .margin(vec2(14., 6.))
                .hint_text("Введите текст…"),
        );
        ui.add_space(6.);
        let rows = if k.russian {
            vec![
                "1234567890-=",
                "йцукенгшщзхъ",
                "фывапролджэ",
                "ячсмитьбюё",
                "/\\:.;,_@\"'!?",
            ]
        } else {
            vec![
                "1234567890-=",
                "qwertyuiop[]",
                "asdfghjkl",
                "zxcvbnm",
                "/\\:.;,_@\"'!?",
            ]
        };
        let key_h = ((ui.available_height() - 55.) / 5. - 6.).clamp(32., 54.);
        let key_w = ((ui.available_width() - 11. * 6.) / 12.).min(78.);
        for (row, letters) in rows.iter().enumerate() {
            ui.horizontal(|ui| {
                for (col, c) in letters.chars().enumerate() {
                    let text = if k.shift {
                        c.to_uppercase().to_string()
                    } else {
                        c.to_string()
                    };
                    if self.button(
                        ui,
                        &format!("key:{row}:{col}"),
                        &text,
                        vec2(key_w, key_h),
                        false,
                    ) {
                        k.text.push_str(&text);
                    }
                }
            });
        }
        ui.add_space(5.);
        ui.horizontal_wrapped(|ui| {
            if self.button(
                ui,
                "keyboard-lang",
                if k.russian { "RU → EN" } else { "EN → RU" },
                vec2(100., 44.),
                false,
            ) {
                k.russian = !k.russian;
            }
            if self.button(ui, "keyboard-shift", "aA", vec2(60., 44.), k.shift) {
                k.shift = !k.shift;
            }
            let account = matches!(k.purpose, TextPurpose::Account(_));
            if self.button(
                ui,
                "keyboard-space",
                if account {
                    "Вставить"
                } else {
                    "Пробел"
                },
                vec2(150., 44.),
                false,
            ) {
                if account {
                    self.paste_requested = true;
                } else {
                    k.text.push(' ');
                }
            }
            if self.button(ui, "keyboard-delete", "Стереть", vec2(90., 44.), false) {
                k.text.pop();
            }
            if self.button(ui, "keyboard-clear", "Очистить", vec2(100., 44.), false) {
                k.text.clear();
            }
            if self.button(ui, "keyboard-apply", "Готово", vec2(130., 44.), true) {
                self.commands.push(Command::ApplyText);
            }
        });
        self.keyboard = Some(k);
    }
    fn art_view(&mut self, ui: &mut egui::Ui) {
        self.back_button(ui);
        let Some(a) = &self.art else {
            return;
        };
        let hero = a.hero;
        Self::heading(
            ui,
            if hero {
                "Выберите фон"
            } else {
                "Выберите обложку"
            },
            "SteamGridDB · выбранное изображение сохранится на устройстве",
        );
        enum Content {
            Busy,
            Games(Vec<ArtGame>),
            Images(Vec<(ArtImage, Option<PathBuf>)>),
        }
        let content = match &a.panel {
            ArtPanel::Searching | ArtPanel::Loading => Content::Busy,
            ArtPanel::Games(g) => Content::Games(g.clone()),
            ArtPanel::Images(i) => Content::Images(i.clone()),
        };
        match content {
            Content::Busy => {
                ui.label("Загрузка…");
            }
            Content::Games(games) => {
                if games.is_empty() {
                    ui.label(
                        "Совпадений нет. Измените название игры или выберите изображение из файла.",
                    );
                }
                egui::ScrollArea::vertical()
                    .id_salt("art-games")
                    .show(ui, |ui| {
                        for game in games {
                            if self.button(
                                ui,
                                &format!("art-game:{}", game.id),
                                &game.name,
                                vec2(ui.available_width(), 56.),
                                false,
                            ) {
                                self.commands.push(Command::ArtGame(game.id));
                            }
                        }
                    });
            }
            Content::Images(images) => {
                if images.is_empty() {
                    ui.label("Изображения не найдены. Можно выбрать файл с устройства.");
                }
                egui::ScrollArea::vertical()
                    .id_salt("art-images")
                    .show(ui, |ui| {
                        let columns = if hero { 2 } else { 4 };
                        let w =
                            (ui.available_width() - 16. * (columns - 1) as f32) / columns as f32;
                        for row in images.chunks(columns) {
                            ui.horizontal(|ui| {
                                for (image, path) in row {
                                    let (rect, response) = ui.allocate_exact_size(
                                        vec2(w, if hero { w * 0.55 } else { w * 1.4 }),
                                        Sense::CLICK,
                                    );
                                    let id = format!("art-image:{}", image.id);
                                    response.surrender_focus();
                                    let focused = self.nav.focused.as_deref() == Some(&id);
                                    self.register(&id, rect);
                                    if let Some((texture, size)) = self.textures.get(path.as_ref())
                                    {
                                        ui.painter().image(
                                            texture,
                                            rect,
                                            cover_uv(size, rect.size()),
                                            Color32::WHITE,
                                        );
                                    } else {
                                        ui.painter().rect_filled(rect, 10, PANEL);
                                        ui.painter().text(
                                            rect.center(),
                                            Align2::CENTER_CENTER,
                                            "Обложка",
                                            FontId::proportional(18.),
                                            MUTED,
                                        );
                                    }
                                    if focused {
                                        ui.painter().rect_stroke(
                                            rect,
                                            10,
                                            Stroke::new(2_f32, ACCENT),
                                            egui::StrokeKind::Inside,
                                        );
                                        if self.nav.reveal {
                                            ui.scroll_to_rect(rect, Some(egui::Align::Center));
                                        }
                                    }
                                    if response.clicked() || focused && self.accept {
                                        self.accept = false;
                                        self.commands.push(Command::ArtImage(image.url.clone()));
                                    }
                                }
                            });
                        }
                    });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_opens_in_english_and_name_editor_keeps_russian() {
        let (_d, _ctx, mut app) = setup(1);
        app.open_keyboard(TextPurpose::Search);
        assert!(!app.keyboard.as_ref().unwrap().russian);
        app.back();
        app.open_keyboard(TextPurpose::Name);
        assert!(app.keyboard.as_ref().unwrap().russian);
        app.back();
        app.open_keyboard(TextPurpose::Search);
        assert!(!app.keyboard.as_ref().unwrap().russian);
    }
    #[test]
    fn launch_and_install_requests_never_fabricate_recent_history() {
        let (_d, ctx, mut app) = setup(1);
        app.games[0].history_verified = false;
        app.games[0].last_played = 0;
        let id = app.games[0].id.clone();
        app.tx.send(Event::Launch(id.clone(), Ok(()))).unwrap();
        app.tx.send(Event::InstallSent(Ok(()))).unwrap();
        app.process_events();
        assert!(recent_games(&app.games, true).is_empty());
        app.gamepad_suspended = true;
        app.tx
            .send(Event::InstallSent(Err("Клиент отсутствует".into())))
            .unwrap();
        app.process_events();
        assert!(!app.gamepad_suspended);
        app.page = Page::Home;
        draw(&mut app, &ctx, vec2(1280., 720.));
        assert!(!app.nav.items.iter().any(|n| n.id.starts_with("game:")));
        let at = timestamp() - 10;
        app.tx.send(Event::Played(id, at)).unwrap();
        app.process_events();
        assert_eq!(recent_games(&app.games, false)[0].last_played, at);
    }
    #[test]
    fn entire_library_filter_and_account_keyboard_fit_handheld() {
        let (_d, ctx, mut app) = setup(2);
        app.games[1].installed = false;
        app.games[1].owned = true;
        assert_eq!(app.visible_games().len(), 1);
        app.settings.show_uninstalled = true;
        assert_eq!(app.visible_games().len(), 2);
        app.games[1].owned = false;
        assert_eq!(app.visible_games().len(), 1);
        app.open_keyboard(TextPurpose::Account(Source::Epic));
        draw(&mut app, &ctx, vec2(760., 540.));
        for id in ["keyboard-space", "keyboard-apply"] {
            let target = app.nav.items.iter().find(|n| n.id == id).unwrap();
            assert!(target.rect.bottom() <= 500.);
        }
        app.paste_text("test-code-1234".into());
        assert_eq!(app.keyboard.as_ref().unwrap().text, "test-code-1234");
        app.back();
        assert!(app.keyboard.is_none());
    }
    fn setup(count: usize) -> (tempfile::TempDir, egui::Context, UiApp) {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let db = Database::open(temp.path()).unwrap();
        let app = UiApp::new(db, ctx.clone(), count).unwrap();
        (temp, ctx, app)
    }
    fn draw(app: &mut UiApp, ctx: &egui::Context, size: Vec2) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            focused: true,
            ..Default::default()
        };
        let _ = ctx.run(input, |c| app.frame(c));
    }
    #[test]
    fn return_from_game_restores_focus() {
        let (_d, ctx, mut app) = setup(12);
        app.method = InputMethod::Gamepad;
        app.change_page(Page::Library);
        draw(&mut app, &ctx, vec2(1280., 800.));
        let id = app.games[3].id.clone();
        let focus = format!("game:{id}");
        app.nav.focused = Some(focus.clone());
        app.action(Action::Accept);
        draw(&mut app, &ctx, vec2(1280., 800.));
        assert_eq!(app.page, Page::Detail);
        draw(&mut app, &ctx, vec2(1280., 800.));
        app.action(Action::Back);
        draw(&mut app, &ctx, vec2(1280., 800.));
        assert_eq!(app.page, Page::Library);
        assert_eq!(app.nav.focused.as_deref(), Some(focus.as_str()));
    }
    #[test]
    fn automatic_scans_keep_notices_quiet_but_manual_refresh_reports_completion() {
        let (_d, _ctx, mut app) = setup(3);
        app.notify("Изменения сохранены");
        let previous = app.notice.clone();
        app.tx.send(Event::Scan(Scan::default(), false)).unwrap();
        app.process_events();
        assert_eq!(app.notice, previous);
        app.tx.send(Event::Scan(Scan::default(), true)).unwrap();
        app.process_events();
        assert!(
            app.notice
                .as_ref()
                .unwrap()
                .0
                .starts_with("Сканирование завершено")
        );
        let manual_notice = app.notice.clone();
        app.tx.send(Event::Scan(Scan::default(), false)).unwrap();
        app.process_events();
        assert_eq!(app.notice, manual_notice);
    }
    #[test]
    fn running_game_has_a_reachable_close_button_until_process_exit() {
        for size in [
            vec2(1280., 720.),
            vec2(1280., 800.),
            vec2(1920., 1080.),
            vec2(1920., 1200.),
        ] {
            let (_d, ctx, mut app) = setup(3);
            let id = app.games[0].id.clone();
            app.commands.push(Command::Open(id.clone()));
            app.process_commands();
            draw(&mut app, &ctx, size);
            let original = app.nav.items.iter().find(|n| n.id == "play").unwrap().rect;
            app.change_page(Page::Home);
            app.tx
                .send(Event::Running(vec![crate::activity::RunningGame {
                    id: id.clone(),
                    processes: vec![],
                }]))
                .unwrap();
            draw(&mut app, &ctx, size);
            assert!(
                app.nav
                    .items
                    .iter()
                    .any(|n| n.id == format!("running:{id}"))
            );
            app.commands.push(Command::Open(id));
            app.process_commands();
            draw(&mut app, &ctx, size);
            let close = app.nav.items.iter().find(|n| n.id == "play").unwrap();
            assert_eq!(close.rect, original);
            assert_eq!(app.nav.items.iter().filter(|n| n.id == "play").count(), 1);
            assert!(!app.nav.items.iter().any(|n| n.id == "close-game"));
            assert!(close.rect.top() >= 0. && close.rect.bottom() <= size.y - 48.);
            app.nav.focused = Some("play".into());
            app.gamepad_suspended = false;
            app.method = InputMethod::Gamepad;
            app.action(Action::Accept);
            draw(&mut app, &ctx, size);
            assert!(
                app.notice
                    .as_ref()
                    .is_some_and(|(text, _)| text.contains("не найдено"))
                    || !app.closing.is_empty()
            );
            app.tx.send(Event::Running(vec![])).unwrap();
            draw(&mut app, &ctx, size);
            assert_eq!(
                app.nav.items.iter().find(|n| n.id == "play").unwrap().rect,
                original
            );
            assert!(!app.nav.items.iter().any(|n| n.id == "close-game"));
        }
    }
    #[test]
    fn clicking_a_tab_does_not_block_keyboard_navigation() {
        let (_d, ctx, mut app) = setup(12);
        let size = vec2(1280., 720.);
        draw(&mut app, &ctx, size);
        let pos = app
            .nav
            .items
            .iter()
            .find(|i| i.id == "tab:Library")
            .unwrap()
            .rect
            .center();
        for pressed in [true, false] {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                focused: true,
                events: vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            };
            let _ = ctx.run(input, |c| app.frame(c));
        }
        draw(&mut app, &ctx, size);
        assert_eq!(app.page, Page::Library);
        assert!(
            !ctx.wants_keyboard_input(),
            "A button must not consume arrow navigation"
        );
        let before = app.nav.focused.clone();
        app.action(Action::Down);
        assert_ne!(before, app.nav.focused);
    }
    #[test]
    fn no_gamepad_actions_while_unfocused() {
        let (_d, ctx, mut app) = setup(12);
        draw(&mut app, &ctx, vec2(1280., 800.));
        let before = app.nav.focused.clone();
        app.focused = false;
        app.action(Action::Down);
        app.action(Action::Accept);
        draw(&mut app, &ctx, vec2(1280., 800.));
        assert_eq!(app.page, Page::Home);
        assert_eq!(app.nav.focused, before);
    }
    #[test]
    fn onscreen_keyboard_fits_small_logical_viewport() {
        let (_d, ctx, mut app) = setup(1);
        app.open_keyboard(TextPurpose::Search);
        for size in [vec2(1280., 720.), vec2(960., 600.), vec2(760., 540.)] {
            draw(&mut app, &ctx, size);
            let done = app
                .nav
                .items
                .iter()
                .find(|i| i.id == "keyboard-apply")
                .unwrap();
            assert!(
                done.rect.bottom() <= size.y - 40.,
                "Done outside viewport {size:?}: {:?}",
                done.rect
            );
            assert!(done.rect.right() <= size.x);
        }
    }
    #[test]
    fn keyboard_accept_edits_search_and_back_cancels() {
        let (_d, ctx, mut app) = setup(12);
        app.method = InputMethod::Gamepad;
        app.open_keyboard(TextPurpose::Search);
        draw(&mut app, &ctx, vec2(1280., 800.));
        app.nav.focused = Some("key:1:0".into());
        app.action(Action::Accept);
        draw(&mut app, &ctx, vec2(1280., 800.));
        assert_eq!(app.keyboard.as_ref().unwrap().text, "q");
        app.nav.focused = Some("keyboard-apply".into());
        app.action(Action::Accept);
        draw(&mut app, &ctx, vec2(1280., 800.));
        assert_eq!(app.search, "q");
        assert!(app.keyboard.is_none());
        app.open_keyboard(TextPurpose::Search);
        app.keyboard.as_mut().unwrap().text = "discard".into();
        app.back();
        assert_eq!(app.search, "q");
    }
    #[test]
    fn thousand_games_keep_offscreen_navigation() {
        let (_d, ctx, mut app) = setup(1000);
        app.change_page(Page::Library);
        draw(&mut app, &ctx, vec2(1280., 720.));
        assert_eq!(
            app.nav
                .items
                .iter()
                .filter(|i| i.id.starts_with("game:"))
                .count(),
            1000
        );
        let last = app.games.last().unwrap().id.clone();
        app.nav.focused = Some(format!("game:{last}"));
        app.nav.reveal = true;
        draw(&mut app, &ctx, vec2(1280., 720.));
        assert_eq!(app.nav.focused, Some(format!("game:{last}")));
    }
    #[test]
    fn no_matches_and_empty_library_are_navigable() {
        let (_d, ctx, mut app) = setup(12);
        app.change_page(Page::Library);
        app.search = "this-will-not-match".into();
        draw(&mut app, &ctx, vec2(1280., 720.));
        assert!(app.nav.items.iter().any(|i| i.id == "clear-search"));
        assert!(!app.nav.items.iter().any(|i| i.id.starts_with("game:")));
        app.games.clear();
        app.change_page(Page::Home);
        draw(&mut app, &ctx, vec2(1280., 720.));
        assert!(app.nav.items.iter().any(|i| i.id == "empty-add"));
    }
    #[test]
    fn file_picker_only_opens_from_other() {
        let (_d, ctx, mut app) = setup(1);
        app.page = Page::Add;
        app.method = InputMethod::Gamepad;
        draw(&mut app, &ctx, vec2(1280., 720.));
        assert!(app.browser.is_none());
        app.nav.focused = Some("other".into());
        app.action(Action::Accept);
        draw(&mut app, &ctx, vec2(1280., 720.));
        assert!(app.browser.is_some());
    }
    #[test]
    fn stale_art_search_does_not_replace_new_dialog() {
        let (_d, _ctx, mut app) = setup(1);
        app.art = Some(ArtDialog {
            token: 2,
            game: "new".into(),
            hero: false,
            panel: ArtPanel::Searching,
        });
        app.tx.send(Event::ArtGames(1, Ok(vec![]))).unwrap();
        app.process_events();
        assert!(matches!(app.art.unwrap().panel, ArtPanel::Searching));
    }
    #[test]
    fn wallpaper_selection_persists_and_cancel_ignores_late_import() {
        let (_d, _ctx, mut app) = setup(1);
        app.wallpaper_epoch = 5;
        app.tx
            .send(Event::WallpaperImported(
                5,
                Ok(PathBuf::from("D:/кэш/обои.png")),
            ))
            .unwrap();
        app.process_events();
        let saved = app.db.settings().unwrap();
        assert_eq!(saved.background_mode, BackgroundMode::Image);
        assert_eq!(
            saved.background_image,
            Some(PathBuf::from("D:/кэш/обои.png"))
        );
        app.commands.push(Command::Browse(Purpose::Wallpaper));
        app.process_commands();
        app.wallpaper_epoch = 6;
        app.back();
        app.tx
            .send(Event::WallpaperImported(
                6,
                Ok(PathBuf::from("D:/late.png")),
            ))
            .unwrap();
        app.process_events();
        assert_eq!(app.settings.background_image, saved.background_image);
    }
    #[test]
    fn wallpaper_picker_copies_real_image_and_survives_restart() {
        let (_d, _ctx, mut app) = setup(1);
        let inputs = tempfile::tempdir().unwrap();
        let source = inputs.path().join("Мои обои с пробелами.png");
        image::RgbaImage::from_pixel(24, 12, image::Rgba([40, 150, 240, 255]))
            .save(&source)
            .unwrap();
        app.commands.push(Command::Browse(Purpose::Wallpaper));
        app.process_commands();
        app.commands.push(Command::Pick(source.clone()));
        app.process_commands();
        let start = Instant::now();
        while app.settings.background_image.is_none() && start.elapsed() < Duration::from_secs(3) {
            app.process_events();
            std::thread::sleep(Duration::from_millis(5));
        }
        let cached = app
            .settings
            .background_image
            .clone()
            .expect("wallpaper imported");
        assert!(cached.starts_with(app.db.root.join("wallpapers")));
        assert!(cached.is_file());
        assert!(app.browser.is_none());
        drop(inputs);
        assert!(!source.exists());
        let restored = Database::open(&app.db.root).unwrap().settings().unwrap();
        assert_eq!(restored.background_mode, BackgroundMode::Image);
        assert_eq!(restored.background_image, Some(cached.clone()));
        assert_eq!(image::open(cached).unwrap().width(), 24);
    }
    #[test]
    fn wallpaper_controls_are_controller_targets_at_handheld_size() {
        let (_d, ctx, mut app) = setup(1);
        app.page = Page::Settings;
        app.settings.background_mode = BackgroundMode::WallpaperEngine;
        draw(&mut app, &ctx, vec2(760., 540.));
        for id in [
            "wallpaper-project",
            "wallpaper-retry",
            "wallpaper-exe",
            "wallpaper-fps:30",
            "background-dim:65",
        ] {
            assert!(app.nav.items.iter().any(|i| i.id == id), "{id}");
        }
    }
    #[test]
    fn artwork_refresh_during_download_is_queued() {
        let (_d, _ctx, mut app) = setup(1);
        app.demo = false;
        app.auto_busy = true;
        app.start_auto_art();
        assert!(app.art_pending);
        app.demo = true;
        app.tx.send(Event::AutoDone(0, None)).unwrap();
        app.process_events();
        assert!(!app.auto_busy);
        assert!(!app.art_pending);
    }
    #[test]
    fn official_art_replaces_broken_base_path_and_preserves_user_override() {
        let (_d, _ctx, mut app) = setup(1);
        let id = app.games[0].id.clone();
        app.games[0].cover = Some("missing-old-store-cover.png".into());
        app.games[0].user_cover = Some("my-custom-cover.png".into());
        app.tx
            .send(Event::AutoArt(
                id,
                Some("repaired-store-cover.png".into()),
                None,
            ))
            .unwrap();
        app.process_events();
        assert_eq!(
            app.games[0].cover,
            Some(PathBuf::from("repaired-store-cover.png"))
        );
        assert_eq!(
            app.games[0].user_cover,
            Some(PathBuf::from("my-custom-cover.png"))
        );
    }
}
