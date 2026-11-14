//! Deduplicate early/task/Explorer starts within the current Windows session.
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::Path;
use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
        System::Threading::CreateMutexW,
    },
    core::PCWSTR,
};
pub struct Instance(HANDLE);
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
pub fn claim(root: &Path) -> Result<Option<Instance>> {
    std::fs::create_dir_all(root)?;
    let path = std::fs::canonicalize(root)?;
    let key = format!(
        "{}|{}",
        crate::early_start::user_sid()?,
        path.to_string_lossy().to_lowercase()
    );
    let digest = Sha256::digest(key.as_bytes());
    let name = format!("Local\\HITDROL-{:x}", digest);
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let handle = CreateMutexW(None, false, PCWSTR(wide.as_ptr()))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            return Ok(None);
        }
        Ok(Some(Instance(handle)))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_running_profile_can_only_be_claimed_once() {
        let profile = tempfile::tempdir().unwrap();
        let root = profile.path().join("new-profile");
        let first = claim(&root).unwrap().unwrap();
        assert!(claim(&root).unwrap().is_none());
        drop(first);
        assert!(claim(&root).unwrap().is_some());
    }
}
