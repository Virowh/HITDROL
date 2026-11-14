//! One-time, non-destructive import from the previous product's local profile.
use crate::{branding, model::Game, storage::Database};
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

// Compatibility bytes identify the existing on-disk profile and credential only.
// All current product names and newly created identifiers come from branding.
pub(crate) fn previous_product_id() -> &'static str {
    "\x4f\x72\x62\x69\x74Launcher"
}

pub fn prepare(local: &Path) -> Result<PathBuf> {
    let root = local.join(branding::PROFILE);
    import(&local.join(previous_product_id()), &root)?;
    Ok(root)
}

fn copy_cache(source: &Path, target: &Path) -> Result<()> {
    if !source.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)?.flatten() {
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        let dest = target.join(entry.file_name());
        if kind.is_dir() {
            copy_cache(&entry.path(), &dest)?;
        } else if kind.is_file() && !dest.exists() {
            std::fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

fn relocate(path: &mut Option<PathBuf>, previous: &Path, current: &Path) {
    if let Some(original) = path
        && let Ok(relative) = original.strip_prefix(previous)
    {
        *original = current.join(relative);
    }
}

pub fn import(previous: &Path, current: &Path) -> Result<bool> {
    if current.join("library.db").exists() || !previous.join("library.db").is_file() {
        return Ok(false);
    }
    ensure!(previous != current, "Папки профилей должны различаться");
    std::fs::create_dir_all(current)?;
    let staging = current.join(format!(
        ".import-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir(&staging)?;
    // SQLite's backup API includes committed WAL data even while the old app is open.
    let source = rusqlite::Connection::open_with_flags(
        previous.join("library.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    source.busy_timeout(std::time::Duration::from_secs(3))?;
    source
        .backup("main", staging.join("library.db"), None)
        .context("Не удалось скопировать библиотеку предыдущей версии")?;
    drop(source);
    for name in ["artwork", "icons", "wallpapers"] {
        copy_cache(&previous.join(name), &current.join(name))?;
    }
    let db = Database::open(&staging)?;
    for mut game in db.games()? {
        relocate_game(&mut game, previous, current);
        db.save(&game)?;
    }
    let mut settings = db.settings()?;
    relocate(&mut settings.background_image, previous, current);
    db.save_settings(&settings)?;
    drop(db);
    // Publish the library only after the snapshot and caches are complete.
    std::fs::rename(staging.join("library.db"), current.join("library.db"))?;
    let _ = std::fs::remove_dir(&staging);
    Ok(true)
}

fn relocate_game(game: &mut Game, previous: &Path, current: &Path) {
    for path in [
        &mut game.cover,
        &mut game.hero,
        &mut game.icon,
        &mut game.user_cover,
        &mut game.user_hero,
    ] {
        relocate(path, previous, current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BackgroundMode, LaunchTarget, Settings, Source};
    #[test]
    fn import_includes_wal_customizations_and_images_without_changing_original() {
        let temp = tempfile::tempdir().unwrap();
        let previous = temp.path().join("previous");
        let current = temp.path().join(branding::PROFILE);
        let old = Database::open(&previous).unwrap();
        let image = previous.join("wallpapers/свои обои.png");
        std::fs::create_dir_all(image.parent().unwrap()).unwrap();
        std::fs::write(&image, b"image fixture").unwrap();
        let mut game = Game::new(
            Source::Manual,
            "example".into(),
            "Игра".into(),
            LaunchTarget::exe("D:/Мои игры/game.exe", "--mode test", "D:/Мои игры"),
        );
        game.favorite = true;
        game.user_title = Some("Моё название".into());
        game.user_cover = Some(image.clone());
        old.save(&game).unwrap();
        old.save_settings(&Settings {
            background_mode: BackgroundMode::Image,
            background_image: Some(image.clone()),
            ..Default::default()
        })
        .unwrap();
        assert!(import(&previous, &current).unwrap());
        let new = Database::open(&current).unwrap();
        let saved = &new.games().unwrap()[0];
        assert!(saved.favorite);
        assert_eq!(saved.name(), "Моё название");
        assert_eq!(saved.launch, game.launch);
        let copied = current.join("wallpapers/свои обои.png");
        assert_eq!(saved.user_cover, Some(copied.clone()));
        assert_eq!(std::fs::read(&copied).unwrap(), b"image fixture");
        assert_eq!(new.settings().unwrap().background_image, Some(copied));
        assert_eq!(old.games().unwrap()[0].user_cover, Some(image));
        assert!(!import(&previous, &current).unwrap());
    }
    #[test]
    fn existing_profile_and_external_images_are_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join("old");
        let new = temp.path().join("new");
        Database::open(&old).unwrap();
        let db = Database::open(&new).unwrap();
        db.save_settings(&Settings {
            fps: 144,
            ..Default::default()
        })
        .unwrap();
        assert!(!import(&old, &new).unwrap());
        assert_eq!(db.settings().unwrap().fps, 144);
        let mut path = Some(PathBuf::from("D:/свои изображения/фон.png"));
        let before = path.clone();
        relocate(&mut path, &old, &new);
        assert_eq!(path, before);
    }
}
