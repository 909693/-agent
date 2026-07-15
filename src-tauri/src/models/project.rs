use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub id: String,
    pub title: String,
    pub genre: String,
    pub premise: String,
    pub tone: String,
    pub themes: Vec<String>,
    pub target_chapter_words: u32,
    pub language: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorldSetting {
    pub era: String,
    pub overview: String,
    pub geography: Vec<GeographyLocation>,
    pub rules: Vec<WorldRule>,
    pub factions: Vec<Faction>,
    pub history: Vec<String>,
    pub culture_notes: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeographyLocation {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub significance: String,
    #[serde(default)]
    pub connected_to: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldRule {
    pub category: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub limitations: Vec<String>,
    #[serde(default)]
    pub plot_implications: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Faction {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub goals: Vec<String>,
    #[serde(default)]
    pub key_members: Vec<String>,
}
