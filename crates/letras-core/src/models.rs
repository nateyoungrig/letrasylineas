use serde::{Deserialize, Serialize};

/// The skills Claude may tag a line with. A fixed list keeps the dashboard's
/// per-skill averages meaningful across songs.
pub const SKILL_TAGS: &[&str] = &[
    "vocabulary",
    "idiom",
    "slang",
    "verb-tense",
    "subjunctive",
    "imperative",
    "ser-estar",
    "por-para",
    "pronouns",
    "reflexive",
    "prepositions",
    "gender-number",
    "word-order",
    "metaphor",
    "cultural-reference",
    "register",
    "colloquial-spelling",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Grade {
    /// 1 (meaning lost) to 5 (meaning and tone captured).
    pub score: u8,
    pub verdict: String,
    pub missed_nuance: Vec<String>,
    pub best_translation: String,
    pub skill_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Attempt {
    pub id: i64,
    pub line_id: i64,
    pub translation: String,
    pub grade: Grade,
    pub source: String,
    pub created_at: String,
    /// The line's Spanish when this was graded; differs from the line if it was edited since.
    pub line_text: Option<String>,
}

/// What saving an edited song does (or would do) to its lines.
#[derive(Debug, Clone, Default, Serialize)]
pub struct EditPlan {
    pub kept: i64,
    pub changed: i64,
    pub added: i64,
    pub removed: i64,
    /// Removed lines that have grades, which are deleted with them.
    pub removed_graded: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Line {
    pub id: i64,
    pub idx: i64,
    pub stanza: i64,
    pub text: String,
    /// Set when this line repeats an earlier one (a chorus); grades live on the original.
    pub repeat_of: Option<i64>,
    pub latest: Option<Attempt>,
    pub attempt_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Song {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub created_at: String,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SongSummary {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub created_at: String,
    /// Distinct lines (repeats excluded).
    pub line_count: i64,
    pub graded_count: i64,
    pub avg_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadMessage {
    pub id: i64,
    pub attempt_id: i64,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TutorMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagStat {
    pub tag: String,
    pub count: i64,
    pub avg_score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub songs: i64,
    pub lines_graded: i64,
    pub attempts: i64,
    pub avg_score: Option<f64>,
    /// Count of latest grades at each score, index 0 = score 1.
    pub score_counts: [i64; 5],
    /// Weakest first.
    pub tags: Vec<TagStat>,
    pub due_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewItem {
    pub song_id: i64,
    pub song_title: String,
    pub artist: String,
    pub line: Line,
}

/// Everything Claude needs to judge one line in context.
#[derive(Debug, Clone)]
pub struct LineContext {
    pub line_id: i64,
    pub title: String,
    pub artist: String,
    pub lyrics: String,
    pub line: String,
    /// Where the lyrics came from: pasted, lrclib:<id>, youtube-subs:<id> or whisper:<id>.
    pub source: String,
    /// Other lines the learner hasn't translated yet, which Claude must not translate.
    pub untranslated: Vec<String>,
}
