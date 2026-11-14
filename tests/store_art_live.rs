//! Optional integration smoke test. No personal keys, accounts or game launches.
use hitdrol_launcher::{
    discovery::{self, GameSource},
    model::{Game, LaunchTarget, Source},
    store_art::OfficialArt,
};

#[test]
#[ignore = "Reads the currently signed-in local Steam account library"]
fn local_steam_licenses_import_owned_uninstalled_games() {
    let root = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey("Software\\Valve\\Steam")
        .unwrap()
        .get_value::<String, _>("SteamPath")
        .unwrap();
    let games = hitdrol_launcher::steam_library::owned(std::path::Path::new(&root)).unwrap();
    assert!(!games.is_empty());
    assert!(games.iter().all(|g| g.owned && !g.installed));
    println!(
        "Owned Steam games from license/package/app caches: {}",
        games.len()
    );
    println!(
        "Verified play history: {}",
        games.iter().filter(|g| g.history_verified).count()
    );
}

#[test]
#[ignore = "Requires Internet; Epic additionally requires a populated local client catalog"]
fn official_images_download_and_cached_images_need_no_second_download() {
    let cache = tempfile::tempdir().unwrap();
    let service = OfficialArt::new().unwrap();
    let mut games = vec![
        Game::new(
            Source::Steam,
            "3405690".into(),
            "Store artwork test".into(),
            LaunchTarget::uri("steam://run/3405690"),
        ),
        Game::new(
            Source::Gog,
            "1963513391".into(),
            "Store artwork test".into(),
            LaunchTarget::uri("goggalaxy://1963513391"),
        ),
    ];
    if let Some(data) = std::env::var_os("PROGRAMDATA") {
        let source = discovery::EpicSource {
            manifests: std::path::PathBuf::from(data).join("Epic/EpicGamesLauncher/Data/Manifests"),
        };
        if let Some(game) = source
            .scan()
            .games
            .into_iter()
            .find(|g| g.store_cover_url.is_some())
        {
            games.push(game);
        }
    }
    for mut game in games {
        let paths = service
            .fetch(&game, cache.path())
            .unwrap_or_else(|e| panic!("{:?}: {e:#}", game.source));
        let cover = paths.cover.expect("official cover");
        let size = image::image_dimensions(&cover).unwrap();
        assert!(size.0 > 0 && size.1 > 0);
        println!(
            "{:?}: cover {}x{}, hero {}",
            game.source,
            size.0,
            size.1,
            paths.hero.is_some()
        );
        game.cover = Some(cover);
        game.hero = paths.hero;
        if game.hero.is_some() {
            let unchanged = service.fetch(&game, cache.path()).unwrap();
            assert!(unchanged.cover.is_none() && unchanged.hero.is_none());
        }
    }
}
