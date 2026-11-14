//! A tiny, harmless launch fixture. Not shipped with the application.
#![windows_subsystem = "windows"]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // An offscreen, non-activating test window exercises real WM_CLOSE delivery.
    // It is never included in the release payload.
    let test_window = if args.iter().any(|s| s == "--close-window") {
        use windows::{Win32::UI::WindowsAndMessaging::*, core::w};
        Some(unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!("HITDROL close test"),
                WS_POPUP | WS_VISIBLE,
                -10000,
                -10000,
                80,
                60,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        })
    } else {
        None
    };
    if let Some(path) = args
        .iter()
        .position(|s| s == "--output")
        .and_then(|i| args.get(i + 1))
    {
        let data = serde_json::json!({"args":args,"cwd":std::env::current_dir().ok()});
        let _ = std::fs::write(path, serde_json::to_vec_pretty(&data).unwrap());
    }
    if let Some(ms) = args
        .iter()
        .position(|s| s == "--hold-ms")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u64>().ok())
    {
        std::thread::sleep(std::time::Duration::from_millis(ms.min(10000)));
    }
    if let Some(window) = test_window {
        use windows::Win32::UI::WindowsAndMessaging::*;
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        unsafe {
            while IsWindow(Some(window)).as_bool() && std::time::Instant::now() < until {
                let mut message = MSG::default();
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if IsWindow(Some(window)).as_bool() {
                let _ = DestroyWindow(window);
            }
        }
    }
}
