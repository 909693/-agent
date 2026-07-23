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
    // Shared standard root (~/Library/Application Support/retl) — extensions must
    // NOT live inside the .app bundle, or a replace-install wipes them.
    let dir = crate::storage::standard_root();
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

    // Atomic write with a UNIQUE temp file. `with_extension("tmp")` produced a
    // fixed sibling name, so two concurrent writers (e.g. two MCP-registry
    // updates) clobbered each other's temp file. Use a per-target unique suffix.
    use std::sync::atomic::{AtomicU64, Ordering};
    static WJ_COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = WJ_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_name = format!(
        "{}.{}.{}.tmp",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("data"),
        std::process::id(),
        n
    );
    let tmp_path = path.with_file_name(tmp_name);
    fs::write(&tmp_path, &content).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }
    fs::rename(&tmp_path, path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        e.to_string()
    })?;
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
        let pull = run_git(&[
            "-C",
            install_path.to_string_lossy().as_ref(),
            "pull",
            "--ff-only",
            "--quiet",
        ]);
        if pull.is_ok() {
            return Ok(());
        }
        // 无法 fast-forward（上游 force-push）或网络中断时重新克隆。
        // 先克隆到临时目录，成功后才替换旧副本——克隆失败时保留原有安装。
        let tmp = sibling_tmp_path(install_path);
        let _ = fs::remove_dir_all(&tmp);
        run_git(&["clone", "--depth=1", "--quiet", repo_url, tmp.to_string_lossy().as_ref()])
            .map_err(|e| {
                let _ = fs::remove_dir_all(&tmp);
                format!("git 更新失败: {}", e)
            })?;
        fs::remove_dir_all(install_path).map_err(|e| e.to_string())?;
        fs::rename(&tmp, install_path).map_err(|e| e.to_string())?;
        return Ok(());
    }
    if install_path.exists() {
        // 残缺安装（目录存在但没有 .git）：清掉后重新克隆
        remove_dir_if_exists(install_path)?;
    }
    if let Some(parent) = install_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    run_git(&["clone", "--depth=1", "--quiet", repo_url, install_path.to_string_lossy().as_ref()])
        .map_err(|e| {
            // 清理半成品目录，避免下次被当作残缺安装
            let _ = fs::remove_dir_all(install_path);
            format!("git clone 失败: {}", e)
        })
}

fn sibling_tmp_path(path: &Path) -> PathBuf {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("repo");
    path.with_file_name(format!("{}.update-tmp", name))
}

/// git 不读取 macOS 系统代理，而 GUI 应用也不继承 shell 的 proxy 环境变量，
/// 会导致打包后的应用内 git 直连仓库直至挂死。
/// 依次探测：系统设置里配置的代理地址（llm_config.json 的 proxyUrl）
/// → 环境变量 → macOS 系统 HTTPS/HTTP 代理（scutil --proxy）。
fn detect_proxy() -> Option<String> {
    if let Ok(Some(config)) = crate::storage::load_llm_config() {
        if let Some(proxy) = config["proxyUrl"].as_str() {
            let proxy = proxy.trim();
            if !proxy.is_empty() {
                return Some(proxy.to_string());
            }
        }
    }
    for key in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"] {
        if let Ok(v) = std::env::var(key) {
            let v = v.trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("scutil").arg("--proxy").output().ok()?;
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        let get = |key: &str| -> Option<String> {
            text.lines().find_map(|line| {
                let line = line.trim();
                line.strip_prefix(key)
                    .and_then(|rest| rest.trim_start().strip_prefix(':'))
                    .map(|v| v.trim().to_string())
            })
        };
        for (enable, host_key, port_key) in [
            ("HTTPSEnable", "HTTPSProxy", "HTTPSPort"),
            ("HTTPEnable", "HTTPProxy", "HTTPPort"),
        ] {
            if get(enable).as_deref() == Some("1") {
                if let (Some(host), Some(port)) = (get(host_key), get(port_key)) {
                    if !host.is_empty() && !port.is_empty() {
                        return Some(format!("http://{}:{}", host, port));
                    }
                }
            }
        }
    }
    None
}

const GIT_TIMEOUT_SECS: u64 = 120;

fn run_git(args: &[&str]) -> Result<(), String> {
    use std::io::Read;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let mut cmd = Command::new("git");
    // 代理下 git/curl 的 HTTP/2 常见 "Error in the HTTP2 framing layer"，强制 HTTP/1.1
    cmd.arg("-c").arg("http.version=HTTP/1.1");
    if let Some(proxy) = detect_proxy() {
        cmd.arg("-c").arg(format!("http.proxy={}", proxy));
        cmd.arg("-c").arg(format!("https.proxy={}", proxy));
    }
    let mut child = cmd
        .args(args)
        // 禁止 git 弹出凭据交互（GUI 环境下会永久挂起）
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("无法启动 git: {}", e))?;

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_string(&mut stderr);
                }
                if status.success() {
                    return Ok(());
                }
                let msg = stderr.trim().to_string();
                return Err(if msg.is_empty() { format!("git 退出码 {:?}", status.code()) } else { msg });
            }
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(GIT_TIMEOUT_SECS) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "git 操作超时（{}s），请检查网络或代理设置后重试",
                        GIT_TIMEOUT_SECS
                    ));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(e.to_string()),
        }
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
