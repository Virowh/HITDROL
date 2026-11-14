use crate::{model::*, platform};
use anyhow::{Context, Result};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use winreg::{RegKey, enums::*};

#[derive(Default)]
pub struct Scan {
    pub games: Vec<Game>,
    pub warnings: Vec<String>,
    pub owned_sources: Vec<Source>,
}
pub trait GameSource {
    fn scan(&self) -> Scan;
}
pub struct SteamSource {
    pub root: PathBuf,
}
pub struct EpicSource {
    pub manifests: PathBuf,
}
pub struct GogSource;

pub fn read_text(path: &Path) -> Result<String> {
    anyhow::ensure!(
        std::fs::metadata(path)?.len() <= 16 * 1024 * 1024,
        "Файл слишком большой"
    );
    let bytes = std::fs::read(path)?;
    if bytes.starts_with(&[0xff, 0xfe]) {
        Ok(String::from_utf16_lossy(
            &bytes[2..]
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        ))
    } else {
        Ok(String::from_utf8(bytes)?
            .trim_start_matches('\u{feff}')
            .to_string())
    }
}
#[derive(Debug, Clone)]
pub enum Vdf {
    Text(String),
    Object(BTreeMap<String, Vdf>),
}
impl Vdf {
    pub(crate) fn get(&self, key: &str) -> Option<&Vdf> {
        if let Self::Object(v) = self {
            v.get(key).or_else(|| {
                v.iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(key))
                    .map(|(_, v)| v)
            })
        } else {
            None
        }
    }
    pub(crate) fn text(&self) -> Option<&str> {
        if let Self::Text(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub(crate) fn str(&self, key: &str) -> Option<&str> {
        self.get(key)?.text()
    }
}
pub fn parse_vdf(input: &str) -> Result<Vdf> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '{' | '}' => tokens.push(c.to_string()),
            '"' => {
                let mut s = String::new();
                let mut closed = false;
                while let Some(c) = chars.next() {
                    if c == '"' {
                        closed = true;
                        break;
                    }
                    if c == '\\' {
                        match chars.peek() {
                            Some('"' | '\\') => s.push(chars.next().unwrap()),
                            _ => s.push(c),
                        }
                    } else {
                        s.push(c)
                    }
                }
                anyhow::ensure!(closed, "Незакрытая строка VDF");
                tokens.push(s);
            }
            _ => {
                let mut s = c.to_string();
                while chars
                    .peek()
                    .is_some_and(|c| !c.is_whitespace() && *c != '{' && *c != '}')
                {
                    s.push(chars.next().unwrap());
                }
                tokens.push(s);
            }
        }
    }
    fn object(tokens: &[String], i: &mut usize, nested: bool, depth: usize) -> Result<Vdf> {
        anyhow::ensure!(depth < 32, "Слишком глубокая вложенность VDF");
        let mut out = BTreeMap::new();
        while *i < tokens.len() {
            let key = tokens[*i].clone();
            *i += 1;
            if key == "}" {
                anyhow::ensure!(nested, "Лишняя скобка VDF");
                return Ok(Vdf::Object(out));
            }
            anyhow::ensure!(key != "{", "Отсутствует ключ VDF");
            let value = tokens.get(*i).context("Отсутствует значение VDF")?;
            *i += 1;
            let v = if value == "{" {
                object(tokens, i, true, depth + 1)?
            } else {
                anyhow::ensure!(value != "}", "Отсутствует значение VDF");
                Vdf::Text(value.clone())
            };
            out.insert(key, v);
        }
        anyhow::ensure!(!nested, "Незакрытый объект VDF");
        Ok(Vdf::Object(out))
    }
    object(&tokens, &mut 0, false, 0)
}

