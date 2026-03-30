use chrono::Utc;
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn app_root() -> PathBuf {
    let dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("retl");
    fs::create_dir_all(&dir).ok();
    dir
}

pub fn extensions_root() -> PathBuf {
    let dir = app_root().join("extensions");
    fs::create_dir_all(&dir).ok();
    fs::create_dir_all(dir.join("skills")).ok();
    fs::create_dir_all(dir.join("mcp")).ok();
    fs::create_dir_all(dir.join("logs")).ok();
    dir
}

pub fn skills_registry_path() -> PathBuf {
    extensions_root().join("skills_registry.json")
}

pub fn mcp_registry_path() -> PathBuf {
    extensions_root().join("mcp_registry.json")
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

pub fn read_json_or_default<T>(path: &Path) -> Result<T, String>
where
    T: DeserializeOwned + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

pub fn write_json<T>(path: &Path, value: &T) -> Result<(), String>
where
    T: Serialize + ?Sized,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    fs::write(path, content).map_err(|e| e.to_string())
}

pub fn repo_id(repo_url: &str) -> Result<String, String> {
    let path = if let Some(rest) = repo_url.split("github.com/").nth(1) {
        rest
    } else if let Some(rest) = repo_url.split("github.com:").nth(1) {
        rest
    } else {
        return Err("Only GitHub repository URLs are supported".into());
    };
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    if parts.len() < 2 {
        return Err("Invalid GitHub repository URL".into());
    }
    let owner = parts[0].trim();
    let repo = parts[1].trim_end_matches(".git").trim();
    Ok(format!("{}-{}", owner, repo)
        .replace('/', "-")
        .to_lowercase())
}

pub fn repo_name(repo_url: &str) -> Result<String, String> {
    repo_id(repo_url).map(|id| id.split('-').skip(1).collect::<Vec<_>>().join("-"))
}

pub fn repo_install_path(kind: &str, repo_url: &str) -> Result<PathBuf, String> {
    let id = repo_id(repo_url)?;
    Ok(extensions_root().join(kind).join(id))
}

pub fn git_clone_or_pull(repo_url: &str, install_path: &Path) -> Result<(), String> {
    if install_path.join(".git").exists() {
        run_git(&[
            "-C",
            install_path.to_string_lossy().as_ref(),
            "pull",
            "--ff-only",
        ])
    } else {
        if install_path.exists() {
            return Err(format!(
                "Install path already exists: {}",
                install_path.display()
            ));
        }
        if let Some(parent) = install_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        run_git(&["clone", repo_url, install_path.to_string_lossy().as_ref()])
    }
}

fn run_git(args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn remove_dir_if_exists(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn read_log_excerpt(path: &Path, max_chars: usize) -> String {
    let content = fs::read_to_string(path).unwrap_or_default();
    let chars: Vec<char> = content.chars().collect();
    if chars.len() <= max_chars {
        content
    } else {
        chars[chars.len() - max_chars..].iter().collect()
    }
}
