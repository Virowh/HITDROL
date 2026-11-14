//! Windows APIs are isolated here. Shell operations run on an STA worker.
use crate::model::{LaunchKind, LaunchTarget};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use windows::{
    Win32::{
        Foundation::CloseHandle,
        Security::Credentials::*,
        System::Com::*,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{Interface, PCWSTR, PWSTR},
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn from_wide(s: &[u16]) -> String {
    String::from_utf16_lossy(&s[..s.iter().position(|c| *c == 0).unwrap_or(s.len())])
}
struct Com;
impl Com {
    fn init() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        }
        Ok(Self)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

pub fn known_folder(id: &windows::core::GUID) -> Result<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)?;
        let value = p.to_string();
        CoTaskMemFree(Some(p.0.cast()));
        Ok(PathBuf::from(value?))
    }
}
pub fn data_dir() -> Result<PathBuf> {
    crate::profile::prepare(&known_folder(&FOLDERID_LocalAppData)?)
}
/// Resolve the profile without importing/opening an old database before logon.
pub fn profile_path() -> Result<PathBuf> {
    Ok(known_folder(&FOLDERID_LocalAppData)?.join(crate::branding::PROFILE))
}
pub fn desktop_dirs() -> Vec<PathBuf> {
    [FOLDERID_Desktop, FOLDERID_PublicDesktop]
        .iter()
        .filter_map(|id| known_folder(id).ok())
        .collect()
}
pub fn drives() -> Vec<PathBuf> {
    use windows::Win32::Storage::FileSystem::GetLogicalDrives;
    let mask = unsafe { GetLogicalDrives() };
    (0..26)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| PathBuf::from(format!("{}:\\", (b'A' + i) as char)))
        .collect()
}

