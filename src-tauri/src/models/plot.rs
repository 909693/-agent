use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlotPoint {
    pub id: String,
    #[serde(rename = "type")]
    pub point_type: String,
    pub summary: String,
    #[serde(default)]
    pub characters_involved: Vec<String>,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub foreshadowing: Vec<String>,
    #[serde(default)]
    pub consequences: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterOutline {
    pub number: u32,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub pov_character: String,
    #[serde(default)]
    pub plot_points: Vec<String>,
    #[serde(default)]
    pub location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Act {
    pub number: u32,
    pub title: String,
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub chapters: Vec<ChapterOutline>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlotOutline {
    pub acts: Vec<Act>,
    #[serde(default)]
    pub plot_points: Vec<PlotPoint>,
    #[serde(default)]
    pub subplots: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub id: String,
    pub timestamp: String,
    #[serde(default)]
    pub sort_key: i32,
    pub description: String,
    #[serde(default)]
    pub characters_involved: Vec<String>,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub chapter_ref: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Timeline {
    pub events: Vec<TimelineEvent>,
}
