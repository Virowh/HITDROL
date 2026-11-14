use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Source {
    #[default]
    Manual,
    Steam,
    Epic,
    Gog,
}
impl Source {
    pub const ALL: [Self; 4] = [Self::Manual, Self::Steam, Self::Epic, Self::Gog];
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Мои игры",
            Self::Steam => "Steam",
            Self::Epic => "Epic Games",
            Self::Gog => "GOG",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LaunchKind {
    Executable,
    Uri,
    ShellLink,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchTarget {
    pub kind: LaunchKind,
    pub path: String,
    pub args: String,
    pub cwd: String,
    #[serde(default)]
    pub resolved_key: Option<String>,
}
impl LaunchTarget {
    pub fn exe(path: impl Into<String>, args: impl Into<String>, cwd: impl Into<String>) -> Self {
        Self {
            kind: LaunchKind::Executable,
            path: path.into(),
            args: args.into(),
            cwd: cwd.into(),
            resolved_key: None,
        }
    }
    pub fn uri(uri: impl Into<String>) -> Self {
        Self {
            kind: LaunchKind::Uri,
            path: uri.into(),
            args: String::new(),
            cwd: String::new(),
            resolved_key: None,
        }
    }
    pub fn key(&self) -> String {
        if let Some(k) = &self.resolved_key {
            return k.clone();
        }
        let path = self.path.trim().replace('/', "\\").to_lowercase();
        let uri = self.path.trim().to_lowercase();
        if let Some(id) = uri
            .strip_prefix("steam://rungameid/")
            .or_else(|| uri.strip_prefix("steam://run/"))
        {
            // Only normalize a bare app launch; launch parameters identify a different action.
            let bare = id.trim_end_matches('/');
            if !bare.is_empty() && bare.chars().all(|c| c.is_ascii_digit()) {
                return format!("steam:{bare}");
            }
        }
        format!(
            "{:?}|{}|{}|{}",
            self.kind,
            if self.kind == LaunchKind::Uri {
                self.path.trim().to_owned()
            } else {
                path
            },
            self.args,
            self.cwd.replace('/', "\\").to_lowercase()
        )
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.path.trim().is_empty(), "Не указан путь запуска");
        anyhow::ensure!(
            !self.path.contains('\0') && !self.args.contains('\0') && !self.cwd.contains('\0'),
            "Недопустимый символ в параметрах запуска"
        );
        if self.kind == LaunchKind::Uri {
            let lower = self.path.to_lowercase();
            anyhow::ensure!(
                ["steam://", "com.epicgames.launcher://", "goggalaxy://"]
                    .iter()
                    .any(|s| lower.starts_with(s)),
                "Поддерживаются игровые ссылки Steam, Epic и GOG"
            );
        } else {
            anyhow::ensure!(
                std::path::Path::new(&self.path).is_file(),
                "Файл не найден. Проверьте путь или подключите диск"
            );
            if self.kind == LaunchKind::Executable && !self.cwd.is_empty() {
                anyhow::ensure!(
                    std::path::Path::new(&self.cwd).is_dir(),
                    "Рабочая папка недоступна"
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub source: Source,
    pub source_id: String,
    pub title: String,
    pub launch: LaunchTarget,
    pub install_dir: Option<PathBuf>,
    pub cover: Option<PathBuf>,
    pub hero: Option<PathBuf>,
    pub icon: Option<PathBuf>,
    pub favorite: bool,
    pub last_played: u64,
    #[serde(default)]
    pub history_verified: bool,
    #[serde(default = "default_installed")]
    pub installed: bool,
    #[serde(default)]
    pub owned: bool,
    #[serde(default)]
    pub install_target: Option<LaunchTarget>,
    #[serde(default)]
    pub process_path: Option<PathBuf>,
    pub available: bool,
    pub user_title: Option<String>,
    pub user_launch: Option<LaunchTarget>,
    pub user_cover: Option<PathBuf>,
    pub user_hero: Option<PathBuf>,
    #[serde(default)]
    pub store_cover_url: Option<String>,
    #[serde(default)]
    pub store_hero_url: Option<String>,
}
impl Game {
    pub fn new(source: Source, source_id: String, title: String, launch: LaunchTarget) -> Self {
        Self {
            id: format!("{:?}:{}", source, source_id),
            source,
            source_id,
            title,
            launch,
            install_dir: None,
            cover: None,
            hero: None,
            icon: None,
            favorite: false,
            last_played: 0,
            history_verified: false,
            installed: true,
            owned: false,
            install_target: None,
            process_path: None,
            available: true,
            user_title: None,
            user_launch: None,
            user_cover: None,
            user_hero: None,
            store_cover_url: None,
            store_hero_url: None,
        }
    }
    pub fn name(&self) -> &str {
        self.user_title.as_deref().unwrap_or(&self.title)
    }
    pub fn target(&self) -> &LaunchTarget {
        self.user_launch.as_ref().unwrap_or(&self.launch)
    }
    pub fn record_played(&mut self, at: u64) {
        if valid_played(at) {
            self.last_played = if self.history_verified && valid_played(self.last_played) {
                self.last_played.max(at)
            } else {
                at
            };
            self.history_verified = true;
        }
    }
    pub fn cover_path(&self) -> Option<&PathBuf> {
        [
            self.user_cover.as_ref(),
            self.cover.as_ref(),
            self.icon.as_ref(),
        ]
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
    }
    pub fn hero_path(&self) -> Option<&PathBuf> {
        [
            self.user_hero.as_ref(),
            self.hero.as_ref(),
            self.user_cover.as_ref(),
            self.cover.as_ref(),
        ]
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundMode {
    #[default]
    Default,
    Image,
    WallpaperEngine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub fps: u32,
    pub display_rate: bool,
    pub fullscreen: bool,
    pub scale: f32,
    pub reduced_motion: bool,
    pub startup_animation: bool,
    pub startup_sound: bool,
    pub background_mode: BackgroundMode,
    pub background_image: Option<PathBuf>,
    pub wallpaper_engine: Option<PathBuf>,
    pub wallpaper_project: Option<PathBuf>,
    pub wallpaper_fps: u32,
    pub background_dim: u8,
    pub store_art: bool,
    pub show_uninstalled: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            fps: 60,
            display_rate: false,
            fullscreen: true,
            scale: 1.0,
            reduced_motion: false,
            startup_animation: true,
            startup_sound: true,
            background_mode: BackgroundMode::Default,
            background_image: None,
            wallpaper_engine: None,
            wallpaper_project: None,
            wallpaper_fps: 30,
            background_dim: 65,
            store_art: true,
            show_uninstalled: false,
        }
    }
}
impl Settings {
    pub fn sanitize(&mut self) {
        self.fps = self.fps.clamp(30, 240);
        if !self.scale.is_finite() {
            self.scale = 1.0;
        }
        self.scale = self.scale.clamp(0.8, 1.4);
        self.wallpaper_fps = self.wallpaper_fps.clamp(15, 60);
        self.background_dim = self.background_dim.clamp(30, 90);
    }
}
fn default_installed() -> bool {
    true
}
pub fn valid_played(at: u64) -> bool {
    (946684800..=timestamp().saturating_add(300)).contains(&at)
}
pub fn recent_games(games: &[Game], include_uninstalled: bool) -> Vec<Game> {
    let mut games: Vec<_> = games
        .iter()
        .filter(|g| {
            g.history_verified
                && valid_played(g.last_played)
                && (include_uninstalled || g.installed)
                && (g.installed || g.owned || g.source == Source::Manual)
        })
        .cloned()
        .collect();
    games.sort_by(|a, b| {
        b.last_played
            .cmp(&a.last_played)
            .then_with(|| a.id.cmp(&b.id))
    });
    games.truncate(18);
    games
}
pub fn played_label(at: u64) -> String {
    let seconds = timestamp().saturating_sub(at);
    if seconds < 60 {
        "только что".into()
    } else if seconds < 3600 {
        format!("{} мин. назад", seconds / 60)
    } else if seconds < 86400 {
        format!("{} ч. назад", seconds / 3600)
    } else {
        format!("{} дн. назад", seconds / 86400)
    }
}
pub fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn stable_hash(s: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(s.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recents_require_confirmed_history_and_descending_real_time() {
        let mut a = Game::new(
            Source::Steam,
            "1".into(),
            "A".into(),
            LaunchTarget::uri("steam://run/1"),
        );
        a.last_played = timestamp();
        assert!(recent_games(&[a.clone()], true).is_empty());
        let mut b = a.clone();
        b.id = "b".into();
        b.record_played(timestamp() - 10);
        a.record_played(timestamp() - 60);
        let games = recent_games(&[a.clone(), b.clone()], true);
        assert_eq!(games[0].id, "b");
        assert_eq!(games[1].id, a.id);
        b.installed = false;
        b.owned = true;
        assert_eq!(recent_games(&[a.clone(), b.clone()], false).len(), 1);
        b.owned = false;
        assert_eq!(
            recent_games(&[a, b], true).len(),
            1,
            "disconnected account entries are hidden"
        );
    }
    #[test]
    fn steam_shortcut_identity() {
        assert_eq!(
            LaunchTarget::uri("steam://rungameid/123").key(),
            LaunchTarget::uri("steam://run/123/").key()
        );
    }
    #[test]
    fn arguments_are_case_sensitive() {
        assert_ne!(
            LaunchTarget::exe("C:/GAME.exe", "--profile A", "").key(),
            LaunchTarget::exe("c:/game.exe", "--profile a", "").key()
        );
    }
    #[test]
    fn reject_non_game_protocol() {
        assert!(
            LaunchTarget::uri("powershell://something")
                .validate()
                .is_err()
        );
    }
}
