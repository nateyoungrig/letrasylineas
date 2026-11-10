//! The Repaso tutor: one ongoing conversation with the AI, which is given a
//! summary of all the learner's graded work with each message.

use crate::models::Stats;

#[derive(Debug, Clone)]
pub struct WeekStat {
    pub week: String,
    pub attempts: i64,
    pub avg_score: f64,
}

#[derive(Debug, Clone)]
pub struct AttemptNote {
    pub attempt_id: i64,
    pub song: String,
    pub spanish: String,
    pub translation: String,
    pub score: i64,
    pub best: String,
    pub missed: Vec<String>,
    pub tags: Vec<String>,
    pub source: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub stats: Stats,
    /// Average first-try score on lines tried more than once.
    pub first_try_avg: Option<f64>,
    /// Average latest score on those same lines.
    pub retried_latest_avg: Option<f64>,
    pub compared_lines: i64,
    pub repaso_count: i64,
    pub repaso_avg: Option<f64>,
    pub weeks: Vec<WeekStat>,
    /// Newest first.
    pub recent: Vec<AttemptNote>,
    /// Lines whose latest score is 1 or 2.
    pub weakest: Vec<AttemptNote>,
}

/// How many earlier tutor messages are sent back with each question.
pub const HISTORY: i64 = 40;

pub const SYSTEM: &str = "\
You are a one-on-one Spanish tutor for a single English-speaking learner. They study Spanish \
by translating song lyrics line by line in an app called Letras y Líneas, where each translation \
is graded 1 to 5. The learner_data below is everything the app knows about their work: scores, \
the skills each line exercised, their translations, and what they missed. It is refreshed with \
every message.

How to tutor:
- Ground what you say in their data. Cite specific lines and mistakes when it helps. Never \
invent history that isn't in the data, and say so when there's too little data to judge.
- Asked about their level, give a CEFR estimate (A1 to C2) for understanding Spanish song \
lyrics, with how confident you are and why. Note that translating lyrics measures \
comprehension, not speaking or writing.
- Teach at their level: a short explanation, a few examples, then a quick check question when \
it helps. Offer practice built around their weak skills, such as mini quizzes or sentences to \
translate. Correct their Spanish whenever they write in it.
- Explain in English with Spanish examples; use more Spanish if they ask for it or their level \
clearly allows.
- Keep replies focused and fairly short. Light Markdown is fine: bold, short lists, small tables \
for conjugations.
- Write your own example and practice sentences. Don't quote song lyrics beyond the lines in \
their data, and don't translate lines from their songs that aren't in the data: they haven't \
done those yet, so help them work those out instead.";

/// The learner summary placed in the system prompt.
pub fn learner_data(p: &Profile) -> String {
    let s = &p.stats;
    if s.attempts == 0 {
        return "No graded work yet: the learner hasn't translated any lines.".to_string();
    }
    let avg = |v: Option<f64>| v.map_or("n/a".to_string(), |v| format!("{v:.2}"));
    let mut out = String::new();
    out.push_str(&format!(
        "Overview: {} songs, {} lines translated, {} graded attempts including retries. \
         Average latest score {}. Latest scores by value: 1: {}, 2: {}, 3: {}, 4: {}, 5: {}. \
         Lines due for review now: {}.\n",
        s.songs,
        s.lines_graded,
        s.attempts,
        avg(s.avg_score),
        s.score_counts[0],
        s.score_counts[1],
        s.score_counts[2],
        s.score_counts[3],
        s.score_counts[4],
        s.due_count,
    ));
    if p.compared_lines > 0 {
        out.push_str(&format!(
            "Retries: on {} lines tried more than once, first tries averaged {} and latest tries {}.\n",
            p.compared_lines,
            avg(p.first_try_avg),
            avg(p.retried_latest_avg),
        ));
    }
    if p.repaso_count > 0 {
        out.push_str(&format!(
            "Review sessions: {} attempts from spaced review, averaging {}.\n",
            p.repaso_count,
            avg(p.repaso_avg)
        ));
    }

    if !p.weeks.is_empty() {
        out.push_str("\nBy week (attempts, average score):\n");
        for w in &p.weeks {
            out.push_str(&format!("- {}: {}, {:.2}\n", w.week, w.attempts, w.avg_score));
        }
    }
    if !s.tags.is_empty() {
        out.push_str("\nSkills, weakest first (average latest score over lines):\n");
        for t in &s.tags {
            out.push_str(&format!("- {}: {:.2} over {}\n", t.tag, t.avg_score, t.count));
        }
    }
    if !p.weakest.is_empty() {
        out.push_str("\nLines still weak (latest score 1 or 2):\n");
        for n in &p.weakest {
            out.push_str(&note(n));
        }
    }
    out.push_str("\nRecent attempts, newest first:\n");
    for n in &p.recent {
        out.push_str(&note(n));
    }
    out
}

