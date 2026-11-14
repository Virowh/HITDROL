use hitdrol_launcher::{discovery, model::LaunchTarget, platform};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use windows::{
    Win32::{System::Com::*, UI::Shell::*},
    core::{Interface, PCWSTR},
};
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn shortcut(path: &Path, exe: &Path, args: &str, cwd: &Path) {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap();
        {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            link.SetPath(PCWSTR(wide(&exe.to_string_lossy()).as_ptr()))
                .unwrap();
            link.SetArguments(PCWSTR(wide(args).as_ptr())).unwrap();
            link.SetWorkingDirectory(PCWSTR(wide(&cwd.to_string_lossy()).as_ptr()))
                .unwrap();
            let p: IPersistFile = link.cast().unwrap();
            p.Save(PCWSTR(wide(&path.to_string_lossy()).as_ptr()), true)
                .unwrap();
        }
        CoUninitialize();
    }
}
fn wait_result(path: &Path) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if let Ok(bytes) = std::fs::read(path)
            && let Ok(value) = serde_json::from_slice(&bytes)
        {
            return value;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("launch probe did not produce {}", path.display());
}
#[test]
fn executable_launch_preserves_unicode_arguments_and_working_directory() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("Игры с пробелами");
    std::fs::create_dir(&work).unwrap();
    let out = work.join("result.json");
    let args = format!("--output \"{}\" --word \"Русский аргумент\"", out.display());
    let target = LaunchTarget::exe(
        env!("CARGO_BIN_EXE_hitdrol-launch-probe"),
        args,
        work.to_string_lossy(),
    );
    platform::launch(&target).unwrap();
    let result = wait_result(&out);
    assert_eq!(result["args"][3], "Русский аргумент");
    assert_eq!(Path::new(result["cwd"].as_str().unwrap()), work);
}
#[test]
fn shell_shortcut_launch_preserves_original_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("shortcut-result.json");
    let link = dir.path().join("Тест игры.lnk");
    let args = format!("--output \"{}\" --word \"из ярлыка\"", out.display());
    shortcut(
        &link,
        Path::new(env!("CARGO_BIN_EXE_hitdrol-launch-probe")),
        &args,
        dir.path(),
    );
    let info = platform::read_shortcut(&link).unwrap();
    assert_eq!(info.target.args, args);
    assert_eq!(Path::new(&info.target.cwd), dir.path());
    platform::launch(&info.target).unwrap();
    assert_eq!(wait_result(&out)["args"][3], "из ярлыка");
}
#[test]
fn broken_shortcut_is_an_error_and_valid_shortcut_imports_icon() {
    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("bad.lnk");
    std::fs::write(&broken, b"invalid shortcut").unwrap();
    assert!(platform::read_shortcut(&broken).is_err());
    let link = dir.path().join("good.lnk");
    shortcut(
        &link,
        Path::new(env!("CARGO_BIN_EXE_hitdrol-launch-probe")),
        "",
        dir.path(),
    );
    let game = discovery::from_file(&link, &dir.path().join("icons")).unwrap();
    assert_eq!(game.name(), "good");
    assert!(game.icon.unwrap().is_file());
}
#[test]
fn missing_file_returns_error() {
    let target = LaunchTarget::exe("Z:/a-file-that-does-not-exist.exe", "", "");
    assert!(platform::launch(&target).is_err());
}

#[test]
fn close_game_targets_a_verified_window_and_reports_actual_exit() {
    use hitdrol_launcher::{activity, model::*};
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("Игра для проверки закрытия.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_hitdrol-launch-probe"), &exe).unwrap();
    let ready = dir.path().join("ready.json");
    let game = Game::new(
        Source::Manual,
        "close-test".into(),
        "Close test".into(),
        LaunchTarget::exe(exe.to_string_lossy(), "", ""),
    );
    let mut child = std::process::Command::new(&exe)
        .args(["--close-window", "--output"])
        .arg(&ready)
        .spawn()
        .unwrap();
    wait_result(&ready);
    let process = activity::processes()
        .into_iter()
        .find(|p| p.pid == child.id())
        .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let monitor = activity::Monitor::with_running(
        |_, _| {},
        move |running| {
            let _ = tx.send(running);
        },
    );
    monitor.update(std::slice::from_ref(&game));
    let running = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(running[0].id, game.id);
    let mut reused_pid = process.clone();
    reused_pid.created += 1;
    assert!(activity::close_game(&game, &[reused_pid]).is_err());
    assert!(child.try_wait().unwrap().is_none());
    let wrong_game = Game::new(
        Source::Manual,
        "other".into(),
        "Other".into(),
        LaunchTarget::exe("C:/other-game.exe", "", ""),
    );
    assert!(activity::close_game(&wrong_game, std::slice::from_ref(&process)).is_err());
    assert!(child.try_wait().unwrap().is_none());
    assert!(activity::close_game(&game, &[process]).unwrap() > 0);
    let deadline = Instant::now() + Duration::from_secs(4);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "WM_CLOSE was not processed");
        std::thread::sleep(Duration::from_millis(20));
    }
    monitor.update(std::slice::from_ref(&game));
    assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap().is_empty());
}

#[test]
fn retired_early_autostart_exits_without_even_creating_a_profile() {
    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("must-not-be-created");
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_hitdrol-launcher"))
        .args(["--early-autostart", "--intro", "--data-dir"])
        .arg(&profile)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Retired entry point did not exit"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!profile.exists());
}

#[test]
fn real_process_is_detected_and_history_survives_restart() {
    use hitdrol_launcher::{activity, model::*, storage::Database};
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("Тест настоящего запуска.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_hitdrol-launch-probe"), &exe).unwrap();
    let mut game = Game::new(
        Source::Manual,
        "actual".into(),
        "Проверка".into(),
        LaunchTarget::exe(
            exe.to_string_lossy(),
            "--hold-ms 4500",
            dir.path().to_string_lossy(),
        ),
    );
    let (tx, rx) = std::sync::mpsc::channel();
    let monitor = activity::Monitor::new(move |id, at| {
        let _ = tx.send((id, at));
    });
    monitor.update(&[game.clone()]);
    let receipt = platform::launch_receipt(game.target()).unwrap();
    assert!(activity::receipt_is_game(&game, &receipt));
    let (id, at) = rx
        .recv_timeout(Duration::from_secs(8))
        .expect("real process observed");
    assert_eq!(id, game.id);
    assert!(valid_played(at));
    game.record_played(at);
    let profile = dir.path().join("profile");
    let db = Database::open(&profile).unwrap();
    db.save(&game).unwrap();
    drop(db);
    let restored = Database::open(&profile).unwrap().games().unwrap();
    assert_eq!(recent_games(&restored, false)[0].last_played, at);
    let client = platform::LaunchReceipt {
        pid: 123,
        started: at,
        path: Some("C:/Steam/steam.exe".into()),
    };
    let store = Game::new(
        Source::Steam,
        "42".into(),
        "Game".into(),
        LaunchTarget::uri("steam://run/42"),
    );
    assert!(!activity::receipt_is_game(&store, &client));
    std::thread::sleep(Duration::from_millis(1600));
}
