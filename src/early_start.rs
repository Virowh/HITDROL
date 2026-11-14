//! Compatibility cleanup only: remove the retired 0.4.5 logon task.
//! This module cannot create, enable or run scheduled tasks.
use anyhow::{Result, ensure};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree, RPC_E_CHANGED_MODE},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::{
            Com::*,
            TaskScheduler::*,
            Threading::{GetCurrentProcess, OpenProcessToken},
            Variant::VARIANT,
        },
    },
    core::{BSTR, PWSTR},
};

pub fn user_sid() -> Result<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let result = (|| -> Result<String> {
            let mut size = 0;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);
            ensure!(size > 0 && size < 65536, "Invalid token user size");
            let mut data = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
            GetTokenInformation(
                token,
                TokenUser,
                Some(data.as_mut_ptr().cast()),
                size,
                &mut size,
            )?;
            let user = &*data.as_ptr().cast::<TOKEN_USER>();
            let mut sid = PWSTR::null();
            ConvertSidToStringSidW(user.User.Sid, &mut sid)?;
            let result = sid.to_string();
            LocalFree(Some(HLOCAL(sid.0.cast())));
            Ok(result?)
        })();
        let _ = CloseHandle(token);
        result
    }
}
fn task_name(sid: &str) -> Result<BSTR> {
    ensure!(
        sid.starts_with("S-1-")
            && sid
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'S' || b == b'-'),
        "Invalid user SID"
    );
    Ok(BSTR::from(format!("HITDROL-Startup-{sid}")))
}
struct Apartment(bool);
impl Apartment {
    fn enter() -> Result<Self> {
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self(false));
        }
        result.ok()?;
        Ok(Self(true))
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() }
        }
    }
}
fn folder() -> Result<ITaskFolder> {
    unsafe {
        let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
        let empty = VARIANT::default();
        service.Connect(&empty, &empty, &empty, &empty)?;
        Ok(service.GetFolder(&BSTR::from("\\"))?)
    }
}
pub fn remove(sid: &str) -> Result<()> {
    let name = task_name(sid)?;
    let _apartment = Apartment::enter()?;
    match unsafe { folder()?.DeleteTask(&name, 0) } {
        Ok(()) => Ok(()),
        Err(e) if e.code() == windows::core::HRESULT::from_win32(2) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
