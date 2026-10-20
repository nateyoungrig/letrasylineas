/// One lyric line as parsed from pasted text, before it is stored.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedLine {
    pub stanza: i64,
    pub text: String,
    /// Index (into the parsed list) of the first identical line, if this one repeats it.
    pub repeat_of: Option<usize>,
}

/// Splits pasted lyrics into lines. Blank lines start a new stanza, section
/// headers like `[Coro]` are dropped, and repeated lines point at their first occurrence.
pub fn parse(raw: &str) -> Vec<ParsedLine> {
    let mut out: Vec<ParsedLine> = Vec::new();
    let mut stanza = 0;
    let mut in_stanza = false;

    for raw_line in raw.lines() {
        let text = raw_line.trim();
        if text.is_empty() {
            if in_stanza {
                stanza += 1;
                in_stanza = false;
            }
            continue;
        }
        if text.starts_with('[') && text.ends_with(']') {
            continue;
        }
        let key = normalize(text);
        let repeat_of = out.iter().position(|l| l.repeat_of.is_none() && normalize(&l.text) == key);
        out.push(ParsedLine { stanza, text: text.to_string(), repeat_of });
        in_stanza = true;
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stanzas_headers_and_repeats() {
        let lines = parse(
            "[Verso 1]\nCamino solo por la ciudad\nLas luces no me dicen nada\n\n\n[Coro]\nCamino solo por la ciudad,\n¿Y tú?\n",
        );
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].stanza, 0);
        assert_eq!(lines[2].stanza, 1);
        assert_eq!(lines[2].repeat_of, Some(0));
        assert_eq!(lines[3].repeat_of, None);
    }
}

/// How one line of an edited song relates to the saved version.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Change {
    /// Same text (ignoring case and punctuation).
    Keep { old: usize, new: usize },
    /// Rewritten in place: keeps the old line, and its grades, with new text.
    Edit { old: usize, new: usize },
    Add { new: usize },
    Remove { old: usize },
}

/// Lines up an edited song with the saved one, in order. Unchanged lines are matched
/// first (longest common subsequence); between them, old and new lines pair up as
/// edits, and any left over are removals or additions.
pub fn align(old: &[String], new: &[ParsedLine]) -> Vec<Change> {
    let a: Vec<String> = old.iter().map(|s| normalize(s)).collect();
    let b: Vec<String> = new.iter().map(|l| normalize(&l.text)).collect();
    let (n, m) = (a.len(), b.len());
    // lcs[i][j] = length of the common subsequence of a[i..] and b[j..]
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }

    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    let (mut gap_old, mut gap_new): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
    let flush = |out: &mut Vec<Change>, gap_old: &mut Vec<usize>, gap_new: &mut Vec<usize>| {
        let paired = gap_old.len().min(gap_new.len());
        for k in 0..paired {
            out.push(Change::Edit { old: gap_old[k], new: gap_new[k] });
        }
        out.extend(gap_old[paired..].iter().map(|&old| Change::Remove { old }));
        out.extend(gap_new[paired..].iter().map(|&new| Change::Add { new }));
        gap_old.clear();
        gap_new.clear();
    };
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            flush(&mut out, &mut gap_old, &mut gap_new);
            out.push(Change::Keep { old: i, new: j });
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            gap_new.push(j);
            j += 1;
        } else {
            gap_old.push(i);
            i += 1;
        }
    }
    flush(&mut out, &mut gap_old, &mut gap_new);
    out
}

#[cfg(test)]
mod align_tests {
    use super::*;

    #[test]
    fn keeps_edits_adds_and_removes() {
        let old: Vec<String> = ["Camino solo por la ciudad", "Las luses no me dicen nada", "Línea basura", "Y el sol se va"]
            .map(String::from)
            .to_vec();
        let new = parse("Camino solo por la ciudad\nLas luces no me dicen nada\nY el sol se va\nMañana vuelvo");
        assert_eq!(
            align(&old, &new),
            vec![
                Change::Keep { old: 0, new: 0 },
                Change::Edit { old: 1, new: 1 },
                Change::Remove { old: 2 },
                Change::Keep { old: 3, new: 2 },
                Change::Add { new: 3 },
            ]
        );
    }

    #[test]
    fn punctuation_and_case_changes_count_as_unchanged() {
        let old = vec!["camino solo por la ciudad".to_string()];
        let new = parse("¡Camino solo por la ciudad!");
        assert_eq!(align(&old, &new), vec![Change::Keep { old: 0, new: 0 }]);
    }
}
