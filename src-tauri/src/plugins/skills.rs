use super::types::SkillRecord;
use super::util::{
    git_clone_or_pull, now_rfc3339, read_json_or_default, remove_dir_if_exists, repo_id,
    repo_install_path, repo_name, skills_registry_path, write_json,
};
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}};

fn load_registry() -> Result<Vec<SkillRecord>, String> {
    read_json_or_default(&skills_registry_path())
}

fn save_registry(records: &[SkillRecord]) -> Result<(), String> {
    write_json(&skills_registry_path(), records)
}

pub fn list_skills() -> Result<Value, String> {
    serde_json::to_value(load_registry()?).map_err(|e| e.to_string())
}

pub fn install_skill_repo(repo_url: String) -> Result<Value, String> {
    let mut records = load_registry()?;
    let id = repo_id(&repo_url)?;
    let install_path = repo_install_path("skills", &repo_url)?;
    git_clone_or_pull(&repo_url, &install_path)?;

    let now = now_rfc3339();
    let record = SkillRecord {
        id: id.clone(),
        name: repo_name(&repo_url)?,
        repo_url: repo_url.clone(),
        install_path: install_path.to_string_lossy().to_string(),
        description: String::new(),
        enabled: true,
        installed_at: now.clone(),
        updated_at: now,
    };

    if let Some(index) = records.iter().position(|item| item.id == id) {
        records[index].repo_url = record.repo_url.clone();
        records[index].install_path = record.install_path.clone();
        if records[index].name.is_empty() {
            records[index].name = record.name.clone();
        }
        records[index].enabled = true;
        if records[index].installed_at.is_empty() {
            records[index].installed_at = record.installed_at.clone();
        }
        records[index].updated_at = record.updated_at.clone();
        let result = records[index].clone();
        save_registry(&records)?;
        return serde_json::to_value(result).map_err(|e| e.to_string());
    }

    records.push(record.clone());
    save_registry(&records)?;
    serde_json::to_value(record).map_err(|e| e.to_string())
}

pub fn update_skill_repo(skill_id: String) -> Result<Value, String> {
    let mut records = load_registry()?;
    let index = records
        .iter()
        .position(|item| item.id == skill_id)
        .ok_or_else(|| "Skill not found".to_string())?;

    let path = PathBuf::from(&records[index].install_path);
    git_clone_or_pull(&records[index].repo_url, &path)?;
    records[index].updated_at = now_rfc3339();
    let result = records[index].clone();
    save_registry(&records)?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

pub fn toggle_skill_repo(skill_id: String, enabled: bool) -> Result<Value, String> {
    let mut records = load_registry()?;
    let index = records
        .iter()
        .position(|item| item.id == skill_id)
        .ok_or_else(|| "Skill not found".to_string())?;

    records[index].enabled = enabled;
    records[index].updated_at = now_rfc3339();
    let result = records[index].clone();
    save_registry(&records)?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

pub fn remove_skill_repo(skill_id: String) -> Result<(), String> {
    let mut records = load_registry()?;
    let index = records
        .iter()
        .position(|item| item.id == skill_id)
        .ok_or_else(|| "Skill not found".to_string())?;

    let record = records.remove(index);
    remove_dir_if_exists(Path::new(&record.install_path))?;
    save_registry(&records)
}

pub fn get_skill_detail(skill_id: String) -> Result<Value, String> {
    let records = load_registry()?;
    let record = records
        .iter()
        .find(|item| item.id == skill_id)
        .cloned()
        .ok_or_else(|| "Skill not found".to_string())?;

    let root = PathBuf::from(&record.install_path);
    let read_text = |name: &str| -> String {
        fs::read_to_string(root.join(name)).unwrap_or_default()
    };

    let mut references: Vec<Value> = Vec::new();
    let refs_dir = root.join("references");
    if refs_dir.exists() {
        if let Ok(entries) = fs::read_dir(&refs_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("").to_string();
                    let content = fs::read_to_string(&path).unwrap_or_default();
                    references.push(json!({"name": name, "path": format!("references/{}", path.file_name().and_then(|v| v.to_str()).unwrap_or("")), "content": content}));
                }
            }
        }
        references.sort_by(|a, b| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")));
    }

    Ok(json!({
        "record": record,
        "readme": read_text("README.md"),
        "skill": read_text("SKILL.md"),
        "references": references,
    }))
}

pub fn read_skill_file(skill_id: String, relative_path: String) -> Result<String, String> {
    // Validate relative_path to prevent path traversal
    if relative_path.contains("..") || relative_path.starts_with('/') || relative_path.starts_with('\\') || relative_path.contains('\0') {
        return Err("Invalid relative path".into());
    }
    // Block Windows absolute paths (e.g. C:\, \\server\share)
    if relative_path.len() >= 2 && relative_path.as_bytes()[1] == b':' {
        return Err("Invalid relative path".into());
    }

    let records = load_registry()?;
    let record = records
        .iter()
        .find(|item| item.id == skill_id)
        .cloned()
        .ok_or_else(|| "Skill not found".to_string())?;

    let root = PathBuf::from(&record.install_path);
    let full = root.join(&relative_path);

    // Verify path is within skill directory BEFORE canonicalize
    // to prevent TOCTOU race condition
    if !full.starts_with(&root) {
        return Err("Path traversal attempt".into());
    }

    // Additional check after canonicalize
    let canon_root = root.canonicalize().map_err(|e| e.to_string())?;
    let canon_file = full.canonicalize().map_err(|e| e.to_string())?;

    if !canon_file.starts_with(&canon_root) {
        return Err("Invalid file path".into());
    }

    // Limit file size to prevent DoS
    let metadata = fs::metadata(&canon_file).map_err(|e| e.to_string())?;
    if metadata.len() > 10_000_000 {
        // 10MB limit
        return Err("File too large (max 10MB)".into());
    }

    fs::read_to_string(&canon_file).map_err(|e| e.to_string())
}
