//! A guarded Claude pass over a Whisper draft. Claude may only propose swapping a
//! word or short phrase for one that sounds nearly the same; every proposal is
//! checked here for sound-alikeness and shown to the learner to accept by ear.

use serde::{Deserialize, Serialize};

use crate::transcribe::{DraftLine, UNSURE_BELOW};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub line: usize,
    pub original: String,
    pub replacement: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuspiciousLine {
    pub line: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Proofread {
    pub suggestions: Vec<Suggestion>,
    pub suspicious_lines: Vec<SuspiciousLine>,
    /// How many of Claude's proposals failed the sound-alike check and were dropped.
    #[serde(default)]
    pub rejected: usize,
}

pub const SYSTEM: &str = "\
You proofread machine transcriptions of sung Spanish. A speech recognizer (Whisper) \
transcribed a song's isolated vocals; it sometimes picks a word that sounds right but \
makes no sense in context. Words it was unsure of are marked like [?word].

Propose a fix only when a word or short phrase doesn't make sense where it is and a \
similar-sounding alternative clearly does (for example caza/casa, haber/a ver, echo/hecho, \
rayo/rallo). The replacement must sound nearly the same as the original when sung.

Do not:
- rewrite lines that already make sense, or improve grammar, style, or rhymes;
- standardize slang or regional spellings (pa', to', 'ta, vo', ustedes/vosotros);
- use any memory of this song's published lyrics. Work only from the transcript; if you \
can't tell from the transcript, leave it.

Separately, list lines that look like recognizer artifacts rather than singing: subtitle \
credits, calls to subscribe, or text that doesn't belong in a song.

Line numbers refer to the [n] labels. Most transcripts need few or no fixes; empty lists are fine.";

/// One message in a discussion of a line, oldest first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
}

pub const DISCUSS_SYSTEM: &str = "\
You help a learner proofread a machine transcription of sung Spanish. A speech recognizer \
(Whisper) transcribed the song's isolated vocals; words it was unsure of are marked like \
[?word]. The learner wants to talk about one line: whether it was heard right, what a word \
is, or a change you proposed earlier. The learner can hear the recording and you can't, so \
take what they say they hear seriously.

- Reason from the transcript, how Spanish words sound when sung, and what makes sense in \
context. Never use any memory of this song's published lyrics; if the transcript can't \
settle it, say so and suggest listening again.
- A fix should sound nearly the same as what was sung. When you suggest one, say exactly \
which words to replace with what. Don't rewrite lines that already make sense, and leave \
slang and regional spellings alone.
- The learner will translate this song into English later, so don't translate or paraphrase \
its lines in English. Talk about the Spanish: the word, its sense, grammar, spelling, sound.
- Answer briefly in plain conversational prose; light Markdown is fine.";

/// The transcript, the line under discussion, and anything already said about it.
pub fn discuss_context(
    title: &str,
    artist: &str,
    lines: &[DraftLine],
    line: usize,
    suggestions: &[Suggestion],
    suspicious: Option<&str>,
) -> String {
    let mut out = format!("{}\n\nThe line to discuss: [{line}]", prompt(title, artist, lines));
    for s in suggestions {
        out.push_str(&format!(
            "\nYour proposed change: \"{}\" → \"{}\" (your reason: {})",
            s.original, s.replacement, s.reason
        ));
    }
    if let Some(reason) = suspicious {
        out.push_str(&format!("\nYou flagged it as possibly not singing: {reason}"));
    }
    out
}

pub fn schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["suggestions", "suspicious_lines"],
        "properties": {
            "suggestions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["line", "original", "replacement", "reason"],
                    "properties": {
                        "line": { "type": "integer" },
                        "original": { "type": "string" },
                        "replacement": { "type": "string" },
                        "reason": { "type": "string" },
                    },
                },
            },
            "suspicious_lines": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["line", "reason"],
                    "properties": {
                        "line": { "type": "integer" },
                        "reason": { "type": "string" },
                    },
                },
            },
        },
    })
}

/// The transcript as Claude sees it: numbered lines, unsure words marked.
pub fn prompt(title: &str, artist: &str, lines: &[DraftLine]) -> String {
    let mut out = format!("Song: \"{title}\" by {artist}\n\n<transcript>\n");
    let mut stanza = lines.first().map_or(0, |l| l.stanza);
    for (i, line) in lines.iter().enumerate() {
        if line.stanza != stanza {
            out.push('\n');
            stanza = line.stanza;
        }
        out.push_str(&format!("[{i}] {}\n", marked(line)));
    }
    out.push_str("</transcript>");
    out
}

