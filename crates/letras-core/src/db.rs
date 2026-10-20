use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::models::{
    Attempt, EditPlan, Grade, Line, LineContext, ReviewItem, Song, SongSummary, Stats, TagStat, ThreadMessage,
    TutorMessage,
};
use crate::tutor::{AttemptNote, Profile, WeekStat};
use crate::{Error, Result, lyrics, review};

const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE songs (
    id          INTEGER PRIMARY KEY,
    title       TEXT NOT NULL,
    artist      TEXT NOT NULL DEFAULT '',
    -- where the lyrics came from: 'pasted' or 'lrclib:<id>'
    source      TEXT NOT NULL DEFAULT 'pasted',
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE lines (
    id            INTEGER PRIMARY KEY,
    song_id       INTEGER NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
    idx           INTEGER NOT NULL,
    stanza        INTEGER NOT NULL,
    text          TEXT NOT NULL,
    repeat_of     INTEGER REFERENCES lines(id) ON DELETE CASCADE,
    interval_days REAL,
    due_at        TEXT,
    UNIQUE (song_id, idx)
);
CREATE INDEX lines_due ON lines(due_at) WHERE repeat_of IS NULL;
CREATE TABLE attempts (
    id               INTEGER PRIMARY KEY,
    line_id          INTEGER NOT NULL REFERENCES lines(id) ON DELETE CASCADE,
    translation      TEXT NOT NULL,
    score            INTEGER NOT NULL,
    verdict          TEXT NOT NULL,
    best_translation TEXT NOT NULL,
    missed_nuance    TEXT NOT NULL,
    source           TEXT NOT NULL,
    created_at       TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX attempts_line ON attempts(line_id, id);
CREATE TABLE attempt_tags (
    attempt_id INTEGER NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
    tag        TEXT NOT NULL,
    PRIMARY KEY (attempt_id, tag)
);
CREATE TABLE messages (
    id          INTEGER PRIMARY KEY,
    attempt_id  INTEGER NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
    role        TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content     TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX messages_attempt ON messages(attempt_id, id);
"#, r#"
CREATE TABLE tutor_messages (
    id          INTEGER PRIMARY KEY,
    role        TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content     TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
"#, r#"
-- The Spanish each grade was made against, so edits to a line can mark its grades stale.
ALTER TABLE attempts ADD COLUMN line_text TEXT;
UPDATE attempts SET line_text = (SELECT text FROM lines WHERE lines.id = attempts.line_id);
"#];

/// The latest attempt for each line, as a reusable CTE.
const LATEST_CTE: &str =
    "latest AS (SELECT a.* FROM attempts a WHERE a.id = (SELECT MAX(id) FROM attempts WHERE line_id = a.line_id))";

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let tx = conn.transaction()?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", i as i64 + 1)?;
        }
        tx.commit()?;
        Ok(Self { conn })
    }

    pub fn create_song(&mut self, title: &str, artist: &str, raw_lyrics: &str, source: &str) -> Result<i64> {
        let title = title.trim();
        if title.is_empty() {
            return Err(Error::Invalid("the song needs a title".into()));
        }
        let parsed = lyrics::parse(raw_lyrics);
        if parsed.is_empty() {
            return Err(Error::Invalid("no lyric lines found".into()));
        }

        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO songs (title, artist, source) VALUES (?1, ?2, ?3)",
            params![title, artist.trim(), source],
        )?;
        let song_id = tx.last_insert_rowid();
        let mut ids: Vec<i64> = Vec::with_capacity(parsed.len());
        {
            let mut insert = tx.prepare(
                "INSERT INTO lines (song_id, idx, stanza, text, repeat_of) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (idx, line) in parsed.iter().enumerate() {
                let repeat_of = line.repeat_of.map(|i| ids[i]);
                insert.execute(params![song_id, idx as i64, line.stanza, line.text, repeat_of])?;
                ids.push(tx.last_insert_rowid());
            }
        }
        tx.commit()?;
        Ok(song_id)
    }

    pub fn list_songs(&self) -> Result<Vec<SongSummary>> {
        let sql = format!(
            "WITH {LATEST_CTE}
             SELECT s.id, s.title, s.artist, s.created_at,
                    COUNT(l.id), COUNT(latest.id), AVG(latest.score)
             FROM songs s
             JOIN lines l ON l.song_id = s.id AND l.repeat_of IS NULL
             LEFT JOIN latest ON latest.line_id = l.id
             GROUP BY s.id
             ORDER BY s.id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            Ok(SongSummary {
                id: r.get(0)?,
                title: r.get(1)?,
                artist: r.get(2)?,
                created_at: r.get(3)?,
                line_count: r.get(4)?,
                graded_count: r.get(5)?,
                avg_score: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get_song(&self, id: i64) -> Result<Song> {
        let (title, artist, created_at): (String, String, String) = self
            .conn
            .query_row("SELECT title, artist, created_at FROM songs WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?
            .ok_or(Error::NotFound("song"))?;

        let mut stmt = self.conn.prepare(
            "SELECT id, idx, stanza, text, repeat_of FROM lines WHERE song_id = ?1 ORDER BY idx",
        )?;
        let mut lines = stmt
            .query_map([id], |r| {
                Ok(Line {
                    id: r.get(0)?,
                    idx: r.get(1)?,
                    stanza: r.get(2)?,
                    text: r.get(3)?,
                    repeat_of: r.get(4)?,
                    latest: None,
                    attempt_count: 0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        for line in &mut lines {
            let source = line.repeat_of.unwrap_or(line.id);
            line.latest = self.latest_attempt(source)?;
            line.attempt_count = self.attempt_count(source)?;
        }
        Ok(Song { id, title, artist, created_at, lines })
    }

    /// Saves edited lyrics, title and artist, matching lines to the saved ones so grades
    /// follow their lines. With `apply` false it only reports what would change.
    pub fn update_song(&mut self, id: i64, title: &str, artist: &str, raw_lyrics: &str, apply: bool) -> Result<EditPlan> {
        let title = title.trim();
        if title.is_empty() {
            return Err(Error::Invalid("the song needs a title".into()));
        }
        let parsed = lyrics::parse(raw_lyrics);
        if parsed.is_empty() {
            return Err(Error::Invalid("no lyric lines found".into()));
        }
        let old: Vec<(i64, String, bool)> = {
            let mut stmt = self.conn.prepare(
                "SELECT l.id, l.text, EXISTS (SELECT 1 FROM attempts a WHERE a.line_id = l.id)
                 FROM lines l WHERE l.song_id = ?1 ORDER BY l.idx",
            )?;
            stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?
        };
        if old.is_empty() && self.conn.query_row("SELECT COUNT(*) FROM songs WHERE id = ?1", [id], |r| r.get::<_, i64>(0))? == 0 {
            return Err(Error::NotFound("song"));
        }
        let old_text: Vec<String> = old.iter().map(|(_, t, _)| t.clone()).collect();
        let changes = lyrics::align(&old_text, &parsed);

        let mut plan = EditPlan::default();
        for change in &changes {
            match *change {
                lyrics::Change::Keep { old: o, new: n } if old[o].1 == parsed[n].text => plan.kept += 1,
                lyrics::Change::Keep { .. } | lyrics::Change::Edit { .. } => plan.changed += 1,
                lyrics::Change::Add { .. } => plan.added += 1,
                lyrics::Change::Remove { old: o } => {
                    plan.removed += 1;
                    plan.removed_graded += i64::from(old[o].2);
                }
            }
        }
        if !apply {
            return Ok(plan);
        }

        let tx = self.conn.transaction()?;
        tx.execute("UPDATE songs SET title = ?2, artist = ?3 WHERE id = ?1", params![id, title, artist.trim()])?;
        // Clear repeat links (so removing a chorus doesn't cascade to its copies) and move
        // positions out of the way of the unique (song, idx) constraint.
        tx.execute("UPDATE lines SET repeat_of = NULL, idx = -1 - idx WHERE song_id = ?1", [id])?;
        let mut new_ids = vec![0i64; parsed.len()];
        for change in &changes {
            match *change {
                lyrics::Change::Keep { old: o, new: n } | lyrics::Change::Edit { old: o, new: n } => {
                    tx.execute(
                        "UPDATE lines SET idx = ?2, stanza = ?3, text = ?4 WHERE id = ?1",
                        params![old[o].0, n as i64, parsed[n].stanza, parsed[n].text],
                    )?;
                    new_ids[n] = old[o].0;
                }
                lyrics::Change::Add { new: n } => {
                    tx.execute(
                        "INSERT INTO lines (song_id, idx, stanza, text) VALUES (?1, ?2, ?3, ?4)",
                        params![id, n as i64, parsed[n].stanza, parsed[n].text],
                    )?;
                    new_ids[n] = tx.last_insert_rowid();
                }
                lyrics::Change::Remove { old: o } => {
                    tx.execute("DELETE FROM lines WHERE id = ?1", [old[o].0])?;
                }
            }
        }
        for (n, line) in parsed.iter().enumerate() {
            if let Some(first) = line.repeat_of {
                tx.execute("UPDATE lines SET repeat_of = ?2 WHERE id = ?1", params![new_ids[n], new_ids[first]])?;
            }
        }
        tx.commit()?;
        Ok(plan)
    }

    /// Fixes one line's Spanish. On a line that others repeat, the repeats change too.
    pub fn edit_line(&mut self, line_id: i64, text: &str) -> Result<()> {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return Err(Error::Invalid("a line can't be empty".into()));
        }
        let tx = self.conn.transaction()?;
        let original: i64 = tx
            .query_row("SELECT COALESCE(repeat_of, id) FROM lines WHERE id = ?1", [line_id], |r| r.get(0))
            .optional()?
            .ok_or(Error::NotFound("line"))?;
        tx.execute("UPDATE lines SET text = ?2 WHERE id = ?1 OR repeat_of = ?1", params![original, text])?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_song(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM songs WHERE id = ?1", [id])?;
        Ok(())
    }

    /// The song and line Claude needs to grade `line_id`. Repeats resolve to their original line.
    pub fn line_context(&self, line_id: i64) -> Result<LineContext> {
        let (song_id, original, line): (i64, i64, String) = self
            .conn
            .query_row(
                "SELECT l.song_id, COALESCE(l.repeat_of, l.id), o.text
                 FROM lines l JOIN lines o ON o.id = COALESCE(l.repeat_of, l.id)
                 WHERE l.id = ?1",
                [line_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .ok_or(Error::NotFound("line"))?;
        let (title, artist, source): (String, String, String) = self.conn.query_row(
            "SELECT title, artist, source FROM songs WHERE id = ?1",
            [song_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;

        // Each line is marked: → the one being translated, ✓ already translated, · not yet.
        let mut stmt = self.conn.prepare(
            "SELECT l.stanza, l.text, COALESCE(l.repeat_of, l.id),
                    EXISTS (SELECT 1 FROM attempts a WHERE a.line_id = COALESCE(l.repeat_of, l.id))
             FROM lines l WHERE l.song_id = ?1 ORDER BY l.idx",
        )?;
        let rows = stmt.query_map([song_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, bool>(3)?))
        })?;
        let mut lyrics = String::new();
        let mut untranslated: Vec<String> = Vec::new();
        let mut last_stanza = None;
        for row in rows {
            let (stanza, text, source_line, done) = row?;
            if last_stanza.is_some_and(|s| s != stanza) {
                lyrics.push('\n');
            }
            let mark = if source_line == original {
                "→"
            } else if done {
                "✓"
            } else {
                if !untranslated.contains(&text) {
                    untranslated.push(text.clone());
                }
                "·"
            };
            lyrics.push_str(&format!("{mark} {text}\n"));
            last_stanza = Some(stanza);
        }

        Ok(LineContext { line_id: original, title, artist, lyrics, line, source, untranslated })
    }

    /// Records a graded attempt on an original line and reschedules it for review.
    pub fn insert_attempt(&mut self, line_id: i64, translation: &str, grade: &Grade, source: &str) -> Result<Attempt> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO attempts (line_id, translation, score, verdict, best_translation, missed_nuance, source, line_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, (SELECT text FROM lines WHERE id = ?1))",
            params![
                line_id,
                translation,
                grade.score,
                grade.verdict,
                grade.best_translation,
                serde_json::to_string(&grade.missed_nuance).expect("strings serialize"),
                source,
            ],
        )?;
        let attempt_id = tx.last_insert_rowid();
        for tag in &grade.skill_tags {
            tx.execute(
                "INSERT OR IGNORE INTO attempt_tags (attempt_id, tag) VALUES (?1, ?2)",
                params![attempt_id, tag],
            )?;
        }

        let previous: Option<f64> =
            tx.query_row("SELECT interval_days FROM lines WHERE id = ?1", [line_id], |r| r.get(0))?;
        let days = review::next_interval_days(grade.score, previous);
        tx.execute(
            "UPDATE lines SET interval_days = ?2, due_at = datetime('now', ?3) WHERE id = ?1",
            params![line_id, days, format!("+{} seconds", (days * 86_400.0) as i64)],
        )?;
        tx.commit()?;

        self.attempt(attempt_id)
    }

    /// Replaces an attempt's grade in place: same attempt, translation and time, so
    /// it isn't counted as another try. If it is the line's latest attempt, the line's
    /// review date is set from the new score.
    pub fn replace_grade(&mut self, attempt_id: i64, grade: &Grade) -> Result<Attempt> {
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "UPDATE attempts SET score = ?2, verdict = ?3, best_translation = ?4, missed_nuance = ?5,
                 line_text = (SELECT text FROM lines WHERE lines.id = attempts.line_id)
             WHERE id = ?1",
            params![
                attempt_id,
                grade.score,
                grade.verdict,
                grade.best_translation,
                serde_json::to_string(&grade.missed_nuance).expect("strings serialize"),
            ],
        )?;
        if changed == 0 {
            return Err(Error::NotFound("attempt"));
        }
        tx.execute("DELETE FROM attempt_tags WHERE attempt_id = ?1", [attempt_id])?;
        for tag in &grade.skill_tags {
            tx.execute(
                "INSERT OR IGNORE INTO attempt_tags (attempt_id, tag) VALUES (?1, ?2)",
                params![attempt_id, tag],
            )?;
        }
        let (line_id, latest): (i64, i64) = tx.query_row(
            "SELECT line_id, (SELECT MAX(id) FROM attempts WHERE line_id = a.line_id) FROM attempts a WHERE id = ?1",
            [attempt_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if latest == attempt_id {
            let days = review::next_interval_days(grade.score, None);
            tx.execute(
                "UPDATE lines SET interval_days = ?2, due_at = datetime('now', ?3) WHERE id = ?1",
                params![line_id, days, format!("+{} seconds", (days * 86_400.0) as i64)],
            )?;
        }
        tx.commit()?;
        self.attempt(attempt_id)
    }

    fn attempt(&self, id: i64) -> Result<Attempt> {
        let attempt = self.conn.query_row(
            "SELECT id, line_id, translation, score, verdict, best_translation, missed_nuance, source, created_at, line_text
             FROM attempts WHERE id = ?1",
            [id],
            attempt_from_row,
        )?;
        self.with_tags(attempt)
    }

    pub fn latest_attempt(&self, line_id: i64) -> Result<Option<Attempt>> {
        let attempt = self
            .conn
            .query_row(
                "SELECT id, line_id, translation, score, verdict, best_translation, missed_nuance, source, created_at, line_text
                 FROM attempts WHERE line_id = ?1 ORDER BY id DESC LIMIT 1",
                [line_id],
                attempt_from_row,
            )
            .optional()?;
        attempt.map(|a| self.with_tags(a)).transpose()
    }

    fn attempt_count(&self, line_id: i64) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM attempts WHERE line_id = ?1", [line_id], |r| r.get(0))?)
    }

    fn with_tags(&self, mut attempt: Attempt) -> Result<Attempt> {
        let mut stmt = self.conn.prepare("SELECT tag FROM attempt_tags WHERE attempt_id = ?1 ORDER BY tag")?;
        attempt.grade.skill_tags = stmt.query_map([attempt.id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(attempt)
    }

    pub fn thread(&self, attempt_id: i64) -> Result<Vec<ThreadMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, attempt_id, role, content, created_at FROM messages WHERE attempt_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([attempt_id], message_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn add_message(&self, attempt_id: i64, role: &str, content: &str) -> Result<ThreadMessage> {
        self.conn.execute(
            "INSERT INTO messages (attempt_id, role, content) VALUES (?1, ?2, ?3)",
            params![attempt_id, role, content],
        )?;
        Ok(self.conn.query_row(
            "SELECT id, attempt_id, role, content, created_at FROM messages WHERE id = ?1",
            [self.conn.last_insert_rowid()],
            message_from_row,
        )?)
    }

    pub fn attempt_by_id(&self, id: i64) -> Result<Attempt> {
        match self.attempt(id) {
            Err(Error::Db(rusqlite::Error::QueryReturnedNoRows)) => Err(Error::NotFound("attempt")),
            other => other,
        }
    }

    /// The most recent `limit` tutor messages, oldest first.
    pub fn tutor_thread(&self, limit: i64) -> Result<Vec<TutorMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, role, content, created_at FROM
               (SELECT * FROM tutor_messages ORDER BY id DESC LIMIT ?1)
             ORDER BY id",
        )?;
        let rows = stmt.query_map([limit], |r| {
            Ok(TutorMessage { id: r.get(0)?, role: r.get(1)?, content: r.get(2)?, created_at: r.get(3)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn add_tutor_message(&self, role: &str, content: &str) -> Result<TutorMessage> {
        self.conn.execute("INSERT INTO tutor_messages (role, content) VALUES (?1, ?2)", params![role, content])?;
        Ok(self.conn.query_row(
            "SELECT id, role, content, created_at FROM tutor_messages WHERE id = ?1",
            [self.conn.last_insert_rowid()],
            |r| Ok(TutorMessage { id: r.get(0)?, role: r.get(1)?, content: r.get(2)?, created_at: r.get(3)? }),
        )?)
    }

    pub fn clear_tutor(&self) -> Result<()> {
        self.conn.execute("DELETE FROM tutor_messages", [])?;
        Ok(())
    }

    /// Everything the tutor is told about the learner's work.
    pub fn tutor_profile(&self) -> Result<Profile> {
        let stats = self.stats()?;
        let (first_try_avg, compared_lines): (Option<f64>, i64) = self.conn.query_row(
            &format!(
                "WITH {LATEST_CTE},
                 first AS (SELECT a.* FROM attempts a WHERE a.id = (SELECT MIN(id) FROM attempts WHERE line_id = a.line_id))
                 SELECT AVG(first.score), COUNT(*) FROM first JOIN latest ON latest.line_id = first.line_id
                 WHERE latest.id != first.id"
            ),
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let retried_latest_avg: Option<f64> = self.conn.query_row(
            &format!(
                "WITH {LATEST_CTE} SELECT AVG(latest.score) FROM latest
                 WHERE (SELECT COUNT(*) FROM attempts WHERE line_id = latest.line_id) > 1"
            ),
            [],
            |r| r.get(0),
        )?;
        let (repaso_count, repaso_avg): (i64, Option<f64>) = self.conn.query_row(
            "SELECT COUNT(*), AVG(score) FROM attempts WHERE source = 'repaso'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;

        let mut stmt = self.conn.prepare(
            "SELECT strftime('%Y-W%W', created_at) AS week, COUNT(*), AVG(score)
             FROM attempts GROUP BY week ORDER BY week",
        )?;
        let weeks = stmt
            .query_map([], |r| Ok(WeekStat { week: r.get(0)?, attempts: r.get(1)?, avg_score: r.get(2)? }))?
            .collect::<rusqlite::Result<_>>()?;

        let recent = self.attempt_notes("1 = 1", "ORDER BY a.id DESC LIMIT 40")?;
        let weakest = self.attempt_notes(
            "a.score <= 2 AND a.id = (SELECT MAX(id) FROM attempts WHERE line_id = a.line_id)",
            "ORDER BY a.score ASC, a.id DESC LIMIT 15",
        )?;

        Ok(Profile { stats, first_try_avg, retried_latest_avg, compared_lines, repaso_count, repaso_avg, weeks, recent, weakest })
    }

    /// Attempts with their line and song, filtered by `condition` (SQL over `a`, the attempt).
    fn attempt_notes(&self, condition: &str, order_limit: &str) -> Result<Vec<AttemptNote>> {
        let sql = format!(
            "SELECT a.id, s.title, l.text, a.translation, a.score, a.best_translation, a.missed_nuance, a.source, a.created_at
             FROM attempts a JOIN lines l ON l.id = a.line_id JOIN songs s ON s.id = l.song_id
             WHERE {condition} {order_limit}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], |r| {
                let missed: String = r.get(6)?;
                Ok(AttemptNote {
                    attempt_id: r.get(0)?,
                    song: r.get(1)?,
                    spanish: r.get(2)?,
                    translation: r.get(3)?,
                    score: r.get(4)?,
                    best: r.get(5)?,
                    missed: serde_json::from_str(&missed).unwrap_or_default(),
                    tags: Vec::new(),
                    source: r.get(7)?,
                    created_at: r.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|mut note| {
                let mut stmt = self.conn.prepare("SELECT tag FROM attempt_tags WHERE attempt_id = ?1 ORDER BY tag")?;
                note.tags = stmt.query_map([note.attempt_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
                Ok(note)
            })
            .collect()
    }

    pub fn stats(&self) -> Result<Stats> {
        let songs = self.conn.query_row("SELECT COUNT(*) FROM songs", [], |r| r.get(0))?;
        let attempts = self.conn.query_row("SELECT COUNT(*) FROM attempts", [], |r| r.get(0))?;
        let (lines_graded, avg_score) = self.conn.query_row(
            &format!("WITH {LATEST_CTE} SELECT COUNT(*), AVG(score) FROM latest"),
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;

        let mut score_counts = [0i64; 5];
        let mut stmt =
            self.conn.prepare(&format!("WITH {LATEST_CTE} SELECT score, COUNT(*) FROM latest GROUP BY score"))?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
            let (score, count) = row?;
            if (1..=5).contains(&score) {
                score_counts[score as usize - 1] = count;
            }
        }

        let mut stmt = self.conn.prepare(&format!(
            "WITH {LATEST_CTE}
             SELECT t.tag, COUNT(*), AVG(latest.score) AS avg
             FROM latest JOIN attempt_tags t ON t.attempt_id = latest.id
             GROUP BY t.tag ORDER BY avg ASC, COUNT(*) DESC"
        ))?;
        let tags = stmt
            .query_map([], |r| Ok(TagStat { tag: r.get(0)?, count: r.get(1)?, avg_score: r.get(2)? }))?
            .collect::<rusqlite::Result<_>>()?;

        let due_count = self.conn.query_row(
            "SELECT COUNT(*) FROM lines WHERE repeat_of IS NULL AND due_at <= datetime('now')",
            [],
            |r| r.get(0),
        )?;

        Ok(Stats { songs, lines_graded, attempts, avg_score, score_counts, tags, due_count })
    }

    /// Lines due for Repaso, weakest first, optionally limited to one skill tag.
    pub fn review_queue(&self, tag: Option<&str>, limit: i64) -> Result<Vec<ReviewItem>> {
        let sql = format!(
            "WITH {LATEST_CTE}
             SELECT l.id, l.idx, l.stanza, l.text, s.id, s.title, s.artist
             FROM lines l
             JOIN songs s ON s.id = l.song_id
             JOIN latest ON latest.line_id = l.id
             WHERE l.repeat_of IS NULL
               AND l.due_at <= datetime('now')
               AND (?1 IS NULL OR EXISTS (
                     SELECT 1 FROM attempt_tags t WHERE t.attempt_id = latest.id AND t.tag = ?1))
             ORDER BY latest.score ASC, l.due_at ASC
             LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![tag, limit], |r| {
                Ok((
                    Line {
                        id: r.get(0)?,
                        idx: r.get(1)?,
                        stanza: r.get(2)?,
                        text: r.get(3)?,
                        repeat_of: None,
                        latest: None,
                        attempt_count: 0,
                    },
                    r.get::<_, i64>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        rows.into_iter()
            .map(|(mut line, song_id, song_title, artist)| {
                line.latest = self.latest_attempt(line.id)?;
                line.attempt_count = self.attempt_count(line.id)?;
                Ok(ReviewItem { song_id, song_title, artist, line })
            })
            .collect()
    }
}

fn attempt_from_row(r: &Row) -> rusqlite::Result<Attempt> {
    let missed: String = r.get(6)?;
    Ok(Attempt {
        id: r.get(0)?,
        line_id: r.get(1)?,
        translation: r.get(2)?,
        grade: Grade {
            score: r.get(3)?,
            verdict: r.get(4)?,
            best_translation: r.get(5)?,
            missed_nuance: serde_json::from_str(&missed).unwrap_or_default(),
            skill_tags: Vec::new(),
        },
        source: r.get(7)?,
        created_at: r.get(8)?,
        line_text: r.get(9)?,
    })
}

fn message_from_row(r: &Row) -> rusqlite::Result<ThreadMessage> {
    Ok(ThreadMessage {
        id: r.get(0)?,
        attempt_id: r.get(1)?,
        role: r.get(2)?,
        content: r.get(3)?,
        created_at: r.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grade(score: u8, tags: &[&str]) -> Grade {
        Grade {
            score,
            verdict: "ok".into(),
            missed_nuance: vec!["x".into()],
            best_translation: "I walk alone through the city".into(),
            skill_tags: tags.iter().map(|t| t.to_string()).collect(),
        }
    }

    #[test]
    fn song_roundtrip_with_grades_and_review() {
        let mut db = Db::open_in_memory().unwrap();
        let id = db.create_song("Ciudad", "Alguien", "Camino solo por la ciudad\nLas luces no me dicen nada\n\nCamino solo por la ciudad", "pasted").unwrap();
        let song = db.get_song(id).unwrap();
        assert_eq!(song.lines.len(), 3);
        let first = song.lines[0].id;
        assert_eq!(song.lines[2].repeat_of, Some(first));

        let ctx = db.line_context(song.lines[2].id).unwrap();
        assert_eq!(ctx.line_id, first);
        assert_eq!(ctx.lyrics, "→ Camino solo por la ciudad\n· Las luces no me dicen nada\n\n→ Camino solo por la ciudad\n");
        assert_eq!(ctx.untranslated, vec!["Las luces no me dicen nada"]);

        db.insert_attempt(first, "I walk the city alone", &grade(1, &["idiom", "vocabulary"]), "board").unwrap();
        let second_line = db.get_song(id).unwrap().lines[1].id;
        let ctx = db.line_context(second_line).unwrap();
        assert!(ctx.lyrics.starts_with("✓ Camino solo por la ciudad\n→ Las luces"));
        assert!(ctx.untranslated.is_empty());
        let song = db.get_song(id).unwrap();
        assert_eq!(song.lines[2].latest.as_ref().unwrap().grade.score, 1);
        assert_eq!(song.lines[0].latest.as_ref().unwrap().grade.skill_tags, vec!["idiom", "vocabulary"]);

        let stats = db.stats().unwrap();
        assert_eq!(stats.lines_graded, 1);
        assert_eq!(stats.score_counts[0], 1);
        assert_eq!(stats.due_count, 1);
        assert_eq!(db.review_queue(Some("idiom"), 10).unwrap().len(), 1);
        assert_eq!(db.review_queue(Some("slang"), 10).unwrap().len(), 0);

        let second = db.insert_attempt(first, "I walk alone through the city", &grade(1, &["idiom"]), "repaso").unwrap();
        assert_eq!(db.stats().unwrap().due_count, 1);
        let regraded = db.replace_grade(second.id, &grade(5, &["slang"])).unwrap();
        assert_eq!((regraded.id, regraded.grade.score), (second.id, 5));
        assert_eq!(regraded.grade.skill_tags, vec!["slang"]);
        assert_eq!(db.get_song(id).unwrap().lines[0].attempt_count, 2);
        assert_eq!(db.stats().unwrap().due_count, 0);

        let summaries = db.list_songs().unwrap();
        assert_eq!(summaries[0].line_count, 2);
        assert_eq!(summaries[0].graded_count, 1);
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    fn grade(score: u8) -> Grade {
        Grade { score, verdict: "ok".into(), missed_nuance: vec![], best_translation: "x".into(), skill_tags: vec![] }
    }

    #[test]
    fn editing_a_song_keeps_grades_on_matching_lines() {
        let mut db = Db::open_in_memory().unwrap();
        let id = db
            .create_song("Prueba", "", "Camino solo por la ciudad\nLas luses no me dicen nada\nLínea basura\n\nCamino solo por la ciudad", "pasted")
            .unwrap();
        let song = db.get_song(id).unwrap();
        let (first, typo, junk) = (song.lines[0].id, song.lines[1].id, song.lines[2].id);
        db.insert_attempt(first, "I walk alone", &grade(4), "board").unwrap();
        db.insert_attempt(typo, "The lights", &grade(2), "board").unwrap();
        db.insert_attempt(junk, "junk", &grade(1), "board").unwrap();

        let edited = "Camino solo por la ciudad\nLas luces no me dicen nada\n\nCamino solo por la ciudad\nMañana vuelvo";
        let plan = db.update_song(id, "Prueba 2", "Alguien", edited, false).unwrap();
        assert_eq!((plan.kept, plan.changed, plan.added, plan.removed, plan.removed_graded), (2, 1, 1, 1, 1));
        assert_eq!(db.get_song(id).unwrap().lines.len(), 4, "a dry run changes nothing");

        db.update_song(id, "Prueba 2", "Alguien", edited, true).unwrap();
        let song = db.get_song(id).unwrap();
        assert_eq!(song.title, "Prueba 2");
        let texts: Vec<&str> = song.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["Camino solo por la ciudad", "Las luces no me dicen nada", "Camino solo por la ciudad", "Mañana vuelvo"]);
        assert_eq!(song.lines[0].id, first);
        assert_eq!(song.lines[0].latest.as_ref().unwrap().grade.score, 4);
        assert_eq!(song.lines[2].repeat_of, Some(first), "the chorus repeat survives");
        assert_eq!(song.lines[1].id, typo, "the fixed line keeps its grades");
        let stale = song.lines[1].latest.as_ref().unwrap();
        assert_eq!(stale.line_text.as_deref(), Some("Las luses no me dicen nada"));
        assert_eq!(song.lines[1].stanza, 0);
        assert_eq!(song.lines[2].stanza, 1);
        assert_eq!(db.stats().unwrap().lines_graded, 2, "the junk line's grade went with it");
    }

    #[test]
    fn editing_a_line_fixes_its_repeats_and_regrade_clears_stale() {
        let mut db = Db::open_in_memory().unwrap();
        let id = db.create_song("Prueba", "", "Las luses no me dicen nada\nOtra línea\nLas luses no me dicen nada", "pasted").unwrap();
        let song = db.get_song(id).unwrap();
        let (orig, repeat) = (song.lines[0].id, song.lines[2].id);
        let attempt = db.insert_attempt(orig, "The lights", &grade(2), "board").unwrap();

        db.edit_line(repeat, "Las luces no me dicen nada").unwrap();
        let song = db.get_song(id).unwrap();
        assert_eq!(song.lines[0].text, "Las luces no me dicen nada");
        assert_eq!(song.lines[2].text, "Las luces no me dicen nada");
        assert_eq!(song.lines[0].latest.as_ref().unwrap().line_text.as_deref(), Some("Las luses no me dicen nada"));

        let regraded = db.replace_grade(attempt.id, &grade(4)).unwrap();
        assert_eq!(regraded.line_text.as_deref(), Some("Las luces no me dicen nada"));
    }
}