pub struct Shortcut {
    pub target: LaunchTarget,
    pub executable: Option<PathBuf>,
}
pub fn read_shortcut(path: &Path) -> Result<Shortcut> {
    let _com = Com::init()?;
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let persist: IPersistFile = link.cast()?;
        let path_w = wide(&path.to_string_lossy());
        persist.Load(PCWSTR(path_w.as_ptr()), STGM_READ)?;
        let mut file = vec![0u16; 32768];
        let mut args = vec![0u16; 32768];
        let mut cwd = vec![0u16; 32768];
        let _ = link.GetPath(&mut file, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32);
        let _ = link.GetArguments(&mut args);
        let _ = link.GetWorkingDirectory(&mut cwd);
        let exe = from_wide(&file);
        let args = from_wide(&args);
        let cwd = from_wide(&cwd);
        let resolved = if exe.is_empty() {
            None
        } else {
            Some(LaunchTarget::exe(exe.clone(), args.clone(), cwd.clone()).key())
        };
        Ok(Shortcut {
            target: LaunchTarget {
                kind: LaunchKind::ShellLink,
                path: path.to_string_lossy().into(),
                args,
                cwd,
                resolved_key: resolved,
            },
            executable: (!exe.is_empty()).then(|| PathBuf::from(exe)),
        })
    }
}
pub fn launch(target: &LaunchTarget) -> Result<()> {
    launch_receipt(target).map(|_| ())
}
pub struct LaunchReceipt {
    pub pid: u32,
    pub started: u64,
    pub path: Option<PathBuf>,
}
pub fn launch_receipt(target: &LaunchTarget) -> Result<LaunchReceipt> {
    target.validate()?;
    let _com = Com::init()?;
    let file = wide(&target.path);
    let args = wide(&target.args);
    let dir = wide(&target.cwd);
    let verb = wide("open");
    let mut receipt = LaunchReceipt {
        pid: 0,
        started: 0,
        path: None,
    };
    unsafe {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        // A .lnk is executed intact: preserve UAC flags, app IDs and its own arguments.
        if target.kind == LaunchKind::Executable {
            info.lpParameters = PCWSTR(args.as_ptr());
            if !target.cwd.is_empty() {
                info.lpDirectory = PCWSTR(dir.as_ptr());
            }
        }
        ShellExecuteExW(&mut info).context("Windows не удалось запустить игру")?;
        if !info.hProcess.is_invalid() {
            use windows::Win32::System::Threading::*;
            receipt.pid = GetProcessId(info.hProcess);
            receipt.started = crate::activity::creation_time(info.hProcess);
            let mut path = vec![0u16; 32768];
            let mut size = path.len() as u32;
            if QueryFullProcessImageNameW(
                info.hProcess,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut size,
            )
            .is_ok()
            {
                receipt.path = Some(PathBuf::from(String::from_utf16_lossy(
                    &path[..size as usize],
                )));
            }
            let _ = CloseHandle(info.hProcess);
        }
    }
    Ok(receipt)
}
const CREDENTIAL: &str = "HITDROLLauncher/SteamGridDB";
pub fn store_key(key: &str) -> Result<()> {
    let mut name = wide(CREDENTIAL);
    let mut user = wide("SteamGridDB");
    let value = key.trim();
    if value.is_empty() {
        for target in [
            CREDENTIAL.to_owned(),
            format!("{}/SteamGridDB", crate::profile::previous_product_id()),
        ] {
            let target = wide(&target);
            unsafe {
                let result = CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None);
                if let Err(e) = result
                    && e.code().0 as u32 != 0x80070490
                {
                    return Err(e.into());
                }
            }
        }
        return Ok(());
    }
    anyhow::ensure!(
        value.len() <= 2048 && value.is_ascii(),
        "Некорректный API-ключ"
    );
    let mut blob = value.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(name.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user.as_mut_ptr()),
        ..Default::default()
    };
    let result = unsafe { CredWriteW(&cred, 0) };
    blob.fill(0);
    result?;
    Ok(())
}
pub fn read_key() -> Result<Option<String>> {
    if let Some(key) = read_credential(CREDENTIAL)? {
        return Ok(Some(key));
    }
    let previous = format!("{}/SteamGridDB", crate::profile::previous_product_id());
    if let Some(key) = read_credential(&previous)? {
        store_key(&key)?;
        return Ok(Some(key));
    }
    Ok(None)
}
fn read_credential(target: &str) -> Result<Option<String>> {
    let name = wide(target);
    let mut ptr = std::ptr::null_mut();
    unsafe {
        match CredReadW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None, &mut ptr) {
            Ok(()) => {
                let cred = &*ptr;
                let key = String::from_utf8(
                    std::slice::from_raw_parts(
                        cred.CredentialBlob,
                        cred.CredentialBlobSize as usize,
                    )
                    .to_vec(),
                );
                CredFree(ptr.cast());
                Ok(Some(key?))
            }
            Err(e) if e.code().0 as u32 == 0x80070490 => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

fn library_credential(source: crate::model::Source) -> Result<&'static str> {
    match source {
        crate::model::Source::Epic => Ok("HITDROLLauncher/Epic/Refresh"),
        crate::model::Source::Gog => Ok("HITDROLLauncher/GOG/Refresh"),
        _ => anyhow::bail!("Этот источник не требует подключения"),
    }
}
pub fn library_token(source: crate::model::Source) -> Result<Option<String>> {
    read_credential(library_credential(source)?)
}
pub fn save_library_token(source: crate::model::Source, token: &str) -> Result<()> {
    let mut name = wide(library_credential(source)?);
    if token.is_empty() {
        if let Err(e) = unsafe { CredDeleteW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None) }
            && e.code().0 as u32 != 0x80070490
        {
            return Err(e.into());
        }
        return Ok(());
    }
    anyhow::ensure!(
        token.len() <= 2560 && token.is_ascii(),
        "Некорректный токен аккаунта"
    );
    let mut blob = token.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(name.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    let result = unsafe { CredWriteW(&cred, 0) };
    blob.fill(0);
    result?;
    Ok(())
}
pub fn open_account_login(source: crate::model::Source) -> Result<()> {
    let url = crate::accounts::login_url(source)?;
    let _com = Com::init()?;
    let url = wide(url.as_str());
    let verb = wide("open");
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(url.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    anyhow::ensure!(
        result.0 as isize > 32,
        "Не удалось открыть браузер для входа"
    );
    Ok(())
}
pub fn open_system_uri(uri: &str) -> Result<()> {
    anyhow::ensure!(
        uri == "ms-settings:startupapps",
        "Неподдерживаемый раздел Windows"
    );
    let _com = Com::init()?;
    let uri = wide(uri);
    let verb = wide("open");
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(uri.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    anyhow::ensure!(
        result.0 as isize > 32,
        "Не удалось открыть настройки Windows"
    );
    Ok(())
}

pub fn cache_icon(path: &Path, cache: &Path) -> Result<PathBuf> {
    use windows::Win32::{Graphics::Gdi::*, Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES};
    std::fs::create_dir_all(cache)?;
    let output = cache.join(format!(
        "{}.png",
        crate::model::stable_hash(&path.to_string_lossy())
    ));
    if output.is_file() {
        return Ok(output);
    }
    let _com = Com::init()?;
    unsafe {
        let name = wide(&path.to_string_lossy());
        let mut info = SHFILEINFOW::default();
        anyhow::ensure!(
            SHGetFileInfoW(
                PCWSTR(name.as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(0),
                Some(&mut info),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON
            ) != 0,
            "Иконка недоступна"
        );
        let result = (|| -> Result<()> {
            let dc = CreateCompatibleDC(None);
            anyhow::ensure!(!dc.is_invalid(), "Не удалось создать DC");
            let bitmap_info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: 64,
                    biHeight: -64,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut pixels = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(
                Some(dc),
                &bitmap_info,
                DIB_RGB_COLORS,
                &mut pixels,
                None,
                0,
            ) {
                Ok(b) => b,
                Err(e) => {
                    let _ = DeleteDC(dc);
                    return Err(e.into());
                }
            };
            let old = SelectObject(dc, HGDIOBJ(bitmap.0));
            std::ptr::write_bytes(pixels, 0, 64 * 64 * 4);
            let drawn = DrawIconEx(dc, 0, 0, info.hIcon, 64, 64, 0, None, DI_NORMAL);
            let mut bytes = std::slice::from_raw_parts(pixels as *const u8, 64 * 64 * 4).to_vec();
            SelectObject(dc, old);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(dc);
            drawn?;
            for p in bytes.as_chunks_mut::<4>().0 {
                p.swap(0, 2);
                if p[3] > 0 {
                    for c in 0..3 {
                        p[c] = ((p[c] as u32 * 255) / p[3] as u32).min(255) as u8;
                    }
                }
            }
            image::save_buffer(&output, &bytes, 64, 64, image::ColorType::Rgba8)?;
            Ok(())
        })();
        let _ = DestroyIcon(info.hIcon);
        result?;
    }
    Ok(output)
}