fn marked(line: &DraftLine) -> String {
    let joined: String = line.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
    // Words only describe the line while the learner hasn't edited it.
    if line.words.is_empty() || normalize(&joined) != normalize(&line.text) {
        return line.text.clone();
    }
    line.words
        .iter()
        .map(|w| if w.prob < UNSURE_BELOW { format!("[?{}]", w.text) } else { w.text.clone() })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Keeps only proposals that point at real text in the line and sound like it.
pub fn screen(mut result: Proofread, lines: &[DraftLine]) -> Proofread {
    let before = result.suggestions.len();
    result.suggestions.retain_mut(|s| {
        s.original = s.original.trim_matches(|c| c == '[' || c == ']' || c == '?').trim().to_string();
        s.replacement = s.replacement.trim().to_string();
        let Some(line) = lines.get(s.line) else { return false };
        !s.original.is_empty()
            && s.original != s.replacement
            && line.text.to_lowercase().contains(&s.original.to_lowercase())
            && sounds_alike(&s.original, &s.replacement)
    });
    result.rejected = before - result.suggestions.len();
    result.suspicious_lines.retain(|s| s.line < lines.len());
    result
}

/// Close in sound: at most one word more or fewer, and a small edit distance once
/// both are reduced to a rough Spanish pronunciation.
pub fn sounds_alike(a: &str, b: &str) -> bool {
    let (wa, wb) = (a.split_whitespace().count(), b.split_whitespace().count());
    if wa.abs_diff(wb) > 1 || wa > 4 || wb > 4 {
        return false;
    }
    let (pa, pb) = (phonetic(a), phonetic(b));
    let longest = pa.chars().count().max(pb.chars().count()).max(1);
    let distance = levenshtein(&pa, &pb);
    distance <= 2.max(longest * 2 / 5)
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A rough sung-Spanish pronunciation: accents dropped, silent h removed, and
/// letters that sound the same merged (b/v, c/s/z, ll/y, qu/k, g/j before e and i).
fn phonetic(s: &str) -> String {
    let plain: String = s
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' => 'a',
            'é' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'ï' => 'i',
            'ó' | 'ò' | 'ö' => 'o',
            'ú' | 'ù' | 'ü' => 'u',
            c => c,
        })
        .filter(|c| c.is_alphabetic())
        .collect();
    let chars: Vec<char> = plain.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let next = chars.get(i + 1).copied();
        let soft = matches!(next, Some('e' | 'i'));
        match (chars[i], next) {
            ('c', Some('h')) => {
                out.push('C');
                i += 1;
            }
            ('l', Some('l')) => {
                out.push('y');
                i += 1;
            }
            ('q', Some('u')) => {
                out.push('k');
                i += 1;
            }
            ('g', Some('u')) if matches!(chars.get(i + 2), Some('e' | 'i')) => {
                out.push('g');
                i += 1;
            }
            ('h', _) => {}
            ('v', _) => out.push('b'),
            ('z', _) => out.push('s'),
            ('c', _) if soft => out.push('s'),
            ('c', _) => out.push('k'),
            ('g', _) if soft => out.push('x'),
            ('j', _) => out.push('x'),
            ('x', _) => out.push('x'),
            ('ñ', _) => out.push('N'),
            ('y', None) => out.push('i'),
            (c, _) => out.push(c),
        }
        i += 1;
    }
    out
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_alikes_pass_and_rewrites_fail() {
        assert!(sounds_alike("caza", "casa"));
        assert!(sounds_alike("haber", "a ver"));
        assert!(sounds_alike("echo", "hecho"));
        assert!(sounds_alike("tuvo", "tubo"));
        assert!(sounds_alike("cayo", "calló"));
        assert!(!sounds_alike("perro", "gato"));
        assert!(!sounds_alike("la noche", "el amor de mi vida"));
        assert!(!sounds_alike("corazón", "camino"));
    }

    #[test]
    fn screening_drops_bad_proposals() {
        let line = |text: &str| DraftLine { text: text.into(), start: 0.0, end: 1.0, stanza: 0, words: vec![], flags: vec![] };
        let lines = vec![line("vamos a la caza de mi madre")];
        let s = |original: &str, replacement: &str| Suggestion { line: 0, original: original.into(), replacement: replacement.into(), reason: String::new() };
        let result = screen(
            Proofread {
                suggestions: vec![s("[?caza]", "casa"), s("madre", "playa"), s("perro", "pero"), Suggestion { line: 5, ..s("caza", "casa") }],
                suspicious_lines: vec![SuspiciousLine { line: 9, reason: String::new() }],
                rejected: 0,
            },
            &lines,
        );
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].original, "caza");
        assert_eq!(result.rejected, 3);
        assert!(result.suspicious_lines.is_empty());
    }
}
