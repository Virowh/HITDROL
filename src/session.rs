//! Wait for an unlocked, connected session and the application's input desktop.
//! Windows can run logon applications while its sign-in screen is still visible.
use std::time::{Duration, Instant};
use windows::{
    Win32::{
        Foundation::HANDLE,
        System::{RemoteDesktop::*, StationsAndDesktops::*, Threading::GetCurrentThreadId},
    },
    core::PWSTR,
};

pub const POLL_INTERVAL: Duration = Duration::from_millis(250);
pub const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(50);

fn session_unlocked() -> bool {
    unsafe {
        let mut buffer = PWSTR::null();
        let mut length = 0;
        if WTSQuerySessionInformationW(
            None,
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut length,
        )
        .is_err()
        {
            return false;
        }
        let ready = if !buffer.is_null() && length as usize >= std::mem::size_of::<WTSINFOEXW>() {
            let info = &*buffer.0.cast::<WTSINFOEXW>();
            info.Level == 1 && {
                let state = info.Data.WTSInfoExLevel1;
                state.SessionState == WTSActive && state.SessionFlags == 1
            }
        } else {
            false
        };
        WTSFreeMemory(buffer.0.cast());
        ready
    }
}
fn desktop_name(desktop: HDESK) -> Option<Vec<u16>> {
    let mut name = [0u16; 256];
    unsafe {
        GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            std::mem::size_of_val(&name) as u32,
            None,
        )
        .ok()?;
    }
    let end = name.iter().position(|c| *c == 0)?;
    Some(name[..end].to_vec())
}
pub fn interactive() -> bool {
    if !session_unlocked() {
        return false;
    }
    unsafe {
        let Ok(input) = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS)
        else {
            return false;
        };
        let input_name = desktop_name(input);
        let _ = CloseDesktop(input);
        let own_name = GetThreadDesktop(GetCurrentThreadId())
            .ok()
            .and_then(desktop_name);
        input_name.is_some() && input_name == own_name
    }
}

#[derive(Default)]
pub struct StartupGate {
    pub interactive: bool,
    pub first_ready: Option<Instant>,
    ready_since: Option<Instant>,
    stability: Duration,
}
impl StartupGate {
    pub fn for_startup(autostart: bool) -> Self {
        Self {
            stability: if autostart {
                Duration::from_secs(2)
            } else {
                Duration::ZERO
            },
            ..Default::default()
        }
    }
    /// No window or GPU may be initialized until this startup gate releases.
    pub fn update(&mut self, now: Instant, interactive: bool) -> bool {
        let ready = if interactive {
            now.saturating_duration_since(*self.ready_since.get_or_insert(now)) >= self.stability
        } else {
            self.ready_since = None;
            false
        };
        let changed = self.interactive != ready;
        self.interactive = ready;
        if ready {
            self.first_ready.get_or_insert(now);
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_start_requires_two_uninterrupted_seconds_after_unlock() {
        let now = Instant::now();
        let mut gate = StartupGate::for_startup(true);
        gate.update(now, true);
        gate.update(now + Duration::from_millis(1999), true);
        assert!(!gate.interactive);
        gate.update(now + Duration::from_secs(2), true);
        assert!(gate.interactive);
        gate.update(now + Duration::from_secs(3), false);
        assert!(!gate.interactive);
        gate.update(now + Duration::from_secs(4), true);
        assert!(!gate.interactive);
        gate.update(now + Duration::from_secs(6), true);
        assert!(gate.interactive);
    }
    #[test]
    fn locked_session_never_releases_startup_even_after_a_long_wait() {
        let now = Instant::now();
        let mut gate = StartupGate::default();
        gate.update(now, false);
        gate.update(now + Duration::from_secs(3600), false);
        assert!(!gate.interactive);
        assert!(gate.first_ready.is_none());
        assert!(gate.update(now + Duration::from_secs(3601), true));
        assert!(gate.interactive);
        assert_eq!(gate.first_ready, Some(now + Duration::from_secs(3601)));
    }
    #[test]
    fn relocking_hides_again_and_unlock_releases_without_a_settle_delay() {
        let now = Instant::now();
        let mut gate = StartupGate::default();
        assert!(gate.update(now, true));
        assert!(gate.update(now + Duration::from_millis(900), false));
        assert!(!gate.interactive);
        assert!(gate.update(now + Duration::from_secs(2), true));
        assert!(gate.interactive);
        assert_eq!(gate.first_ready, Some(now));
    }
    #[test]
    fn repeated_checks_do_not_move_the_first_ready_time() {
        let now = Instant::now();
        let mut gate = StartupGate::default();
        assert!(gate.update(now, true));
        assert!(!gate.update(now + Duration::from_secs(2), true));
        assert_eq!(gate.first_ready, Some(now));
    }
}
