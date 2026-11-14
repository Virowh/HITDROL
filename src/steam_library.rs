//! Read-only account library and play history from the installed Steam client.
use crate::{
    discovery::{self, Vdf},
    model::*,
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

pub fn account_folder(root: &Path) -> Option<PathBuf> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    if let Some(id) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Valve\\Steam\\ActiveProcess")
        .ok()
        .and_then(|k| k.get_value::<u32, _>("ActiveUser").ok())
        .filter(|n| *n > 0)
    {
        let p = root.join("userdata").join(id.to_string());
        if p.join("config/localconfig.vdf").is_file() {
            return Some(p);
        }
    }
    if let Ok(data) = discovery::read_text(&root.join("config/loginusers.vdf"))
        .and_then(|s| discovery::parse_vdf(&s))
        && let Some(Vdf::Object(users)) = data.get("users")
    {
        for (id, user) in users {
            if user.str("MostRecent") == Some("1")
                && let Ok(id) = id.parse::<u64>()
            {
                let p = root.join("userdata").join((id & 0xffff_ffff).to_string());
                if p.is_dir() {
                    return Some(p);
                }
            }
        }
    }
    std::fs::read_dir(root.join("userdata"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|s| s.to_string_lossy().bytes().all(|b| b.is_ascii_digit()))
        })
        .filter_map(|p| {
            std::fs::metadata(p.join("config/localconfig.vdf"))
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|time| (time, p))
        })
        .max_by_key(|(time, _)| *time)
        .map(|(_, p)| p)
}
pub fn history(root: &Path) -> HashMap<String, u64> {
    account_folder(root)
        .and_then(|p| discovery::read_text(&p.join("config/localconfig.vdf")).ok())
        .and_then(|s| parse_history(&s).ok())
        .unwrap_or_default()
}
fn parse_history(text: &str) -> Result<HashMap<String, u64>> {
    let tree = discovery::parse_vdf(text)?;
    let mut node = &tree;
    for key in ["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"] {
        node = node.get(key).context("В Steam пока нет истории запусков")?;
    }
    let mut result = HashMap::new();
    if let Vdf::Object(apps) = node {
        for (id, app) in apps {
            if id.bytes().all(|b| b.is_ascii_digit())
                && let Some(at) = app.str("LastPlayed").and_then(|s| s.parse::<u64>().ok())
            {
                result.insert(id.clone(), if valid_played(at) { at } else { 0 });
            }
        }
    }
    Ok(result)
}
fn limited_read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    ensure!(
        std::fs::metadata(path)?.len() <= limit,
        "Кэш Steam слишком большой"
    );
    Ok(std::fs::read(path)?)
}

