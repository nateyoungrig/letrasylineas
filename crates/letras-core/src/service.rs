//! Operations that combine the database and the AI. The database lock is never
//! held across a network call.

use std::sync::Mutex;

use crate::models::{Attempt, ThreadMessage, TutorMessage};
use crate::{Ai, Db, Error, Result};

fn lock(db: &Mutex<Db>) -> std::sync::MutexGuard<'_, Db> {
    db.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Grades, asking again when a reply comes back unusable (empty feedback or bad JSON).
async fn grade_with_retries(
    ai: &Ai,
    ctx: &crate::models::LineContext,
    translation: &str,
) -> Result<crate::models::Grade> {
    let mut tries = 0;
    loop {
        match ai.grade(ctx, translation).await {
            Err(Error::Incomplete | Error::BadResponse(_)) if tries < 2 => tries += 1,
            other => return other,
        }
    }
}

/// Grades a translation of `line_id` (or of the line it repeats) and stores the attempt.
pub async fn grade_line(db: &Mutex<Db>, ai: &Ai, line_id: i64, translation: &str, source: &str) -> Result<Attempt> {
    if translation.trim().is_empty() {
        return Err(Error::Invalid("write a translation first".into()));
    }
    let ctx = lock(db).line_context(line_id)?;
    let grade = grade_with_retries(ai, &ctx, translation).await?;
    lock(db).insert_attempt(ctx.line_id, translation.trim(), &grade, source)
}

/// Grades an attempt's translation again and replaces its grade, without adding an attempt.
pub async fn regrade(db: &Mutex<Db>, ai: &Ai, attempt_id: i64) -> Result<Attempt> {
    let (attempt, ctx) = {
        let db = lock(db);
        let attempt = db.attempt_by_id(attempt_id)?;
        let ctx = db.line_context(attempt.line_id)?;
        (attempt, ctx)
    };
    let grade = grade_with_retries(ai, &ctx, &attempt.translation).await?;
    lock(db).replace_grade(attempt_id, &grade)
}

/// Asks the Repaso tutor something; returns the stored question and answer.
pub async fn ask_tutor(db: &Mutex<Db>, ai: &Ai, question: &str) -> Result<Vec<TutorMessage>> {
    let question = question.trim();
    if question.is_empty() {
        return Err(Error::Invalid("ask something first".into()));
    }
    let (data, history) = {
        let db = lock(db);
        (
            crate::tutor::learner_data(&db.tutor_profile()?),
            db.tutor_thread(crate::tutor::HISTORY)?,
        )
    };
    let answer = ai.tutor(&data, &history, question).await?;
    let db = lock(db);
    Ok(vec![
        db.add_tutor_message("user", question)?,
        db.add_tutor_message("assistant", &answer)?,
    ])
}

/// Asks the AI a follow-up about an attempt; returns the stored question and answer.
pub async fn ask(db: &Mutex<Db>, ai: &Ai, attempt_id: i64, question: &str) -> Result<Vec<ThreadMessage>> {
    let question = question.trim();
    if question.is_empty() {
        return Err(Error::Invalid("ask something first".into()));
    }
    let (attempt, ctx, history) = {
        let db = lock(db);
        let attempt = db.attempt_by_id(attempt_id)?;
        let ctx = db.line_context(attempt.line_id)?;
        let history = db.thread(attempt_id)?;
        (attempt, ctx, history)
    };
    let answer = ai
        .follow_up(&ctx, &attempt.translation, &attempt.grade, &history, question)
        .await?;
    let db = lock(db);
    Ok(vec![
        db.add_message(attempt_id, "user", question)?,
        db.add_message(attempt_id, "assistant", &answer)?,
    ])
}
