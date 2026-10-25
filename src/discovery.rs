use crate::model::PlatformKind;
use crate::sqlite_adapter;
use anyhow::{anyhow, Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Environment {
    pub codex_home: PathBuf,
    pub sqlite_home: PathBuf,
    pub codex_executable: Option<PathBuf>,
    pub codex_version: Option<String>,
    pub platform: PlatformKind,
    pub wsl: bool,
    pub state_db: Option<PathBuf>,
    pub schema_version: Option<i64>,
}

pub fn discover(explicit_home: Option<&Path>) -> Result<Environment> {
    let codex_home = if let Some(path) = explicit_home {
        path.to_path_buf()
    } else if let Some(path) = env::var_os("CODEX_HOME") {
        PathBuf::from(path)
    } else {
        dirs::home_dir()
            .ok_or_else(|| anyhow!("cannot determine home directory"))?
            .join(".codex")
    };

    let sqlite_home = env::var_os("CODEX_SQLITE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| codex_home.clone());
    let state_db = find_state_db(&sqlite_home)?;
    let schema_version = state_db
        .as_deref()
        .and_then(|path| read_schema_version(path).ok().flatten());
    let codex_executable = find_executable("codex");
    let codex_version = codex_executable
        .as_deref()
        .and_then(|path| command_version(path).ok());
    let wsl = is_wsl();
    let platform = current_platform();

    Ok(Environment {
        codex_home,
        sqlite_home,
        codex_executable,
        codex_version,
        platform,
        wsl,
        state_db,
        schema_version,
    })
}

pub fn current_platform() -> PlatformKind {
    if is_wsl() {
        PlatformKind::Wsl
    } else if cfg!(target_os = "macos") {
        PlatformKind::Macos
    } else if cfg!(target_os = "windows") {
        PlatformKind::Windows
    } else if cfg!(target_os = "linux") {
        PlatformKind::Linux
    } else {
        PlatformKind::Unknown
    }
}

pub fn ensure_codex_stopped(codex_home: &Path) -> Result<()> {
    let default_home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")));
    if default_home.as_deref() != Some(codex_home) {
        return Ok(());
    }
    let running = if cfg!(target_os = "windows") {
        Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq Codex.exe", "/NH"])
            .output()
            .ok()
            .is_some_and(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .to_ascii_lowercase()
                    .contains("codex.exe")
            })
    } else {
        let desktop_running = Command::new("pgrep")
            .args(["-f", "Codex.app/Contents/MacOS/Codex"])
            .status()
            .is_ok_and(|status| status.success());
        let cli_running = Command::new("pgrep")
            .args(["-x", "codex"])
            .status()
            .is_ok_and(|status| status.success());
        desktop_running || cli_running
    };
    if running {
        anyhow::bail!(
            "Codex is still running. Close the Codex desktop app and all Codex CLI sessions, then retry"
        );
    }
    Ok(())
}

pub fn find_state_db(home: &Path) -> Result<Option<PathBuf>> {
    if !home.exists() {
        return Ok(None);
    }
    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(home).with_context(|| format!("read {}", home.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("state_") && name.ends_with(".sqlite") {
            candidates.push(entry.path());
        }
    }
    candidates.sort_by_key(|path| {
        path.file_stem()
            .and_then(|value| value.to_str())
            .and_then(|value| value.strip_prefix("state_"))
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0)
    });
    Ok(candidates.pop())
}

fn read_schema_version(path: &Path) -> Result<Option<i64>> {
    let connection = sqlite_adapter::open_readable(path)?;
    let exists: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='_sqlx_migrations'",
        [],
        |row| row.get(0),
    )?;
    if exists == 0 {
        return Ok(None);
    }
    let version = connection.query_row(
        "SELECT max(version) FROM _sqlx_migrations WHERE success = 1",
        [],
        |row| row.get::<_, Option<i64>>(0),
    )?;
    Ok(version)
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH").unwrap_or_default();
    for directory in env::split_paths(&path) {
        if let Some(candidate) = executable_in(&directory, name) {
            return Some(candidate);
        }
    }
    #[cfg(windows)]
    if name == "codex" {
        let roots = [
            env::var_os("LOCALAPPDATA").map(|root| PathBuf::from(root).join("OpenAI/Codex/bin")),
            env::var_os("APPDATA")
                .map(|root| PathBuf::from(root).join("npm/node_modules/@openai/codex")),
        ];
        let mut candidates = roots
            .into_iter()
            .flatten()
            .flat_map(|root| {
                walkdir::WalkDir::new(root)
                    .max_depth(8)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_type().is_file() && entry.file_name() == "codex.exe")
                    .map(|entry| entry.into_path())
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|path| {
            std::fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .ok()
        });
        if let Some(candidate) = candidates.pop() {
            return Some(candidate);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let bundled = PathBuf::from("/Applications/Codex.app/Contents/Resources/codex");
        if bundled.is_file() {
            return Some(bundled);
        }
    }
    None
}

fn executable_in(directory: &Path, name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    let candidate = directory.join(format!("{name}.exe"));
    #[cfg(not(windows))]
    let candidate = directory.join(name);
    candidate.is_file().then_some(candidate)
}

fn command_version(executable: &Path) -> Result<String> {
    let output = Command::new(executable)
        .arg("--version")
        .output()
        .with_context(|| format!("run {}", executable.display()))?;
    if !output.status.success() {
        return Err(anyhow!("codex --version failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn is_wsl() -> bool {
    if env::var_os("WSL_DISTRO_NAME").is_some() || env::var_os("WSL_INTEROP").is_some() {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(value) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
            return value.to_ascii_lowercase().contains("microsoft");
        }
    }
    false
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn ignores_npm_shell_and_cmd_shims_when_starting_native_codex() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("codex"), "#!/bin/sh").unwrap();
        std::fs::write(directory.path().join("codex.cmd"), "@echo off").unwrap();
        assert!(executable_in(directory.path(), "codex").is_none());
        let executable = directory.path().join("codex.exe");
        std::fs::write(&executable, b"fixture").unwrap();
        assert_eq!(executable_in(directory.path(), "codex"), Some(executable));
    }
}
