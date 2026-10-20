//! A backstop for the no-spoilers rule: Claude sees the whole song, and sometimes
//! quotes a line the learner hasn't translated yet with its translation in
//! parentheses. Those parentheticals are blanked out here.

/// Replaces "(…)" right after a quote of any `untranslated` line (or a run of four or
/// more of its words) with "(…)".
pub fn hide(text: &str, untranslated: &[String]) -> String {
    let mut out = text.to_string();
    for line in untranslated {
        for needle in needles(line) {
            out = blank_after(&out, &needle);
        }
    }
    out
}

/// The whole line and every four-word run of it, lowercased, punctuation trimmed.
fn needles(line: &str) -> Vec<String> {
    let words: Vec<&str> = line
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| !w.is_empty())
        .collect();
    let mut out = vec![words.join(" ").to_lowercase()];
    if words.len() > 4 {
        out.extend(words.windows(4).map(|w| w.join(" ").to_lowercase()));
    }
    out.retain(|n| n.split(' ').count() >= 3 || n.chars().count() >= 12);
    out
}

fn blank_after(text: &str, needle: &str) -> String {
    let lower = text.to_lowercase();
    // Lowercasing can change byte lengths; only search when it doesn't.
    if lower.len() != text.len() || needle.is_empty() {
        return text.to_string();
    }
    let mut out = String::new();
    let mut from = 0;
    while let Some(found) = lower[from..].find(needle) {
        let end = from + found + needle.len();
        // Skip the rest of the quoted line: closing punctuation, quotes, spaces.
        let mut paren = end;
        for (i, c) in text[end..].char_indices() {
            if c == '(' {
                paren = end + i;
                break;
            }
            if c.is_alphanumeric() || i > 6 {
                break;
            }
        }
        let close = (paren > end || text[end..].starts_with('('))
            .then(|| text[paren..].find(')').map(|i| paren + i))
            .flatten();
        match close {
            Some(close) if text[paren..].starts_with('(') => {
                out.push_str(&text[from..=paren]);
                out.push('…');
                from = close;
            }
            _ => {
                out.push_str(&text[from..end]);
                from = end;
            }
        }
    }
    out.push_str(&text[from..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blanks_translations_of_untranslated_lines() {
        let untranslated = vec!["Las luces no me dicen nada".to_string()];
        let text = "This sets up the next line, \"Las luces no me dicen nada\" (the lights tell me nothing), where the mood turns.";
        assert_eq!(
            hide(text, &untranslated),
            "This sets up the next line, \"Las luces no me dicen nada\" (…), where the mood turns."
        );
    }

    #[test]
    fn catches_partial_quotes_and_leaves_other_parentheses() {
        let untranslated = vec!["Y cuando vuelvas a la casa vieja".to_string()];
        let text = "Compare \"cuando vuelvas a la casa\" (when you come back home) with \"a mil\" (racing).";
        assert_eq!(hide(text, &untranslated), "Compare \"cuando vuelvas a la casa\" (…) with \"a mil\" (racing).");
    }

    #[test]
    fn leaves_quotes_without_a_translation() {
        let untranslated = vec!["Las luces no me dicen nada".to_string()];
        let text = "The next line, «Las luces no me dicen nada», answers this one.";
        assert_eq!(hide(text, &untranslated), text);
    }
}
