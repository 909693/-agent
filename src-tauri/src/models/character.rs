use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Character {
    pub id: String,
    pub name: String,
    pub role: String,
    #[serde(default)]
    pub age: String,
    #[serde(default)]
    pub appearance: String,
    #[serde(default)]
    pub personality: String,
    #[serde(default)]
    pub backstory: String,
    #[serde(default)]
    pub motivations: Vec<String>,
    #[serde(default)]
    pub secrets: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    pub arc: Option<CharacterArc>,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
    #[serde(default)]
    pub faction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterArc {
    pub start_state: String,
    pub end_state: String,
    #[serde(default)]
    pub key_turning_points: Vec<String>,
    #[serde(default)]
    pub internal_conflict: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub target: String,
    pub rel_type: String,
    pub description: String,
    #[serde(default)]
    pub evolution: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterEnsemble {
    pub characters: Vec<Character>,
}
