//! Remove only HITDROL startup values from the trusted MSI owner's loaded hive.
//! The entry may have been added in the application after installation.
#[path = "../../src/early_start.rs"]
mod early_start;
#[link(name = "msi")]
unsafe extern "system" {
    fn MsiGetPropertyW(install: u32, name: *const u16, value: *mut u16, length: *mut u32) -> u32;
    fn MsiCreateRecord(fields: u32) -> u32;
    fn MsiRecordSetStringW(record: u32, field: u32, value: *const u16) -> u32;
    fn MsiProcessMessage(install: u32, kind: u32, record: u32) -> i32;
    fn MsiCloseHandle(handle: u32) -> u32;

}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegOpenKeyExW(
        key: isize,
        name: *const u16,
        options: u32,
        access: u32,
        result: *mut isize,
    ) -> i32;
    fn RegDeleteValueW(key: isize, name: *const u16) -> i32;
    fn RegCloseKey(key: isize) -> i32;
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn note(install: u32, text: &str) {
    let text = wide(text);
    unsafe {
        let record = MsiCreateRecord(0);
        if record != 0 {
            MsiRecordSetStringW(record, 0, text.as_ptr());
            MsiProcessMessage(install, 0x04000000, record);
            MsiCloseHandle(record);
        }
    }
}
fn property(install: u32, name: &str) -> Option<String> {
    let mut buffer = [0u16; 32768];
    let mut length = buffer.len() as u32;
    if unsafe {
        MsiGetPropertyW(
            install,
            wide(name).as_ptr(),
            buffer.as_mut_ptr(),
            &mut length,
        )
    } != 0
        || length as usize >= buffer.len()
    {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}
/// Remove the retired task after an installation/update, irrespective of flags.
#[unsafe(no_mangle)]
pub extern "system" fn RemoveLegacyStartupTask(install: u32) -> u32 {
    let result = (|| -> anyhow::Result<()> {
        let sid = property(install, "UserSID").ok_or_else(|| anyhow::anyhow!("Missing UserSID"))?;
        anyhow::ensure!(sid == early_start::user_sid()?, "Installer user mismatch");
        early_start::remove(&sid)
    })();
    match result {
        Ok(()) => {
            note(install, "HITDROL retired startup task removed");
            0
        }
        // The retired --early-autostart entry point is inert in this release.
        // A disabled Scheduler service must not break installation of the fix.
        Err(e) => {
            note(
                install,
                &format!("HITDROL retired task cleanup warning: {e:#}"),
            );
            0
        }
    }
}
/// Windows Installer calls this export with a valid installation handle.
#[unsafe(no_mangle)]
pub extern "system" fn CleanupStartup(install: u32) -> u32 {
    let mut buffer = [0u16; 160];
    let mut length = buffer.len() as u32;
    let name = wide("UserSID");
    if unsafe { MsiGetPropertyW(install, name.as_ptr(), buffer.as_mut_ptr(), &mut length) } != 0
        || length as usize >= buffer.len()
    {
        return 1603;
    }
    let sid = String::from_utf16_lossy(&buffer[..length as usize]);
    if !sid.starts_with("S-1-")
        || !sid
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'S' || b == b'-')
    {
        return 1603;
    }
    let value = wide("HITDROL");
    if let Err(e) = early_start::remove(&sid) {
        note(install, &format!("HITDROL early startup cleanup: {e:#}"));
        // The visible Run value is removed below even when policy prevents
        // task deletion. Its absence prevents the task from opening HITDROL.
    }
    // Win32 predefined registry handles are sign-extended on x64.
    let users = (0x80000003u32 as i32) as isize;
    for view in [0x0100, 0x0200] {
        for path in [
            "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
            "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run",
        ] {
            let path = wide(&format!("{sid}\\{path}"));
            let mut key = 0;
            let status = unsafe { RegOpenKeyExW(users, path.as_ptr(), 0, 0x0002 | view, &mut key) };
            if status == 2 || status == 3 {
                continue;
            }
            if status != 0 {
                note(install, "HITDROL startup cleanup: cannot open user key");
                return 1603;
            }
            let status = unsafe { RegDeleteValueW(key, value.as_ptr()) };
            unsafe { RegCloseKey(key) };
            note(
                install,
                &format!("HITDROL startup cleanup result: {status}"),
            );
            if status != 0 && status != 2 {
                return 1603;
            }
        }
    }
    0
}
