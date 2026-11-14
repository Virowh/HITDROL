//! Ordinary per-user Run entry, controlled by Windows Task Manager.
//! No scheduled task or early GPU startup is registered.
use anyhow::{Context, Result, ensure};
use std::{io, path::Path};
use winreg::{RegKey, enums::*};
const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const APPROVED: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
pub const NAME: &str = "HITDROL";

fn missing_ok(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}
pub fn registered() -> Result<bool> {
    let user = RegKey::predef(HKEY_CURRENT_USER);
    match user.open_subkey_with_flags(RUN, KEY_READ | KEY_WOW64_64KEY) {
        Ok(key) => match key.get_value::<String, _>(NAME) {
            Ok(value) => Ok(!value.is_empty()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
pub fn command(exe: &Path) -> Result<String> {
    let path = exe
        .to_str()
        .context("Путь приложения не поддерживается Windows")?;
    ensure!(
        exe.is_absolute() && !path.contains(['"', '\n', '\r']),
        "Некорректный путь приложения"
    );
    let command = format!("\"{path}\" --autostart");
    ensure!(
        command.encode_utf16().count() <= 260,
        "Для автозапуска установите HITDROL в папку с более коротким путём"
    );
    Ok(command)
}
pub fn set(enabled: bool) -> Result<()> {
    let user = RegKey::predef(HKEY_CURRENT_USER);
    if enabled {
        crate::early_start::remove(&crate::early_start::user_sid()?)
            .context("Не удалось удалить прежнее задание раннего запуска")?;
        let exe = std::env::current_exe()?;
        ensure!(exe.is_file(), "Файл HITDROL недоступен");
        let (key, _) =
            user.create_subkey_with_flags(RUN, KEY_READ | KEY_WRITE | KEY_WOW64_64KEY)?;
        let value = command(&exe)?;
        key.set_value(NAME, &value)?;
        ensure!(
            key.get_value::<String, _>(NAME)? == value,
            "Windows не сохранила автозапуск HITDROL"
        );
        // Windows owns the enable/disable decision in Task Manager. Re-registering
        // the application must not silently override that decision.
    } else {
        // Delete the visible switch first, so an undeletable task still cannot
        // launch the UI. Only this user's HITDROL task is ever touched.
        for path in [RUN, APPROVED] {
            match user.open_subkey_with_flags(path, KEY_SET_VALUE | KEY_WOW64_64KEY) {
                Ok(key) => missing_ok(key.delete_value(NAME))?,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        crate::early_start::remove(&crate::early_start::user_sid()?)?;
    }
    Ok(())
}
pub fn open_windows_settings() -> Result<()> {
    crate::platform::open_system_uri("ms-settings:startupapps")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoted_unicode_path_and_arguments_are_preserved() {
        let result = command(Path::new("C:/Папка с пробелами/HITDROL.exe")).unwrap();
        assert_eq!(result, "\"C:/Папка с пробелами/HITDROL.exe\" --autostart");
        assert!(command(Path::new("HITDROL.exe")).is_err());
        assert!(command(Path::new("C:/bad\"/HITDROL.exe")).is_err());
        assert!(command(Path::new(&format!("C:/{}HITDROL.exe", "x".repeat(260)))).is_err());
    }
}
