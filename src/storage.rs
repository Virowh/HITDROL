use crate::model::{Game, Settings};
use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};

pub struct Database {
    conn: Connection,
    pub root: PathBuf,
}
impl Database {
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let conn = Connection::open(root.join("library.db"))?;
        conn.busy_timeout(std::time::Duration::from_secs(3))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS games(id TEXT PRIMARY KEY, target_key TEXT NOT NULL, data TEXT NOT NULL); CREATE INDEX IF NOT EXISTS target_idx ON games(target_key); CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, data TEXT NOT NULL); PRAGMA user_version=1;")?;
        Ok(Self {
            conn,
            root: root.into(),
        })
    }
    pub fn games(&self) -> Result<Vec<Game>> {
        let mut stmt = self.conn.prepare("SELECT data FROM games ORDER BY id")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|s| serde_json::from_str(&s?).context("Повреждена запись библиотеки"))
            .collect()
    }
    pub fn save(&self, game: &Game) -> Result<()> {
        self.conn.execute("INSERT INTO games(id,target_key,data) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET target_key=excluded.target_key,data=excluded.data",params![game.id,game.launch.key(),serde_json::to_string(game)?])?;
        Ok(())
    }
    pub fn merge(&mut self, incoming: Vec<Game>) -> Result<Vec<Game>> {
        self.merge_library(incoming, &[], false)
    }
    /// Ownership is replaced only after a successful authoritative response.
    /// Account metadata must never downgrade an existing local installation.
    pub fn merge_library(
        &mut self,
        incoming: Vec<Game>,
        authoritative: &[crate::model::Source],
        metadata_only: bool,
    ) -> Result<Vec<Game>> {
        let mut games = self.games()?;
        for game in &mut games {
            if authoritative.contains(&game.source) {
                game.owned = false;
            }
        }
        let mut by_id: std::collections::HashMap<String, usize> = games
            .iter()
            .enumerate()
            .flat_map(|(i, g)| {
                [
                    (g.id.clone(), i),
                    (format!("{:?}:{}", g.source, g.source_id), i),
                ]
            })
            .collect();
        let mut by_target: std::collections::HashMap<String, usize> = games
            .iter()
            .enumerate()
            .map(|(i, g)| (g.launch.key(), i))
            .collect();
        let tx = self.conn.transaction()?;
        for game in games.iter().filter(|g| authoritative.contains(&g.source)) {
            tx.execute(
                "UPDATE games SET data=?2 WHERE id=?1",
                params![game.id, serde_json::to_string(game)?],
            )?;
        }
        for mut new in incoming {
            let index = by_id
                .get(&new.id)
                .or_else(|| by_id.get(&format!("{:?}:{}", new.source, new.source_id)))
                .or_else(|| by_target.get(&new.launch.key()))
                .copied();
            if let Some(index) = index {
                let old = &mut games[index];
                if metadata_only {
                    new.installed = old.installed;
                    new.available = old.available;
                    new.install_dir = old.install_dir.clone();
                    new.process_path = old.process_path.clone();
                    if old.installed {
                        new.launch = old.launch.clone();
                    }
                }
                by_target.remove(&old.launch.key());
                new.id = old.id.clone();
                new.favorite = old.favorite;
                if old.history_verified {
                    new.record_played(old.last_played);
                }
                new.owned |= old.owned;
                new.install_target = new.install_target.or(old.install_target.clone());
                new.process_path = new.process_path.or(old.process_path.clone());
                new.user_title = old.user_title.clone();
                new.user_launch = old.user_launch.clone();
                new.user_cover = old.user_cover.clone();
                new.user_hero = old.user_hero.clone();
                new.cover = new.cover.or(old.cover.clone());
                new.hero = new.hero.or(old.hero.clone());
                new.icon = new.icon.or(old.icon.clone());
                new.store_cover_url = new.store_cover_url.or(old.store_cover_url.clone());
                new.store_hero_url = new.store_hero_url.or(old.store_hero_url.clone());
                // A desktop rescan must not downgrade a recognized store identity.
                if new.source == crate::model::Source::Manual
                    && old.source != crate::model::Source::Manual
                {
                    new.source = old.source;
                    new.source_id = old.source_id.clone();
                    new.launch = old.launch.clone();
                    new.install_dir = old.install_dir.clone();
                }
                *old = new.clone();
                by_id.insert(new.id.clone(), index);
                by_id.insert(format!("{:?}:{}", new.source, new.source_id), index);
                by_target.insert(new.launch.key(), index);
            } else {
                by_id.insert(new.id.clone(), games.len());
                by_id.insert(format!("{:?}:{}", new.source, new.source_id), games.len());
                by_target.insert(new.launch.key(), games.len());
                games.push(new.clone());
            }
            tx.execute("INSERT INTO games(id,target_key,data) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET target_key=excluded.target_key,data=excluded.data",params![new.id,new.launch.key(),serde_json::to_string(&new)?])?;
        }
        tx.commit()?;
        Ok(games)
    }
    pub fn settings(&self) -> Result<Settings> {
        use rusqlite::OptionalExtension;
        let s: Option<String> = self
            .conn
            .query_row("SELECT data FROM settings WHERE key='ui'", [], |r| r.get(0))
            .optional()?;
        let mut value = s
            .map(|s| serde_json::from_str::<Settings>(&s))
            .transpose()?
            .unwrap_or_default();
        value.sanitize();
        Ok(value)
    }
    /// Disconnect account ownership without deleting installed games, history or edits.
    pub fn clear_store_cache(&mut self, sources: &[crate::model::Source]) -> Result<usize> {
        let games = self.games()?;
        let tx = self.conn.transaction()?;
        let mut removed = 0;
        tx.execute_batch("PRAGMA secure_delete=ON;")?;
        for mut game in games.into_iter().filter(|g| sources.contains(&g.source)) {
            let custom = game.last_played != 0
                || game.favorite
                || game.user_title.is_some()
                || game.user_launch.is_some()
                || game.user_cover.is_some()
                || game.user_hero.is_some();
            if !game.installed && !custom {
                removed += tx.execute("DELETE FROM games WHERE id=?1", [game.id])?;
            } else {
                game.owned = false;
                tx.execute(
                    "UPDATE games SET data=?2 WHERE id=?1",
                    params![game.id, serde_json::to_string(&game)?],
                )?;
            }
        }
        tx.commit()?;
        self.conn.execute_batch(
            "PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA wal_checkpoint(TRUNCATE);",
        )?;
        Ok(removed)
    }
    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        self.conn.execute("INSERT INTO settings(key,data) VALUES('ui',?1) ON CONFLICT(key) DO UPDATE SET data=excluded.data",[serde_json::to_string(s)?])?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    #[test]
    fn clearing_account_cache_keeps_installs_customizations_and_other_sources() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Database::open(dir.path()).unwrap();
        let mut installed = Game::new(
            Source::Steam,
            "1".into(),
            "Installed".into(),
            LaunchTarget::uri("steam://run/1"),
        );
        installed.owned = true;
        installed.user_title = Some("Моё имя".into());
        installed.record_played(timestamp() - 60);
        let mut cached = Game::new(
            Source::Steam,
            "2".into(),
            "Cached".into(),
            LaunchTarget::uri("steam://run/2"),
        );
        cached.installed = false;
        cached.owned = true;
        let mut favorite = Game::new(
            Source::Epic,
            "3".into(),
            "Favorite".into(),
            LaunchTarget::uri("com.epicgames.launcher://apps/3"),
        );
        favorite.installed = false;
        favorite.owned = true;
        favorite.favorite = true;
        let mut other = Game::new(
            Source::Gog,
            "4".into(),
            "Other".into(),
            LaunchTarget::uri("goggalaxy://openGameView/4"),
        );
        other.installed = false;
        other.owned = true;
        let mut history = Game::new(
            Source::Epic,
            "5".into(),
            "Previously played".into(),
            LaunchTarget::uri("com.epicgames.launcher://apps/5"),
        );
        history.installed = false;
        history.owned = true;
        history.record_played(timestamp() - 120);
        for game in [&installed, &cached, &favorite, &other, &history] {
            db.save(game).unwrap();
        }
        assert_eq!(
            db.clear_store_cache(&[Source::Steam, Source::Epic])
                .unwrap(),
            1
        );
        let games = db.games().unwrap();
        assert_eq!(games.len(), 4);
        let game = games.iter().find(|g| g.id == installed.id).unwrap();
        assert_eq!(game.name(), "Моё имя");
        assert!(game.installed && game.history_verified && !game.owned);
        let game = games.iter().find(|g| g.id == favorite.id).unwrap();
        assert!(game.favorite && !game.owned);
        assert!(games.iter().find(|g| g.id == other.id).unwrap().owned);
        let game = games.iter().find(|g| g.id == history.id).unwrap();
        assert!(!game.installed && !game.owned && game.history_verified);
        assert_eq!(game.last_played, history.last_played);
        assert_eq!(
            db.clear_store_cache(&[Source::Steam, Source::Epic])
                .unwrap(),
            0
        );
    }
    #[test]
    fn store_identity_deduplicates_even_when_saved_id_and_launch_uri_changed() {
        let d = tempfile::tempdir().unwrap();
        let mut db = Database::open(d.path()).unwrap();
        let mut saved = Game::new(
            Source::Epic,
            "app42".into(),
            "Game".into(),
            LaunchTarget::uri("com.epicgames.launcher://apps/app42?action=launch"),
        );
        saved.id = "original-shortcut-id".into();
        saved.favorite = true;
        db.save(&saved).unwrap();
        let new = Game::new(
            Source::Epic,
            "app42".into(),
            "New title".into(),
            LaunchTarget::uri(
                "com.epicgames.launcher://apps/ns%3Aitem%3Aapp42?action=launch&silent=true",
            ),
        );
        let games = db.merge(vec![new]).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].id, saved.id);
        assert!(games[0].favorite);
    }
    #[test]
    fn account_sync_keeps_installation_and_replaces_only_authoritative_ownership() {
        let d = tempfile::tempdir().unwrap();
        let mut db = Database::open(d.path()).unwrap();
        let mut installed = Game::new(
            Source::Gog,
            "42".into(),
            "Old".into(),
            LaunchTarget::exe("D:/Игра/game.exe", "--x", "D:/Игра"),
        );
        installed.install_dir = Some("D:/Игра".into());
        installed.owned = true;
        installed.favorite = true;
        installed.user_title = Some("Моё название".into());
        installed.record_played(timestamp() - 30);
        db.save(&installed).unwrap();
        let mut metadata = Game::new(
            Source::Gog,
            "42".into(),
            "New".into(),
            LaunchTarget::uri("goggalaxy://openGameView/42"),
        );
        metadata.installed = false;
        metadata.available = false;
        metadata.owned = true;
        let games = db
            .merge_library(vec![metadata], &[Source::Gog], true)
            .unwrap();
        let game = &games[0];
        assert!(game.installed && game.available && game.owned && game.favorite);
        assert_eq!(game.launch, installed.launch);
        assert_eq!(game.install_dir, installed.install_dir);
        assert_eq!(game.name(), "Моё название");
        assert_eq!(game.last_played, installed.last_played);
        assert!(game.history_verified);
        let games = db.merge_library(vec![], &[], true).unwrap();
        assert!(
            games[0].owned,
            "failed/offline sync does not revoke cached ownership"
        );
        let games = db.merge_library(vec![], &[Source::Gog], true).unwrap();
        assert!(!games[0].owned);
        assert!(games[0].installed);
        assert!(!db.games().unwrap()[0].owned, "disconnect remains saved");
    }
    #[test]
    fn rescan_preserves_newer_verified_history_and_replaces_legacy_clicks() {
        let d = tempfile::tempdir().unwrap();
        let mut db = Database::open(d.path()).unwrap();
        let mut old = Game::new(
            Source::Steam,
            "42".into(),
            "Game".into(),
            LaunchTarget::uri("steam://run/42"),
        );
        old.last_played = timestamp();
        db.save(&old).unwrap();
        let mut scan = old.clone();
        scan.record_played(timestamp() - 86400);
        let games = db.merge(vec![scan.clone()]).unwrap();
        assert_eq!(games[0].last_played, scan.last_played);
        scan.record_played(timestamp() - 20);
        db.merge(vec![scan.clone()]).unwrap();
        let games = db.merge(vec![old]).unwrap();
        assert_eq!(games[0].last_played, scan.last_played);
        assert!(games[0].history_verified);
    }
    #[test]
    fn rescan_preserves_edits_and_missing_games() {
        let d = tempfile::tempdir().unwrap();
        let mut db = Database::open(d.path()).unwrap();
        let mut game = Game::new(
            Source::Steam,
            "1".into(),
            "Original".into(),
            LaunchTarget::uri("steam://run/1"),
        );
        db.merge(vec![game.clone()]).unwrap();
        game.user_title = Some("Моя игра".into());
        game.favorite = true;
        game.user_launch = Some(LaunchTarget::uri("steam://run/2"));
        db.save(&game).unwrap();
        let games = db
            .merge(vec![Game::new(
                Source::Steam,
                "1".into(),
                "Updated".into(),
                game.launch.clone(),
            )])
            .unwrap();
        assert_eq!(games[0].name(), "Моя игра");
        assert!(games[0].favorite);
        assert_eq!(games[0].target().path, "steam://run/2");
        assert_eq!(db.merge(vec![]).unwrap().len(), 1);
    }
    #[test]
    fn exact_duplicates_merge_but_names_do_not() {
        let d = tempfile::tempdir().unwrap();
        let mut db = Database::open(d.path()).unwrap();
        let a = Game::new(
            Source::Manual,
            "a".into(),
            "Same".into(),
            LaunchTarget::uri("steam://rungameid/1"),
        );
        let b = Game::new(
            Source::Steam,
            "1".into(),
            "Same".into(),
            LaunchTarget::uri("steam://run/1"),
        );
        let c = Game::new(
            Source::Gog,
            "2".into(),
            "Same".into(),
            LaunchTarget::uri("goggalaxy://2"),
        );
        assert_eq!(db.merge(vec![a, b, c]).unwrap().len(), 2);
    }
    #[test]
    fn imported_shortcut_keeps_store_identity_when_launch_target_changes() {
        let d = tempfile::tempdir().unwrap();
        let mut db = Database::open(d.path()).unwrap();
        let launch = LaunchTarget::exe("D:/Game/game.exe", "", "D:/Game");
        let manual = Game::new(
            Source::Manual,
            "shortcut".into(),
            "Game".into(),
            launch.clone(),
        );
        let store = Game::new(Source::Gog, "123".into(), "Game".into(), launch);
        db.merge(vec![manual, store]).unwrap();
        // Reopen the database so in-memory aliases cannot mask a persistence error.
        drop(db);
        let mut db = Database::open(d.path()).unwrap();
        let via_galaxy = Game::new(
            Source::Gog,
            "123".into(),
            "Game".into(),
            LaunchTarget::exe("C:/Galaxy/GalaxyClient.exe", "/gameId=123", "C:/Galaxy"),
        );
        let games = db.merge(vec![via_galaxy]).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].source, Source::Gog);
        assert!(games[0].launch.path.ends_with("GalaxyClient.exe"));
    }
}