pub fn steam_from_manifest(text: &str, steamapps: &Path) -> Result<Game> {
    let data = parse_vdf(text)?;
    let state = data.get("AppState").context("Нет AppState")?;
    let id = state.str("appid").context("Нет appid")?;
    anyhow::ensure!(
        id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty(),
        "Некорректный appid"
    );
    let name = state.str("name").context("Нет названия")?;
    let dir = state.str("installdir").context("Нет каталога")?;
    let mut game = Game::new(
        Source::Steam,
        id.into(),
        name.into(),
        LaunchTarget::uri(format!("steam://run/{id}")),
    );
    game.install_dir = Some(steamapps.join("common").join(dir));
    game.installed = state
        .str("StateFlags")
        .and_then(|s| s.parse::<u32>().ok())
        .is_none_or(|flags| flags & 4 != 0);
    game.available = game.installed && game.install_dir.as_ref().is_some_and(|p| p.is_dir());
    game.install_target = Some(LaunchTarget::uri(format!("steam://install/{id}")));
    if let Some(at) = state.str("LastPlayed").and_then(|s| s.parse().ok()) {
        game.record_played(at);
    }
    Ok(game)
}
fn files(path: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(|r| r.ok().map(|e| e.path()))
        .collect()
}
pub(crate) fn local_steam_art(root: &Path, id: &str, kind: &str) -> Option<PathBuf> {
    let cache = root.join("appcache/librarycache");
    let mut candidates = files(&cache.join(id));
    let dirs: Vec<_> = candidates.iter().filter(|p| p.is_dir()).cloned().collect();
    for d in dirs {
        candidates.extend(files(&d));
    }
    candidates.sort();
    let kinds: &[&str] = match kind {
        "library_600x900" => &[
            "library_600x900",
            "library_capsule",
            "library_header",
            "header",
        ],
        "library_hero" => &["library_hero", "library_header", "header"],
        _ => &[kind],
    };
    for kind in kinds {
        for ext in ["jpg", "png", "webp", "jpeg"] {
            for p in [
                cache.join(format!("{id}_{kind}.{ext}")),
                cache.join(id).join(format!("{kind}.{ext}")),
            ] {
                if p.is_file() {
                    return Some(p);
                }
            }
        }
        if let Some(p) = candidates.iter().find(|p| {
            p.is_file()
                && p.file_stem()
                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(kind))
        }) {
            return Some(p.clone());
        }
    }
    None
}
impl GameSource for SteamSource {
    fn scan(&self) -> Scan {
        let mut result = Scan::default();
        let mut libraries = vec![self.root.clone()];
        let config = self.root.join("steamapps/libraryfolders.vdf");
        if config.is_file() {
            match read_text(&config).and_then(|s| parse_vdf(&s)) {
                Ok(v) => {
                    if let Some(Vdf::Object(folders)) =
                        v.get("libraryfolders").or_else(|| v.get("LibraryFolders"))
                    {
                        for (k, v) in folders {
                            if k.chars().all(|c| c.is_ascii_digit())
                                && let Some(path) = v.str("path").or(v.text())
                            {
                                libraries.push(PathBuf::from(path));
                            }
                        }
                    }
                }
                Err(e) => result.warnings.push(format!("Steam: {e}")),
            }
        }
        let mut seen = std::collections::HashSet::new();
        libraries.retain(|p| seen.insert(path_key(p)));
        let history = crate::steam_library::history(&self.root);
        for library in libraries {
            let apps = library.join("steamapps");
            for p in files(&apps) {
                if p.file_name()
                    .is_none_or(|n| !n.to_string_lossy().starts_with("appmanifest_"))
                    || p.extension().is_none_or(|e| e != "acf")
                {
                    continue;
                }
                match read_text(&p).and_then(|s| steam_from_manifest(&s, &apps)) {
                    Ok(mut g) => {
                        if [
                            "Steamworks Common Redistributables",
                            "Steam Linux Runtime",
                            "Proton",
                        ]
                        .iter()
                        .any(|s| g.title.starts_with(s))
                        {
                            continue;
                        }
                        g.cover = local_steam_art(&self.root, &g.source_id, "library_600x900");
                        g.hero = local_steam_art(&self.root, &g.source_id, "library_hero");
                        if let Some(at) = history.get(&g.source_id) {
                            g.last_played = 0;
                            g.history_verified = false;
                            g.record_played(*at);
                        }
                        result.games.push(g);
                    }
                    Err(e) => result.warnings.push(format!("Steam, {}: {e}", p.display())),
                }
            }
        }
        result
    }
}

