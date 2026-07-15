use chrono::Utc;
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use url::Url;

/// Whitelist of trusted Git hosting domains
const TRUSTED_GIT_DOMAINS: &[&str] = &[
    "github.com",
    "gitlab.com",
    "gitee.com",
];

/// Validate if a Git URL is from a trusted domain
fn is_trusted_git_url(repo_url: &str) -> bool {
    // Parse URL
    let parsed = match Url::parse(repo_url) {
        Ok(url) => url,
        Err(_) => {
            // Try SSH format: git@github.com:user/repo.git
            if repo_url.starts_with("git@") {
                let domain = repo_url.split('@').nth(1)
                    .and_then(|s| s.split(':').next());
                return domain.map(|d| TRUSTED_GIT_DOMAINS.contains(&d)).unwrap_or(false);
            }
            return false;
        }
    };

    // Check scheme
    if parsed.scheme() != "https" && parsed.scheme() != "git" {
        return false;
    }

    // Check domain
    if let Some(host) = parsed.host_str() {
        return TRUSTED_GIT_DOMAINS.contains(&host);
    }

    false
}

pub fn app_root() -> PathBuf {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("data");
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

    // Atomic write: write to temp file then rename
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, &content).map_err(|e| e.to_string())?;
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp_path, path).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn repo_id(repo_url: &str) -> Result<String, String> {
    // Support GitHub, GitLab, Gitee
    let path = if let Some(rest) = repo_url.split("github.com/").nth(1) {
        rest
    } else if let Some(rest) = repo_url.split("github.com:").nth(1) {
        rest
    } else if let Some(rest) = repo_url.split("gitlab.com/").nth(1) {
        rest
    } else if let Some(rest) = repo_url.split("gitlab.com:").nth(1) {
        rest
    } else if let Some(rest) = repo_url.split("gitee.com/").nth(1) {
        rest
    } else if let Some(rest) = repo_url.split("gitee.com:").nth(1) {
        rest
    } else {
        return Err("仅支持 GitHub / GitLab / Gitee 仓库 URL".into());
    };
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    if parts.len() < 2 {
        return Err("无效的仓库 URL".into());
    }
    let owner = parts[0].trim();
    let repo = parts[1].trim_end_matches(".git").trim();
    if owner.is_empty() || repo.is_empty() {
        return Err("仓库 URL 缺少 owner 或 repo 名称".into());
    }
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
    // Strict URL validation: only allow trusted GitHub URLs
    if !is_trusted_git_url(repo_url) {
        return Err(format!("不受信任的仓库 URL: {}", repo_url));
    }

    // Additional validation to prevent command injection
    if repo_url.contains("--") || repo_url.contains(";") || repo_url.contains("|")
        || repo_url.contains("&") || repo_url.contains('\0') || repo_url.contains('\n') {
        return Err("仓库 URL 包含非法字符".into());
    }

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
        run_git(&["clone", "--depth=1", repo_url, install_path.to_string_lossy().as_ref()])
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
    // Check file size before reading to prevent OOM on huge log files
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() > 20_000_000 {
            // >20MB: only read tail via seek
            return "[日志文件过大，仅显示末尾]\n".to_string()
                + &read_tail_bytes(path, max_chars * 4);
        }
    }
    let content = fs::read_to_string(path).unwrap_or_default();
    let chars: Vec<char> = content.chars().collect();
    if chars.len() <= max_chars {
        content
    } else {
        chars[chars.len() - max_chars..].iter().collect()
    }
}

fn read_tail_bytes(path: &Path, max_bytes: usize) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0) as usize;
    let start = len.saturating_sub(max_bytes);
    let _ = file.seek(SeekFrom::Start(start as u64));
    let mut buf = Vec::with_capacity(max_bytes.min(len));
    let _ = file.take(max_bytes as u64).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}
