//! Lyrics lookup via LRCLIB (https://lrclib.net), a free, open lyrics database.
//!
//! Lyrics come back verbatim from the database and are never passed through a
//! language model, so a lookup either finds real stored lyrics or finds nothing.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const SEARCH_URL: &str = "https://lrclib.net/api/search";
const USER_AGENT: &str = concat!("letrasylineas/", env!("CARGO_PKG_VERSION"), " (personal lyrics study app)");

#[derive(Debug, Clone, Serialize)]
pub struct LyricsMatch {
    /// LRCLIB record id, kept so a song's source can be traced.
    pub id: i64,
    pub track: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: f64,
    pub lyrics: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    id: i64,
    track_name: String,
    artist_name: String,
    #[serde(default)]
    album_name: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    instrumental: bool,
    #[serde(default)]
    plain_lyrics: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LookupResult {
    pub matches: Vec<LyricsMatch>,
    /// False when nothing matched the artist given and these are title-only matches.
    pub artist_matched: bool,
}

/// Searches by title and artist; if that finds nothing, by title alone, since a
/// misspelled artist ("Fino" for "Fito") otherwise hides the song.
pub async fn search(track: &str, artist: &str) -> Result<LookupResult> {
    let http = reqwest::Client::builder().timeout(Duration::from_secs(20)).user_agent(USER_AGENT).build()?;
    let (track, artist) = (track.trim(), artist.trim());

    if !artist.is_empty() {
        let matches = query(&http, &[("track_name", track), ("artist_name", artist)]).await?;
        if !matches.is_empty() {
            return Ok(LookupResult { matches, artist_matched: true });
        }
    }
    let matches = query(&http, &[("track_name", track)]).await?;
    Ok(LookupResult { matches, artist_matched: artist.is_empty() })
}

/// One search, retried a few times because LRCLIB returns intermittent 5xx errors.
async fn query(http: &reqwest::Client, params: &[(&str, &str)]) -> Result<Vec<LyricsMatch>> {
    const DELAYS_MS: [u64; 3] = [400, 1200, 3000];
    let mut attempt = 0;
    loop {
        let outcome = http.get(SEARCH_URL).query(params).send().await;
        let retryable = match &outcome {
            Ok(resp) => resp.status().is_server_error(),
            Err(e) => e.is_timeout() || e.is_connect(),
        };
        if retryable && attempt < DELAYS_MS.len() {
            tokio::time::sleep(Duration::from_millis(DELAYS_MS[attempt])).await;
            attempt += 1;
            continue;
        }
        let resp = outcome?;
        if resp.status().is_server_error() {
            return Err(Error::Unavailable(format!(
                "LRCLIB isn't responding right now ({}). Try again in a minute, or paste the lyrics.",
                resp.status().as_u16()
            )));
        }
        let records: Vec<Record> = resp.error_for_status()?.json().await?;
        return Ok(dedupe(records));
    }
}

/// Drops instrumentals and records without lyrics, and collapses the many
/// compilation-album copies that carry identical lyrics.
fn dedupe(records: Vec<Record>) -> Vec<LyricsMatch> {
    let mut out: Vec<LyricsMatch> = Vec::new();
    for r in records {
        let Some(lyrics) = r.plain_lyrics.filter(|l| !r.instrumental && !l.trim().is_empty()) else {
            continue;
        };
        if out.iter().any(|m| m.lyrics.trim() == lyrics.trim()) {
            continue;
        }
        out.push(LyricsMatch {
            id: r.id,
            track: r.track_name,
            artist: r.artist_name,
            album: r.album_name.unwrap_or_default(),
            duration_secs: r.duration.unwrap_or_default(),
            lyrics,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupes_identical_lyrics_and_skips_empty() {
        let records: Vec<Record> = serde_json::from_str(
            r#"[
              {"id":1,"trackName":"A","artistName":"X","albumName":"Single","duration":200,"instrumental":false,"plainLyrics":"hola\nadiós"},
              {"id":2,"trackName":"A","artistName":"X","albumName":"Hits 2020","duration":199,"instrumental":false,"plainLyrics":"hola\nadiós\n"},
              {"id":3,"trackName":"A","artistName":"X","albumName":null,"duration":200,"instrumental":true,"plainLyrics":null},
              {"id":4,"trackName":"A (Remix)","artistName":"X","albumName":"R","duration":250,"instrumental":false,"plainLyrics":"hola\notra cosa"}
            ]"#,
        )
        .unwrap();
        let ids: Vec<i64> = dedupe(records).iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![1, 4]);
    }

    /// Hits the real LRCLIB API: `cargo test -p letras-core -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn live_search() {
        let found = search("Despacito", "Luis Fonsi").await.unwrap();
        assert!(found.artist_matched);
        assert!(found.matches[0].lyrics.contains("Despacito"));

        let misspelled = search("El Colesterol", "Fino Olivares").await.unwrap();
        assert!(!misspelled.artist_matched);
        assert!(misspelled.matches.iter().any(|m| m.artist.contains("Fito Olivares")));
    }
}