pub fn epic_from_manifest(text: &str) -> Result<Option<Game>> {
    let data: Value = serde_json::from_str(text)?;
    let strval = |k: &str| data.get(k).and_then(Value::as_str).unwrap_or_default();
    if data.get("bIsIncompleteInstall").and_then(Value::as_bool) == Some(true) {
        return Ok(None);
    }
    if !strval("MainGameAppName").is_empty() && strval("MainGameAppName") != strval("AppName") {
        return Ok(None);
    }
    let app = strval("AppName");
    let ns = strval("CatalogNamespace");
    let item = strval("CatalogItemId");
    anyhow::ensure!(
        !app.is_empty() && !strval("InstallLocation").is_empty(),
        "Неполный манифест Epic"
    );
    if strval("LaunchExecutable").is_empty() {
        return Ok(None);
    }
    fn encode(s: &str) -> String {
        s.bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    }
    let key = if !ns.is_empty() && !item.is_empty() {
        format!("{}%3A{}%3A{}", encode(ns), encode(item), encode(app))
    } else {
        encode(app)
    };
    let mut game = Game::new(
        Source::Epic,
        app.into(),
        strval("DisplayName").into(),
        LaunchTarget::uri(format!(
            "com.epicgames.launcher://apps/{key}?action=launch&silent=true"
        )),
    );
    game.install_dir = Some(PathBuf::from(strval("InstallLocation")));
    game.process_path =
        Some(PathBuf::from(strval("InstallLocation")).join(strval("LaunchExecutable")));
    game.available = game.process_path.as_ref().is_some_and(|p| p.is_file());
    game.install_target = Some(LaunchTarget::uri(format!(
        "com.epicgames.launcher://apps/{key}?action=install&silent=false"
    )));
    Ok(Some(game))
}
impl GameSource for EpicSource {
    fn scan(&self) -> Scan {
        let mut result = Scan::default();
        let catalog = self
            .manifests
            .parent()
            .and_then(|p| read_epic_catalog(&p.join("Catalog/catcache.bin")).ok());
        for p in files(&self.manifests) {
            if p.extension().is_none_or(|s| s != "item") {
                continue;
            }
            match read_text(&p).and_then(|s| epic_from_manifest(&s)) {
                Ok(Some(mut g)) => {
                    if let Some(catalog) = &catalog
                        && let Ok(text) = read_text(&p)
                        && let Ok(manifest) = serde_json::from_str::<Value>(&text)
                    {
                        apply_epic_art(&mut g, &manifest, catalog);
                    }
                    result.games.push(g);
                }
                Ok(None) => {}
                Err(e) => result.warnings.push(format!("Epic, {}: {e}", p.display())),
            }
        }
        result
    }
}

