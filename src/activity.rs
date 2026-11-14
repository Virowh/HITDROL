//! Process creation times confirm a game started; a store URI by itself does not.
use crate::{
    discovery::path_key,
    model::{Game, LaunchKind, valid_played},
    platform,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};
use windows::Win32::{
    Foundation::{CloseHandle, FILETIME},
    System::{Diagnostics::ToolHelp::*, Threading::*},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub path: PathBuf,
    pub started: u64,
    /// Exact FILETIME, independent of the seconds used by recent-game history.
    pub created: u64,
}
fn creation_stamp(handle: windows::Win32::Foundation::HANDLE) -> u64 {
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) }
        .is_err()
    {
        return 0;
    }
    (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)
}
pub(crate) fn creation_time(handle: windows::Win32::Foundation::HANDLE) -> u64 {
    creation_stamp(handle).saturating_sub(116444736000000000) / 10000000
}
pub fn processes() -> Vec<Process> {
    let mut result = vec![];
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return result;
        };
        let mut item = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Process32FirstW(snapshot, &mut item).is_ok();
        while more {
            if let Ok(handle) =
                OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, item.th32ProcessID)
            {
                let mut text = vec![0u16; 32768];
                let mut len = text.len() as u32;
                if QueryFullProcessImageNameW(
                    handle,
                    PROCESS_NAME_WIN32,
                    windows::core::PWSTR(text.as_mut_ptr()),
                    &mut len,
                )
                .is_ok()
                {
                    let path = PathBuf::from(String::from_utf16_lossy(&text[..len as usize]));
                    if game_process(&path) {
                        let started = creation_time(handle);
                        if valid_played(started) {
                            result.push(Process {
                                pid: item.th32ProcessID,
                                path,
                                started,
                                created: creation_stamp(handle),
                            });
                        }
                    }
                }
                let _ = CloseHandle(handle);
            }
            more = Process32NextW(snapshot, &mut item).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    result
}
fn game_process(path: &Path) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    ![
        "steam.exe",
        "steamwebhelper.exe",
        "epicgameslauncher.exe",
        "galaxyclient.exe",
        "galaxyclientservice.exe",
        "dxsetup.exe",
        "setup.exe",
        "start_protected_game.exe",
    ]
    .contains(&name.as_str())
        && ![
            "crashreport",
            "crashhandler",
            "unitycrash",
            "unins",
            "vc_redist",
            "vcredist",
            "easyanticheat",
            "battleye",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}
pub fn matches(game: &Game, path: &Path) -> bool {
    if !game.installed || !game_process(path) {
        return false;
    }
    if game
        .process_path
        .as_ref()
        .is_some_and(|p| path_key(p) == path_key(path))
    {
        return true;
    }
    if game.target().kind == LaunchKind::Executable
        && path_key(Path::new(&game.target().path)) == path_key(path)
    {
        return true;
    }
    if game.source == crate::model::Source::Manual {
        return false;
    }
    game.install_dir.as_ref().is_some_and(|root| {
        let key = path_key(root);
        key.len() > 3 && path_key(path).starts_with(&(key + "\\"))
    })
}
pub fn receipt_is_game(game: &Game, receipt: &platform::LaunchReceipt) -> bool {
    receipt.started > 0
        && (receipt.path.as_ref().is_some_and(|p| matches(game, p))
            || (game.target().kind == LaunchKind::Executable
                && game_process(Path::new(&game.target().path))
                && receipt.pid > 0))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningGame {
    pub id: String,
    pub processes: Vec<Process>,
}

fn same_process(expected: &Process, actual: &Process) -> bool {
    expected.pid != 0
        && expected.created != 0
        && expected.pid == actual.pid
        && expected.created == actual.created
        && path_key(&expected.path) == path_key(&actual.path)
}

/// Ask only windows of confirmed game processes to close. Never terminate a
/// process tree: URI launch receipts can belong to Steam/Epic rather than a game.
pub fn close_game(game: &Game, expected: &[Process]) -> anyhow::Result<usize> {
    use anyhow::{Context, ensure};
    use windows::{
        Win32::{
            Foundation::*, System::RemoteDesktop::ProcessIdToSessionId, UI::WindowsAndMessaging::*,
        },
        core::BOOL,
    };
    struct Windows(Vec<HWND>, u32);
    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
        let data = unsafe { &mut *(data.0 as *mut Windows) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == data.1 && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            data.0.push(hwnd);
        }
        TRUE
    }
    let current = processes();
    let mut sent = 0;
    for expected in expected {
        if !matches(game, &expected.path)
            || !current.iter().any(|actual| same_process(expected, actual))
        {
            continue;
        }
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, expected.pid)
                .context("Нет доступа к процессу игры")?;
            // Holding the process handle prevents PID reuse during enumeration.
            let result = (|| -> anyhow::Result<usize> {
                if creation_stamp(handle) != expected.created {
                    return Ok(0);
                }
                let mut game_session = 0;
                let mut own_session = 0;
                ProcessIdToSessionId(expected.pid, &mut game_session)?;
                ProcessIdToSessionId(GetCurrentProcessId(), &mut own_session)?;
                ensure!(
                    game_session == own_session,
                    "Игра находится в другой сессии Windows"
                );
                let mut windows = Windows(vec![], expected.pid);
                EnumWindows(
                    Some(collect),
                    LPARAM((&mut windows as *mut Windows) as isize),
                )?;
                let mut count = 0;
                for hwnd in windows.0 {
                    let mut pid = 0;
                    GetWindowThreadProcessId(hwnd, Some(&mut pid));
                    if pid == expected.pid {
                        PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0))
                            .context("Windows не разрешила закрыть окно игры")?;
                        count += 1;
                    }
                }
                Ok(count)
            })();
            let _ = CloseHandle(handle);
            sent += result?;
        }
    }
    ensure!(
        sent > 0,
        "Окно игры не найдено. Возможно, игра уже закрыта или запущена с правами администратора."
    );
    Ok(sent)
}

