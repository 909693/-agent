//! API Key 本地存储。
//!
//! 早期版本用系统钥匙串(macOS Keychain / Windows Credential Manager),
//! 但未签名 / ad-hoc 签名的 app 每次 rebuild 签名身份都会变,导致钥匙串
//! ACL 拒绝读写 —— 表现为「每次启动都要重输 API Key」。本地单机工具用
//! 钥匙串得不偿失,故改为存本地文件(轻度混淆,防明文肩窥),与
//! llm_config.json 同目录。函数签名保持不变,上层无需改动。

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// 混淆用的固定字节流(仅防明文肩窥,非加密)。
const OBFUSCATE_KEY: &[u8] = b"retl-secret-obfuscation-v1";

/// secrets 文件路径:<app_root>/.secrets.json,与 llm_config.json 同目录。
fn secrets_path() -> PathBuf {
    super::app_root_dir().join(".secrets.json")
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// XOR 混淆 + hex 编码。
fn obfuscate(plain: &str) -> String {
    let bytes: Vec<u8> = plain
        .as_bytes()
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ OBFUSCATE_KEY[i % OBFUSCATE_KEY.len()])
        .collect();
    to_hex(&bytes)
}

/// 还原 obfuscate。失败返回 None。
fn deobfuscate(encoded: &str) -> Option<String> {
    let bytes = from_hex(encoded)?;
    let plain: Vec<u8> = bytes
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ OBFUSCATE_KEY[i % OBFUSCATE_KEY.len()])
        .collect();
    String::from_utf8(plain).ok()
}

/// 读取整个 secrets map(provider -> 混淆后的值)。
fn load_map() -> BTreeMap<String, String> {
    let path = secrets_path();
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return BTreeMap::new(),
    };
    serde_json::from_str(&content).unwrap_or_default()
}

/// 原子写入整个 secrets map。
fn save_map(map: &BTreeMap<String, String>) -> Result<(), String> {
    let path = secrets_path();
    let dir = path.parent().ok_or("无法确定 secrets 目录")?;
    fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {}", e))?;

    let content = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
    let tmp_path = dir.join(".secrets.json.tmp");
    fs::write(&tmp_path, &content).map_err(|e| format!("写入临时文件失败: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
    }

    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    fs::rename(&tmp_path, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("重命名文件失败: {}", e)
    })
}

/// 存储 API Key(provider 作为键)。
pub fn store_api_key(provider: &str, api_key: &str) -> Result<(), String> {
    let mut map = load_map();
    map.insert(provider.to_string(), obfuscate(api_key));
    save_map(&map)
}

/// 读取 API Key。不存在返回 Ok(None)。
pub fn get_api_key(provider: &str) -> Result<Option<String>, String> {
    let map = load_map();
    match map.get(provider) {
        Some(encoded) => Ok(deobfuscate(encoded)),
        None => Ok(None),
    }
}

/// 删除 API Key。不存在也视为成功。
#[allow(dead_code)]
pub fn delete_api_key(provider: &str) -> Result<(), String> {
    let mut map = load_map();
    if map.remove(provider).is_some() {
        save_map(&map)?;
    }
    Ok(())
}

/// 只保留 valid_ids 中的键,删除其余(供应商被删除后清理孤儿 key)。
pub fn prune_keys(valid_ids: &[String]) -> Result<(), String> {
    let mut map = load_map();
    let before = map.len();
    map.retain(|k, _| valid_ids.iter().any(|v| v == k));
    if map.len() != before {
        save_map(&map)?;
    }
    Ok(())
}