fn read_epic_catalog(path: &Path) -> Result<Value> {
    use base64::Engine;
    let text = read_text(path)?;
    if let Ok(value) = serde_json::from_str(&text) {
        return Ok(value);
    }
    let compact: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&compact)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(&compact))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn apply_epic_art(game: &mut Game, manifest: &Value, catalog: &Value) {
    let Some(items) = catalog.as_array() else {
        return;
    };
    // Names are not identities: namespaces and catalog item IDs must both match.
    let Some(item) = items.iter().find(|i| {
        i["id"].as_str().is_some()
            && i["id"] == manifest["CatalogItemId"]
            && i["namespace"].as_str().is_some()
            && i["namespace"] == manifest["CatalogNamespace"]
    }) else {
        return;
    };
    let images = item["keyImages"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let choose = |types: &[&str]| {
        types.iter().find_map(|kind| {
            images
                .iter()
                .find(|i| i["type"].as_str() == Some(kind))
                .and_then(|i| i["url"].as_str())
                .map(str::to_owned)
        })
    };
    game.store_cover_url = choose(&[
        "DieselGameBoxTall",
        "OfferImageTall",
        "Thumbnail",
        "DieselGameBox",
        "OfferImageWide",
    ]);
    game.store_hero_url = choose(&[
        "DieselGameBox",
        "OfferImageWide",
        "DieselStoreFrontWide",
        "Featured",
    ]);
}

pub fn gog_from_info(text: &str, dir: &Path, galaxy: Option<&Path>) -> Result<Option<Game>> {
    let data: Value = serde_json::from_str(text)?;
    let id = data["gameId"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| data["gameId"].as_u64().map(|n| n.to_string()))
        .context("Нет gameId")?;
    if data["rootGameId"]
        .as_str()
        .is_some_and(|root| !root.is_empty() && root != id)
    {
        return Ok(None);
    }
    let tasks = data["playTasks"]
        .as_array()
        .context("Нет действий запуска")?;
    let task = tasks
        .iter()
        .find(|t| {
            t["isPrimary"].as_bool() == Some(true)
                && t["type"].as_str().unwrap_or("FileTask") == "FileTask"
        })
        .or_else(|| {
            tasks.iter().find(|t| {
                t["path"].as_str().is_some()
                    && t["type"].as_str().unwrap_or("FileTask") == "FileTask"
            })
        });
    let Some(task) = task else {
        return Ok(None);
    };
    let path = dir.join(task["path"].as_str().context("Нет пути запуска")?);
    let working = dir.join(task["workingDir"].as_str().unwrap_or(""));
    let launch = if let Some(client) = galaxy {
        LaunchTarget::exe(
            client.to_string_lossy(),
            format!(
                "/launchViaAutostart /gameId={id} /command=runGame /path=\"{}\"",
                dir.display()
            ),
            client.parent().unwrap_or(dir).to_string_lossy(),
        )
    } else {
        LaunchTarget::exe(
            path.to_string_lossy(),
            task["arguments"].as_str().unwrap_or(""),
            working.to_string_lossy(),
        )
    };
    let mut game = Game::new(
        Source::Gog,
        id,
        data["name"].as_str().unwrap_or("Игра GOG").into(),
        launch,
    );
    game.install_dir = Some(dir.into());
    game.process_path = Some(path);
    game.available = game.process_path.as_ref().is_some_and(|p| p.is_file());
    game.install_target = Some(LaunchTarget::uri(format!(
        "goggalaxy://openGameView/{}",
        game.source_id
    )));
    Ok(Some(game))
}
impl GameSource for GogSource {
    fn scan(&self) -> Scan {
        let mut result = Scan::default();
        let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
        let galaxy = [
            "SOFTWARE\\GOG.com\\GalaxyClient\\paths",
            "SOFTWARE\\WOW6432Node\\GOG.com\\GalaxyClient\\paths",
        ]
        .iter()
        .find_map(|key| {
            machine
                .open_subkey(key)
                .ok()?
                .get_value::<String, _>("client")
                .ok()
        })
        .map(PathBuf::from)
        .filter(|p| p.is_file());
        for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            for flags in [KEY_READ | KEY_WOW64_32KEY, KEY_READ | KEY_WOW64_64KEY] {
                let Ok(uninstall) = RegKey::predef(hive).open_subkey_with_flags(
                    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                    flags,
                ) else {
                    continue;
                };
                for name in uninstall.enum_keys().flatten() {
                    let Ok(key) = uninstall.open_subkey(&name) else {
                        continue;
                    };
                    if key.get_value::<String, _>("Publisher").unwrap_or_default() != "GOG.com" {
                        continue;
                    }
                    let Some(id) = name
                        .strip_suffix("_is1")
                        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
                    else {
                        continue;
                    };
                    let Ok(dir) = key.get_value::<String, _>("InstallLocation") else {
                        continue;
                    };
                    let dir = PathBuf::from(dir);
                    if !dir.is_dir() {
                        continue;
                    }
                    let info = dir.join(format!("goggame-{id}.info"));
                    match read_text(&info).and_then(|s| gog_from_info(&s, &dir, galaxy.as_deref()))
                    {
                        Ok(Some(g)) => result.games.push(g),
                        Ok(None) => {}
                        Err(e) => result.warnings.push(format!("GOG {id}: {e}")),
                    }
                }
            }
        }
        result.games.sort_by(|a, b| a.id.cmp(&b.id));
        result.games.dedup_by(|a, b| a.id == b.id);
        result
    }
}