fn note(n: &AttemptNote) -> String {
    let clip = |s: &str, max: usize| {
        if s.chars().count() > max { format!("{}…", s.chars().take(max).collect::<String>()) } else { s.to_string() }
    };
    let mut line = format!(
        "- [{}, {}{}] \"{}\" → their translation \"{}\": {}/5",
        clip(&n.song, 40),
        n.created_at.get(..10).unwrap_or(&n.created_at),
        if n.source == "repaso" { ", review" } else { "" },
        n.spanish,
        clip(&n.translation, 200),
        n.score,
    );
    if n.score < 5 && !n.best.is_empty() {
        line.push_str(&format!("; better: \"{}\"", clip(&n.best, 200)));
    }
    if !n.missed.is_empty() {
        let missed: Vec<String> = n.missed.iter().take(3).map(|m| clip(m, 160)).collect();
        line.push_str(&format!("; missed: {}", missed.join(" | ")));
    }
    if !n.tags.is_empty() {
        line.push_str(&format!("; skills: {}", n.tags.join(", ")));
    }
    line.push('\n');
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db;
    use crate::models::Grade;

    fn grade(score: u8, tags: &[&str]) -> Grade {
        Grade {
            score,
            verdict: "ok".into(),
            missed_nuance: if score < 5 { vec!["'a mil' means racing".into()] } else { vec![] },
            best_translation: "My heart is racing".into(),
            skill_tags: tags.iter().map(|t| t.to_string()).collect(),
        }
    }

    #[test]
    fn profile_summarizes_work() {
        let mut db = Db::open_in_memory().unwrap();
        assert!(learner_data(&db.tutor_profile().unwrap()).starts_with("No graded work yet"));

        let id = db.create_song("Prueba", "Alguien", "Me late el corazón a mil\nCamino solo por la ciudad", "pasted").unwrap();
        let song = db.get_song(id).unwrap();
        let (a, b) = (song.lines[0].id, song.lines[1].id);
        db.insert_attempt(a, "My heart beats at a thousand", &grade(2, &["idiom"]), "board").unwrap();
        db.insert_attempt(b, "I walk alone through the city", &grade(5, &["vocabulary"]), "board").unwrap();
        db.insert_attempt(a, "My heart is pounding", &grade(4, &["idiom"]), "repaso").unwrap();

        let p = db.tutor_profile().unwrap();
        assert_eq!((p.stats.attempts, p.compared_lines, p.repaso_count), (3, 1, 1));
        assert_eq!((p.first_try_avg, p.retried_latest_avg), (Some(2.0), Some(4.0)));
        assert_eq!(p.recent.len(), 3);
        assert_eq!(p.recent[0].translation, "My heart is pounding");
        assert!(p.weakest.is_empty(), "the weak line was fixed on retry");

        let text = learner_data(&p);
        assert!(text.contains("2 lines translated, 3 graded attempts"));
        assert!(text.contains("first tries averaged 2.00 and latest tries 4.00"));
        assert!(text.contains("- idiom: 4.00 over 1"));
        assert!(text.contains("\"Me late el corazón a mil\" → their translation \"My heart is pounding\": 4/5"));
        assert!(text.contains(", review]"));
    }

    #[test]
    fn tutor_messages_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        for i in 0..5 {
            db.add_tutor_message(if i % 2 == 0 { "user" } else { "assistant" }, &format!("m{i}")).unwrap();
        }
        let last3: Vec<String> = db.tutor_thread(3).unwrap().into_iter().map(|m| m.content).collect();
        assert_eq!(last3, ["m2", "m3", "m4"]);
        db.clear_tutor().unwrap();
        assert!(db.tutor_thread(10).unwrap().is_empty());
    }
}

#[cfg(test)]
mod upgrade {
    /// Opens a copy of a real database: LETRAS_DB_COPY=path cargo test -p letras-core upgrade -- --ignored --nocapture
    #[test]
    #[ignore]
    fn upgrades_existing_library() {
        let path = std::env::var("LETRAS_DB_COPY").expect("set LETRAS_DB_COPY");
        let db = crate::Db::open(&path).unwrap();
        let p = db.tutor_profile().unwrap();
        let data = super::learner_data(&p);
        println!(
            "opened: {} attempts, {} recent notes, {} weak lines, {} weeks, summary {} chars (~{} tokens)",
            p.stats.attempts, p.recent.len(), p.weakest.len(), p.weeks.len(), data.len(), data.len() / 4
        );
    }
}
