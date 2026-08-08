use std::{fs, path::PathBuf, sync::Mutex};
use directories::ProjectDirs;
use crate::{error::{AppError, AppResult}, model::PersistedState};

pub struct StateStore {
    path: PathBuf,
    state: Mutex<PersistedState>,
}

impl StateStore {
    pub fn load() -> AppResult<Self> {
        let dirs = ProjectDirs::from("cn", "ProviderDeck", "Provider Deck")
            .ok_or_else(|| AppError::Config("无法确定应用数据目录".into()))?;
        let dir = dirs.config_dir();
        fs::create_dir_all(dir)?;
        let path = dir.join("state.json");
        let state = if path.exists() {
            serde_json::from_str(&fs::read_to_string(&path)?)
                .map_err(|e| AppError::Config(format!("状态文件解析失败：{e}")))?
        } else { PersistedState::default() };
        Ok(Self { path, state: Mutex::new(state) })
    }

    pub fn read(&self) -> PersistedState {
        self.state.lock().expect("state mutex poisoned").clone()
    }

    pub fn update<T>(&self, operation: impl FnOnce(&mut PersistedState) -> AppResult<T>) -> AppResult<T> {
        let mut guard = self.state.lock().expect("state mutex poisoned");
        let result = operation(&mut guard)?;
        let bytes = serde_json::to_vec_pretty(&*guard)
            .map_err(|e| AppError::Config(format!("状态序列化失败：{e}")))?;
        atomic_replace(&self.path, &bytes)?;
        Ok(result)
    }
}

pub fn atomic_replace(path: &std::path::Path, bytes: &[u8]) -> AppResult<()> {
    let parent = path.parent().ok_or_else(|| AppError::Config("目标路径没有父目录".into()))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".{}.{}.tmp", path.file_name().and_then(|v| v.to_str()).unwrap_or("config"), uuid::Uuid::new_v4()));
    {
        use std::io::Write;
        let mut file = fs::OpenOptions::new().create_new(true).write(true).open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    replace_path(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        AppError::Config(format!("原子替换失败：{error}"))
    })
}

#[cfg(not(windows))]
fn replace_path(temp: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    fs::rename(temp, target)
}

#[cfg(windows)]
fn replace_path(temp: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH};
    let temp_w: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    let target_w: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe { MoveFileExW(temp_w.as_ptr(), target_w.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) };
    if ok == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_replace_creates_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        atomic_replace(&path, b"one").unwrap();
        atomic_replace(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
    }
}