pub fn scan_all() -> Scan {
    scan_library(false, &[])
}
pub fn scan_library(include_uninstalled: bool, previous: &[Game]) -> Scan {
    let mut result = Scan::default();
    let steam = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Valve\\Steam")
        .ok()
        .and_then(|k| k.get_value::<String, _>("SteamPath").ok());
    let epic = std::env::var_os("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:/ProgramData"))
        .join("Epic/EpicGamesLauncher/Data/Manifests");
    let mut providers: Vec<Box<dyn GameSource>> = vec![
        Box::new(EpicSource { manifests: epic }),
        Box::new(GogSource),
    ];
    if let Some(root) = &steam {
        providers.push(Box::new(SteamSource { root: root.into() }));
    }
    for provider in providers {
        let scan = provider.scan();
        result.games.extend(scan.games);
        result.warnings.extend(scan.warnings);
    }
    if include_uninstalled && let Some(root) = &steam {
        match crate::steam_library::read_owned(Path::new(root)) {
            Ok(owned) => {
                if owned.missing_metadata == 0 {
                    result.owned_sources.push(Source::Steam);
                } else {
                    result.warnings.push(format!("Кэш Steam неполный: {} записей без метаданных. Откройте библиотеку Steam, дождитесь обновления и повторите сканирование.",owned.missing_metadata));
                }
                for mut game in owned.games {
                    if let Some(installed) = result
                        .games
                        .iter_mut()
                        .find(|g| g.source == game.source && g.source_id == game.source_id)
                    {
                        installed.owned = true;
                        if game.history_verified {
                            installed.record_played(game.last_played);
                        }
                    } else {
                        game.installed = false;
                        result.games.push(game);
                    }
                }
            }
            Err(error) => result
                .warnings
                .push(format!("Вся библиотека Steam: {error}")),
        }
    }
    for old in previous {
        if result
            .games
            .iter()
            .any(|g| g.source == old.source && g.source_id == old.source_id)
        {
            continue;
        }
        let mut game = old.clone();
        if result.owned_sources.contains(&game.source) {
            game.owned = false;
        }
        if game.source == Source::Manual {
            game.available =
                game.target().kind == LaunchKind::Uri || Path::new(&game.target().path).is_file();
        } else if let Some(dir) = &game.install_dir {
            let volume = dir.ancestors().last().unwrap_or(dir);
            if volume.is_dir() {
                game.installed = false;
            }
            game.available = false;
        } else {
            game.available = false;
        }
        result.games.push(game);
    }
    result.games.sort_by(|a, b| a.id.cmp(&b.id));
    result.games.dedup_by(|a, b| {
        if a.id == b.id {
            b.record_played(a.last_played);
            true
        } else {
            false
        }
    });
    result
}
pub fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}
pub fn from_file(path: &Path, icon_cache: &Path) -> Result<Game> {
    let name = path
        .file_stem()
        .context("Нет имени файла")?
        .to_string_lossy()
        .into_owned();
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let (launch, install) = match ext.as_str() {
        "exe" => (
            LaunchTarget::exe(
                path.to_string_lossy(),
                "",
                path.parent().unwrap_or(Path::new("")).to_string_lossy(),
            ),
            path.parent().map(Path::to_path_buf),
        ),
        "lnk" => {
            let link = platform::read_shortcut(path)?;
            (
                link.target,
                link.executable
                    .as_ref()
                    .and_then(|p| p.parent().map(Path::to_path_buf)),
            )
        }
        "url" => {
            let text = read_text(path)?;
            let uri = text
                .lines()
                .find_map(|l| {
                    l.trim()
                        .split_once('=')
                        .filter(|(k, _)| k.eq_ignore_ascii_case("URL"))
                        .map(|(_, v)| v.trim())
                })
                .context("Нет игровой ссылки в ярлыке")?;
            let target = LaunchTarget::uri(uri);
            target.validate()?;
            (target, None)
        }
        _ => anyhow::bail!("Выберите .exe, .lnk или игровой .url"),
    };
    let key = launch.key();
    let mut game = Game::new(Source::Manual, stable_hash(&key), name, launch);
    game.install_dir = install;
    game.process_path = match game.launch.kind {
        LaunchKind::Executable => Some(PathBuf::from(&game.launch.path)),
        LaunchKind::ShellLink => platform::read_shortcut(path)
            .ok()
            .and_then(|s| s.executable),
        LaunchKind::Uri => None,
    };
    game.icon = platform::cache_icon(path, icon_cache).ok();
    Ok(game)
}
pub fn desktop(icon_cache: &Path) -> Scan {
    let mut scan = Scan::default();
    for dir in platform::desktop_dirs() {
        for p in files(&dir) {
            let ext = p
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            if !["exe", "lnk", "url"].contains(&ext.as_str()) {
                continue;
            }
            match from_file(&p, icon_cache) {
                Ok(g) => scan.games.push(g),
                Err(e) => scan.warnings.push(format!("{}: {e}", p.display())),
            }
        }
    }
    scan.games.sort_by_key(|g| g.name().to_lowercase());
    scan.games.dedup_by(|a, b| a.launch.key() == b.launch.key());
    scan
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn steam_capsule_in_hash_directory_and_exact_hero() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("appcache/librarycache/42/asset-hash");
        std::fs::create_dir_all(&cache).unwrap();
        for file in [
            "library_capsule.jpg",
            "library_hero_blur.jpg",
            "library_hero.jpg",
        ] {
            std::fs::write(cache.join(file), b"fixture").unwrap();
        }
        assert_eq!(
            local_steam_art(root.path(), "42", "library_600x900"),
            Some(cache.join("library_capsule.jpg"))
        );
        assert_eq!(
            local_steam_art(root.path(), "42", "library_hero"),
            Some(cache.join("library_hero.jpg"))
        );
        assert!(local_steam_art(root.path(), "43", "library_600x900").is_none());
    }
    #[test]
    fn steam_legacy_and_header_fallback_remain_supported() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("appcache/librarycache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("42_library_600x900.png"), b"legacy").unwrap();
        std::fs::write(cache.join("43_header.jpg"), b"header").unwrap();
        assert_eq!(
            local_steam_art(root.path(), "42", "library_600x900"),
            Some(cache.join("42_library_600x900.png"))
        );
        assert_eq!(
            local_steam_art(root.path(), "43", "library_600x900"),
            Some(cache.join("43_header.jpg"))
        );
    }
    #[test]
    fn epic_catalog_decodes_base64_and_matches_both_ids() {
        use base64::Engine;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catcache.bin");
        let catalog = serde_json::json!([
            {"id":"item","namespace":"wrong","keyImages":[{"type":"DieselGameBoxTall","url":"https://cdn1.epicgames.com/wrong"}]},
            {"id":"item","namespace":"ns","keyImages":[{"type":"DieselGameBox","url":"https://cdn1.epicgames.com/wide"},{"type":"DieselGameBoxTall","url":"https://cdn1.epicgames.com/tall"}]}
        ]);
        let mut game = Game::new(
            Source::Epic,
            "app".into(),
            "same name".into(),
            LaunchTarget::uri("com.epicgames.launcher://apps/app"),
        );
        let manifest = serde_json::json!({"CatalogItemId":"item","CatalogNamespace":"ns"});
        std::fs::write(
            &path,
            base64::engine::general_purpose::STANDARD.encode(catalog.to_string()),
        )
        .unwrap();
        apply_epic_art(&mut game, &manifest, &read_epic_catalog(&path).unwrap());
        assert_eq!(
            game.store_cover_url.as_deref(),
            Some("https://cdn1.epicgames.com/tall")
        );
        assert_eq!(
            game.store_hero_url.as_deref(),
            Some("https://cdn1.epicgames.com/wide")
        );
        std::fs::write(&path, catalog.to_string()).unwrap();
        assert!(read_epic_catalog(&path).unwrap().is_array());
    }
    #[test]
    fn vdf_paths_and_comments() {
        let v = parse_vdf("// hello\n\"libraryfolders\" { \"0\" { \"path\" \"D:\\\\Игры\" } }")
            .unwrap();
        assert_eq!(
            v.get("libraryfolders")
                .unwrap()
                .get("0")
                .unwrap()
                .str("path"),
            Some("D:\\Игры")
        );
        assert!(parse_vdf("\"x\" { \"y\" \"z\"").is_err());
    }
    #[test]
    fn steam_multiple_fields() {
        let g = steam_from_manifest(
            r#""AppState" { "appid" "42" "name" "Игра с пробелами" "installdir" "My Game" }"#,
            Path::new("D:/steamapps"),
        )
        .unwrap();
        assert_eq!(g.source_id, "42");
        assert!(g.install_dir.unwrap().ends_with("My Game"));
    }
    #[test]
    fn epic_ignores_incomplete_and_dlc() {
        assert!(
            epic_from_manifest(r#"{"bIsIncompleteInstall":true}"#)
                .unwrap()
                .is_none()
        );
        let g=epic_from_manifest(r#"{"AppName":"a","CatalogNamespace":"n","CatalogItemId":"i","DisplayName":"Game","InstallLocation":"D:/Game","LaunchExecutable":"g.exe"}"#).unwrap().unwrap();
        assert!(g.launch.path.contains("n%3Ai%3Aa"));
        assert!(epic_from_manifest("{}").is_err());
    }
    #[test]
    fn gog_retains_arguments_and_working_directory() {
        let g=gog_from_info(r#"{"gameId":"12","name":"Test","playTasks":[{"isPrimary":true,"type":"FileTask","path":"bin/game.exe","arguments":"--test A","workingDir":"bin"}]}"#,Path::new("D:/Игры/G"),None).unwrap().unwrap();
        assert_eq!(g.launch.args, "--test A");
        assert!(g.launch.cwd.ends_with("bin"));
    }
    #[test]
    fn utf16_shortcut_text() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("тест.url");
        let mut b = vec![255, 254];
        for c in "[InternetShortcut]\r\nURL=steam://rungameid/42".encode_utf16() {
            b.extend(c.to_le_bytes());
        }
        std::fs::write(&p, b).unwrap();
        assert!(read_text(&p).unwrap().contains("steam://rungameid/42"));
    }
}
