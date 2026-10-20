//! Minimal Claude Messages API client (raw HTTP; there is no official Rust SDK).

use std::time::Duration;

use serde_json::{Value, json};

use crate::models::{Grade, LineContext, SKILL_TAGS, ThreadMessage, TutorMessage};
use crate::proofread::{self, ChatTurn, Proofread, Suggestion};
use crate::spoilers;
use crate::transcribe::DraftLine;
use crate::{Error, Result};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
/// Re-runs a request on Anthropic's recommended model if the first one declines it.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

const SYSTEM: &str = "\
You are a warm but exacting Spanish tutor. An English-speaking learner is translating \
song lyrics one line at a time. You see the whole song so you can read each line in context.

When grading, judge whether the learner's English captures what the line means: its \
idiomatic sense, tone, register, slang, and imagery. Natural English that keeps the meaning \
beats a word-for-word rendering; a different but valid translation still earns full marks.

Scores:
5 = meaning and tone captured; nothing important missed
4 = meaning right, a small nuance or tone slip
3 = gist right, but a real piece is missed or mistranslated
2 = partly right; a key word, idiom, or grammar point misread
1 = meaning lost or mostly wrong

Fields:
- verdict: one or two sentences to the learner (\"you\"), saying what landed and what didn't.
- missed_nuance: each specific thing they missed, naming the Spanish words involved. Empty if nothing.
- best_translation: your own idiomatic English for the line, in the context of the song.
- skill_tags: the one to three skills this line exercises.
- score: from the scale above.

No spoilers. In the lyrics, → marks the line being translated, ✓ lines the learner has \
already translated, and · lines they haven't done yet. Never translate, gloss, or paraphrase \
the meaning of a · line in English, anywhere: not in parentheses, not as an aside, not in \
follow-up answers, even when explaining how it connects to this line. You may quote a · \
line's Spanish and say how it relates (\"the next line finishes this thought\"); working out \
its meaning is the learner's job. ✓ lines you may discuss freely.

After a grade, the learner may ask follow-up questions about the line. Answer those in \
plain conversational prose (no JSON), briefly, quoting the Spanish where it helps.";

pub struct Claude {
    http: reqwest::Client,
    api_key: String,
    model: String,
}

impl Claude {
    pub fn new(api_key: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .expect("HTTP client builds");
        Self { http, api_key: api_key.into(), model: DEFAULT_MODEL.into() }
    }

    pub async fn grade(&self, ctx: &LineContext, translation: &str) -> Result<Grade> {
        let body = json!({
            "model": self.model,
            "max_tokens": 16000,
            "thinking": { "type": "adaptive" },
            "output_config": {
                "effort": "medium",
                "format": { "type": "json_schema", "schema": grade_schema() },
            },
            "fallbacks": "default",
            "system": SYSTEM,
            "messages": [{ "role": "user", "content": grade_prompt(ctx, translation) }],
        });
        let text = self.send(&body).await?;
        let mut grade: Grade =
            serde_json::from_str(&text).map_err(|e| Error::BadResponse(format!("{e}: {text}")))?;
        // Now and then a grade arrives with a score but every text field empty.
        if grade.verdict.trim().is_empty() || grade.best_translation.trim().is_empty() {
            eprintln!("letras: grade without feedback; raw reply: {text}");
            return Err(Error::Incomplete);
        }
        grade.score = grade.score.clamp(1, 5);
        grade.skill_tags.retain(|t| SKILL_TAGS.contains(&t.as_str()));
        grade.verdict = spoilers::hide(&grade.verdict, &ctx.untranslated);
        for item in &mut grade.missed_nuance {
            *item = spoilers::hide(item, &ctx.untranslated);
        }
        Ok(grade)
    }

    /// Answers a follow-up question in the thread under a graded attempt.
    pub async fn follow_up(
        &self,
        ctx: &LineContext,
        translation: &str,
        grade: &Grade,
        history: &[ThreadMessage],
        question: &str,
    ) -> Result<String> {
        let mut messages = vec![
            json!({ "role": "user", "content": grade_prompt(ctx, translation) }),
            json!({ "role": "assistant", "content": serde_json::to_string(grade).expect("grade serializes") }),
        ];
        messages.extend(history.iter().map(|m| json!({ "role": m.role, "content": m.content })));
        messages.push(json!({ "role": "user", "content": question }));

        let body = json!({
            "model": self.model,
            "max_tokens": 16000,
            "thinking": { "type": "adaptive" },
            "output_config": { "effort": "low" },
            "fallbacks": "default",
            "system": SYSTEM,
            "messages": messages,
        });
        Ok(spoilers::hide(&self.send(&body).await?, &ctx.untranslated))
    }

    /// One turn of the Repaso tutor conversation.
    pub async fn tutor(&self, learner_data: &str, history: &[TutorMessage], question: &str) -> Result<String> {
        let mut messages: Vec<Value> =
            history.iter().map(|m| json!({ "role": m.role, "content": m.content })).collect();
        // The API wants turns to alternate, starting with the learner.
        while messages.first().is_some_and(|m| m["role"] != "user") {
            messages.remove(0);
        }
        messages.push(json!({ "role": "user", "content": question }));
        let body = json!({
            "model": self.model,
            "max_tokens": 16000,
            "thinking": { "type": "adaptive" },
            "output_config": { "effort": "medium" },
            "fallbacks": "default",
            "system": format!("{}\n\n<learner_data>\n{learner_data}</learner_data>", crate::tutor::SYSTEM),
            "messages": messages,
        });
        self.send(&body).await
    }

    /// Proposes sound-alike fixes to a Whisper draft; proposals that don't sound
    /// like the original are dropped before they reach the learner.
    pub async fn proofread(&self, title: &str, artist: &str, lines: &[DraftLine]) -> Result<Proofread> {
        let body = json!({
            "model": self.model,
            "max_tokens": 16000,
            "thinking": { "type": "adaptive" },
            "output_config": {
                "effort": "medium",
                "format": { "type": "json_schema", "schema": proofread::schema() },
            },
            "fallbacks": "default",
            "system": proofread::SYSTEM,
            "messages": [{ "role": "user", "content": proofread::prompt(title, artist, lines) }],
        });
        let text = self.send(&body).await?;
        let result: Proofread =
            serde_json::from_str(&text).map_err(|e| Error::BadResponse(format!("{e}: {text}")))?;
        Ok(proofread::screen(result, lines))
    }

    /// One turn of a conversation about a line of a transcript.
    #[allow(clippy::too_many_arguments)]
    pub async fn discuss(
        &self,
        title: &str,
        artist: &str,
        lines: &[DraftLine],
        line: usize,
        suggestions: &[Suggestion],
        suspicious: Option<&str>,
        history: &[ChatTurn],
        question: &str,
    ) -> Result<String> {
        let mut messages: Vec<Value> =
            history.iter().map(|m| json!({ "role": m.role, "content": m.content })).collect();
        messages.push(json!({ "role": "user", "content": question }));
        // The transcript rides along with the learner's first message.
        let context = proofread::discuss_context(title, artist, lines, line, suggestions, suspicious);
        let first = messages[0]["content"].as_str().unwrap_or_default();
        messages[0]["content"] = json!(format!("{context}\n\n{first}"));
        let body = json!({
            "model": self.model,
            "max_tokens": 16000,
            "thinking": { "type": "adaptive" },
            "output_config": { "effort": "low" },
            "fallbacks": "default",
            "system": proofread::DISCUSS_SYSTEM,
            "messages": messages,
        });
        self.send(&body).await
    }

    /// Sends a request and returns the reply's text blocks joined together.
    async fn send(&self, body: &Value) -> Result<String> {
        let resp = self
            .http
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", FALLBACK_BETA)
            .json(body)
            .send()
            .await?;

        let status = resp.status();
        let payload: Value = resp.json().await?;
        if !status.is_success() {
            let message = payload["error"]["message"].as_str().unwrap_or("unknown error").to_string();
            return Err(Error::Api { status: status.as_u16(), message });
        }

        match payload["stop_reason"].as_str() {
            Some("refusal") => return Err(Error::Refusal),
            Some("max_tokens") => return Err(Error::Truncated),
            _ => {}
        }

        let text: String = payload["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect();
        if text.trim().is_empty() {
            return Err(Error::BadResponse("empty reply".into()));
        }
        Ok(text)
    }
}

fn grade_prompt(ctx: &LineContext, translation: &str) -> String {
    let byline = if ctx.artist.is_empty() { String::new() } else { format!(" by {}", ctx.artist) };
    let note = if ctx.source.starts_with("whisper:") {
        "\n(These lyrics were transcribed from the audio by machine and checked by ear, so a line \
         may still be misheard. Grade against the line as written; if it looks misheard, say so in \
         the verdict.)\n"
    } else {
        ""
    };
    format!(
        "Song: \"{title}\"{byline}\n\n<lyrics>\n{lyrics}</lyrics>\n{note}\n\
         Line to translate:\n<line>{line}</line>\n\n\
         My translation:\n<translation>{translation}</translation>",
        title = ctx.title,
        lyrics = ctx.lyrics,
        line = ctx.line,
        translation = translation.trim(),
    )
}

/// Reasoning fields come before the score so the score follows from them.
fn grade_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["verdict", "missed_nuance", "best_translation", "skill_tags", "score"],
        "properties": {
            "verdict": { "type": "string" },
            "missed_nuance": { "type": "array", "items": { "type": "string" } },
            "best_translation": { "type": "string" },
            "skill_tags": { "type": "array", "items": { "type": "string", "enum": SKILL_TAGS } },
            "score": { "type": "integer", "enum": [1, 2, 3, 4, 5] },
        },
    })
}