pub struct Monitor {
    games: Arc<Mutex<Vec<Game>>>,
    stop: Arc<AtomicBool>,
    wake: mpsc::SyncSender<()>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Monitor {
    pub fn new(on_started: impl FnMut(String, u64) + Send + 'static) -> Self {
        Self::with_running(on_started, |_| {})
    }
    pub fn with_running(
        mut on_started: impl FnMut(String, u64) + Send + 'static,
        mut on_running: impl FnMut(Vec<RunningGame>) + Send + 'static,
    ) -> Self {
        let games = Arc::new(Mutex::new(Vec::<Game>::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (wake, rx) = mpsc::sync_channel(1);
        let (watched, shutdown) = (games.clone(), stop.clone());
        let worker = thread::spawn(move || {
            let mut seen = HashMap::<String, u64>::new();
            let mut previous = Vec::<RunningGame>::new();
            while !shutdown.load(Ordering::Acquire) {
                let games = watched.lock().unwrap().clone();
                let mut active = vec![];
                if !games.is_empty() {
                    let running = processes();
                    for game in &games {
                        let matched: Vec<_> = running
                            .iter()
                            .filter(|p| matches(game, &p.path))
                            .cloned()
                            .collect();
                        if !matched.is_empty() {
                            active.push(RunningGame {
                                id: game.id.clone(),
                                processes: matched,
                            });
                        }
                        if let Some(at) = running
                            .iter()
                            .filter(|p| matches(game, &p.path))
                            .map(|p| p.started)
                            .max()
                            && at > seen.get(&game.id).copied().unwrap_or(0)
                        {
                            seen.insert(game.id.clone(), at);
                            on_started(game.id.clone(), at);
                        }
                    }
                }
                if active != previous {
                    previous = active.clone();
                    on_running(active);
                }
                let _ = rx.recv_timeout(Duration::from_secs(3));
            }
        });
        Self {
            games,
            stop,
            wake,
            worker: Some(worker),
        }
    }
    pub fn update(&self, games: &[Game]) {
        *self.games.lock().unwrap() = games.to_vec();
        let _ = self.wake.try_send(());
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    #[test]
    fn process_matching_uses_boundaries_and_excludes_store_clients() {
        let mut game = Game::new(
            Source::Steam,
            "42".into(),
            "Game".into(),
            LaunchTarget::uri("steam://run/42"),
        );
        game.install_dir = Some("D:/Games/Game".into());
        assert!(matches(&game, Path::new("d:/games/game/bin/game.exe")));
        assert!(!matches(&game, Path::new("D:/Games/Game Other/game.exe")));
        assert!(!matches(&game, Path::new("D:/Games/Game/steam.exe")));
        assert!(!matches(
            &game,
            Path::new("D:/Games/Game/CrashReportClient.exe")
        ));
        game.installed = false;
        assert!(!matches(&game, Path::new("D:/Games/Game/game.exe")));
    }
    #[test]
    fn manual_game_does_not_match_all_exes_in_its_folder() {
        let mut game = Game::new(
            Source::Manual,
            "a".into(),
            "Game".into(),
            LaunchTarget::exe("D:/Game.exe", "", "D:/"),
        );
        game.install_dir = Some("D:/".into());
        assert!(matches(&game, Path::new("D:/Game.exe")));
        assert!(!matches(&game, Path::new("D:/Other.exe")));
    }
    #[test]
    fn reused_pid_or_another_executable_is_not_the_same_game_process() {
        let original = Process {
            pid: 42,
            created: 123456789,
            started: 1,
            path: "D:/Games/Game.exe".into(),
        };
        let mut other = original.clone();
        assert!(same_process(&original, &other));
        other.created += 1;
        assert!(!same_process(&original, &other));
        other = original.clone();
        other.path = "D:/Steam/steam.exe".into();
        assert!(!same_process(&original, &other));
        other = original.clone();
        other.pid += 1;
        assert!(!same_process(&original, &other));
        other = original.clone();
        other.created = 0;
        assert!(!same_process(&other, &other));
    }
}