/// Park–Miller sequence with a 32-entry shuffle, used as a reversible account-ID mask.
/// Only package IDs and validity flags are decoded; session/access-token fields are skipped.
struct Mask {
    state: i64,
    table: [i64; 32],
    selected: i64,
}
impl Mask {
    fn new(account: u32) -> Self {
        let mut state = i64::from(account as i32).abs().max(1);
        let mut table = [0; 32];
        for slot in (0..40).rev() {
            state = (state * 16807) % 2147483647;
            if slot < 32 {
                table[slot] = state;
            }
        }
        Self {
            state,
            selected: table[0],
            table,
        }
    }
    fn next(&mut self) -> u8 {
        loop {
            self.state = (self.state * 16807) % 2147483647;
            let slot = (self.selected / 67108864) as usize;
            self.selected = std::mem::replace(&mut self.table[slot], self.state);
            if self.selected <= 2147483647 - (2147483648i64 % 95) {
                return (32 + self.selected % 95) as u8;
            }
        }
    }
}
fn varint(bytes: &[u8], pos: &mut usize) -> Result<u64> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let b = *bytes.get(*pos).context("Обрезанный кэш лицензий Steam")?;
        *pos += 1;
        ensure!(shift < 63 || b <= 1, "Переполнение кэша лицензий Steam");
        value |= u64::from(b & 127) << shift;
        if b & 128 == 0 {
            return Ok(value);
        }
    }
    anyhow::bail!("Некорректный кэш лицензий Steam")
}
fn fields(
    mut bytes: &[u8],
    mut visit: impl FnMut(u64, Option<u64>, Option<&[u8]>) -> Result<()>,
) -> Result<()> {
    while !bytes.is_empty() {
        let mut pos = 0;
        let tag = varint(bytes, &mut pos)?;
        ensure!(tag >> 3 > 0, "Некорректная запись лицензии");
        let (number, data) = match tag & 7 {
            0 => (Some(varint(bytes, &mut pos)?), None),
            1 => {
                pos += 8;
                (None, None)
            }
            2 => {
                let size = usize::try_from(varint(bytes, &mut pos)?)?;
                let end = pos.checked_add(size).context("Размер лицензии")?;
                let data = bytes.get(pos..end).context("Обрезанная запись лицензии")?;
                pos = end;
                (None, Some(data))
            }
            5 => {
                pos += 4;
                (None, None)
            }
            _ => anyhow::bail!("Неизвестный формат кэша лицензий"),
        };
        ensure!(pos <= bytes.len(), "Обрезанный кэш лицензий");
        visit(tag >> 3, number, data)?;
        bytes = &bytes[pos..];
    }
    Ok(())
}
fn licenses(bytes: &[u8], account: u32) -> Result<HashSet<u32>> {
    ensure!(
        bytes.len() > 4 && bytes.len() < 32 * 1024 * 1024,
        "Некорректный размер кэша лицензий"
    );
    let mut mask = Mask::new(account);
    let mut decoded: Vec<_> = bytes.iter().map(|b| b ^ mask.next()).collect();
    let mut result = HashSet::new();
    let mut success = false;
    let parsed = fields(&decoded[..decoded.len() - 4], |field, number, data| {
        if field == 1 {
            success = number == Some(1);
        }
        if field == 2
            && let Some(data) = data
        {
            let mut package = 0;
            let mut flags = 0;
            fields(data, |field, number, _| {
                if field == 1 {
                    package = number.unwrap_or(0) as u32;
                }
                if field == 7 {
                    flags = number.unwrap_or(0);
                }
                Ok(())
            })?;
            if package > 0
                && flags & (0x02 | 0x04 | 0x08 | 0x10 | 0x20 | 0x400 | 0x800 | 0x2000 | 0x40000)
                    == 0
            {
                result.insert(package);
            }
        }
        Ok(())
    });
    decoded.fill(0);
    parsed?;
    ensure!(success, "Steam не подтвердил список лицензий");
    Ok(result)
}
pub fn owned(root: &Path) -> Result<Vec<Game>> {
    Ok(read_owned(root)?.games)
}
pub struct OwnedLibrary {
    pub games: Vec<Game>,
    pub missing_metadata: usize,
}
pub fn read_owned(root: &Path) -> Result<OwnedLibrary> {
    let account = account_folder(root)
        .context("Запустите Steam и войдите в аккаунт, затем обновите библиотеку")?;
    let id = account
        .file_name()
        .context("Нет аккаунта Steam")?
        .to_string_lossy()
        .parse::<u32>()?;
    let packages = licenses(
        &limited_read(&account.join("config/licensecache"), 32 * 1024 * 1024)?,
        id,
    )?;
    let package_bytes = limited_read(&root.join("appcache/packageinfo.vdf"), 256 * 1024 * 1024)?;
    let package_data = steam_vdf_parser::parse_packageinfo(&package_bytes)
        .map_err(|e| anyhow::anyhow!("Кэш пакетов Steam: {e}"))?;
    let mut appids = HashSet::new();
    let mut missing_metadata = 0;
    for package in packages {
        if let Some(entry) = package_data.get(&package.to_string()) {
            let entry = entry.get(&package.to_string()).unwrap_or(entry);
            if let Some(apps) = entry.get("appids").and_then(|v| v.as_obj()) {
                for (_, value) in apps.iter() {
                    if let Some(app) = value.as_i32().filter(|n| *n > 0) {
                        appids.insert(app.to_string());
                    }
                }
            }
        } else {
            missing_metadata += 1;
        }
    }
    let app_bytes = limited_read(&root.join("appcache/appinfo.vdf"), 512 * 1024 * 1024)?;
    let apps = steam_vdf_parser::parse_appinfo(&app_bytes)
        .map_err(|e| anyhow::anyhow!("Кэш приложений Steam: {e}"))?;
    let history = history(root);
    let mut result = vec![];
    for id in appids {
        let Some(entry) = apps.get(&id) else {
            missing_metadata += 1;
            continue;
        };
        let entry = entry.get("appinfo").unwrap_or(entry);
        let Some(name) = entry.get_str(&["common", "name"]) else {
            missing_metadata += 1;
            continue;
        };
        let kind = entry
            .get_str(&["common", "type"])
            .unwrap_or("game")
            .to_lowercase();
        if !["game", "application", "demo"].contains(&kind.as_str()) {
            continue;
        }
        let mut game = Game::new(
            Source::Steam,
            id.clone(),
            name.into(),
            LaunchTarget::uri(format!("steam://run/{id}")),
        );
        game.installed = false;
        game.available = false;
        game.owned = true;
        game.install_target = Some(LaunchTarget::uri(format!("steam://install/{id}")));
        game.cover = discovery::local_steam_art(root, &id, "library_600x900");
        game.hero = discovery::local_steam_art(root, &id, "library_hero");
        if let Some(at) = history.get(&id) {
            game.record_played(*at);
        }
        result.push(game);
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(OwnedLibrary {
        games: result,
        missing_metadata,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn steam_history_is_case_insensitive_and_never_uses_import_time() {
        let at = timestamp() - 3600;
        let text = format!(
            r#""UserLocalConfigStore" {{ "Software" {{ "Valve" {{ "Steam" {{ "Apps" {{ "42" {{ "LastPlayed" "{at}" }} "43" {{ "LastPlayed" "0" }} }} }} }} }} }}"#
        );
        let history = parse_history(&text).unwrap();
        assert_eq!(history["42"], at);
        assert_eq!(history["43"], 0);
    }
    #[test]
    fn licenses_reject_garbage_and_expired_entries() {
        assert!(licenses(b"garbage", 42).is_err());
        let plain = [8, 1, 18, 4, 8, 42, 56, 0, 18, 4, 8, 43, 56, 8, 0, 0, 0, 0];
        let mut mask = Mask::new(42);
        let encoded: Vec<_> = plain.iter().map(|b| b ^ mask.next()).collect();
        assert_eq!(licenses(&encoded, 42).unwrap(), HashSet::from([42]));
        assert!(licenses(&encoded, 43).is_err());
    }
}
