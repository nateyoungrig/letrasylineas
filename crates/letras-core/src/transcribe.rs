//! Lyrics from a YouTube video: uploaded Spanish captions when the video has them,
//! otherwise Demucs (vocal isolation) and faster-whisper running locally.
//!
//! The result is a [`Draft`] for the learner to check by ear before saving. Nothing
//! here asks a language model for lyrics; see `proofread` for the guarded Claude pass.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::{Error, Result};

const WHISPER_SCRIPT: &str = include_str!("../scripts/transcribe.py");
const PYTHON: &str = "3.11";
/// Recent torchaudio needs torchcodec to save audio, which Demucs does not use, and
/// Demucs imports NumPy without declaring it, counting on older PyTorch to bring it.
const DEMUCS_PINS: [&str; 6] = ["--with", "torch<2.9", "--with", "torchaudio<2.9", "--with", "numpy<2"];
/// CUDA libraries for faster-whisper on the GPU: the same builds PyTorch (for Demucs)
/// installs, so uv reuses its cached copies instead of downloading them again.
const WHISPER_CUDA_LIBS: [&str; 4] = ["--with", "nvidia-cublas-cu12==12.8.4.1", "--with", "nvidia-cudnn-cu12==9.10.2.21"];
const MAX_DURATION_SECS: f64 = 15.0 * 60.0;
/// A gap this long between sung lines starts a new stanza.
const STANZA_GAP_SECS: f64 = 3.0;
/// Words Whisper gave less than this probability are marked unsure.
pub const UNSURE_BELOW: f64 = 0.5;

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    /// metadata | download | captions | separate | transcribe | done
    pub stage: &'static str,
    pub message: String,
    /// 0..1 within the stage, when known.
    pub fraction: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub prob: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftLine {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub stanza: i64,
    /// Empty for uploaded captions, which carry no confidence.
    pub words: Vec<Word>,
    /// unsure | no-speech | stock-phrase | loop
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    pub video_id: String,
    pub url: String,
    pub title: String,
    pub artist: String,
    /// "youtube-subs" (uploaded captions) or "whisper"
    pub method: String,
    pub audio_path: String,
    pub vocals_path: Option<String>,
    pub device: Option<String>,
    pub lines: Vec<DraftLine>,
}

struct Tools {
    yt_dlp: PathBuf,
    uv: PathBuf,
}

impl Tools {
    fn find() -> Result<Self> {
        let yt_dlp = find_tool("yt-dlp")
            .ok_or_else(|| Error::Invalid("yt-dlp isn't installed (sudo pacman -S yt-dlp)".into()))?;
        let uv = find_tool("uv")
            .ok_or_else(|| Error::Invalid("uv isn't installed (https://docs.astral.sh/uv/)".into()))?;
        Ok(Self { yt_dlp, uv })
    }
}

/// Looks on PATH, then where uv and mise usually live, since an app started
/// from the desktop may not inherit the shell's PATH.
fn find_tool(name: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let extra = [".local/bin", ".cargo/bin", ".local/share/mise/shims"].map(|d| home.join(d));
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(extra)
        .chain(["/usr/local/bin", "/usr/bin"].map(PathBuf::from))
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// Builds a draft for `url`, reusing earlier work saved under `cache_dir`.
pub async fn transcribe(url: &str, cache_dir: &Path, progress: impl Fn(Progress)) -> Result<Draft> {
    let tools = Tools::find()?;
    let say = |stage, message: &str, fraction| progress(Progress { stage, message: message.into(), fraction });

    say("metadata", "Looking up the video…", None);
    let meta = metadata(&tools, url).await?;
    let id = meta["id"].as_str().filter(|id| id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    let id = id.ok_or_else(|| Error::BadResponse("yt-dlp gave no usable video id".into()))?.to_string();
    if meta["duration"].as_f64().is_some_and(|d| d > MAX_DURATION_SECS) {
        return Err(Error::Invalid("that video is over 15 minutes; pick a single song".into()));
    }

    let dir = cache_dir.join("transcribe").join(&id);
    let draft_path = dir.join("draft.json");
    if let Ok(saved) = tokio::fs::read(&draft_path).await
        && let Ok(draft) = serde_json::from_slice::<Draft>(&saved)
    {
        say("done", "Loaded your earlier transcription", Some(1.0));
        return Ok(draft);
    }
    tokio::fs::create_dir_all(&dir).await.map_err(io)?;

    let (title, artist) = title_and_artist(&meta);
    let audio = dir.join("audio.wav");
    if !audio.is_file() {
        say("download", "Downloading audio…", Some(0.0));
        download_audio(&tools, url, &dir, |f| say("download", "Downloading audio…", Some(f))).await?;
    }

    let mut draft = Draft {
        video_id: id,
        url: url.to_string(),
        title,
        artist,
        method: String::new(),
        audio_path: audio.to_string_lossy().into(),
        vocals_path: None,
        device: None,
        lines: Vec::new(),
    };

    if let Some(lang) = uploaded_spanish_captions(&meta) {
        say("captions", "Using the Spanish captions uploaded with the video", None);
        draft.method = "youtube-subs".into();
        draft.lines = caption_lines(&tools, url, &dir, &lang).await?;
    } else {
        say("separate", "Separating vocals from the music…", Some(0.0));
        let vocals = separate_vocals(&tools, &audio, &dir, |f| say("separate", "Separating vocals from the music…", Some(f))).await?;
        say("transcribe", "Loading Whisper…", None);
        let prompt = format!("{}, {}.", draft.title, draft.artist);
        let duration = meta["duration"].as_f64().unwrap_or(0.0);
        let (segments, device) = whisper(&tools, &vocals, &dir, &prompt, |at| {
            let fraction = (duration > 0.0).then(|| (at / duration).min(1.0));
            say("transcribe", "Transcribing…", fraction);
        })
        .await?;
        draft.method = "whisper".into();
        draft.vocals_path = Some(vocals.to_string_lossy().into());
        draft.device = device;
        draft.lines = whisper_lines(segments);
    }

    if draft.lines.is_empty() {
        return Err(Error::Invalid("no sung words were found in that video".into()));
    }
    tokio::fs::write(&draft_path, serde_json::to_vec(&draft).expect("draft serializes")).await.map_err(io)?;
    say("done", "Ready to review", Some(1.0));
    Ok(draft)
}

fn io(e: std::io::Error) -> Error {
    Error::Invalid(format!("file error: {e}"))
}

async fn metadata(tools: &Tools, url: &str) -> Result<Value> {
    let mut cmd = Command::new(&tools.yt_dlp);
    cmd.args(["--no-playlist", "--dump-single-json", "--", url]);
    let out = run(cmd, |_, _| {}).await?;
    serde_json::from_str(&out).map_err(|e| Error::BadResponse(format!("yt-dlp metadata: {e}")))
}

/// Prefers YouTube Music's track/artist fields, then "Artist - Title" in the video title.
fn title_and_artist(meta: &Value) -> (String, String) {
    let field = |k: &str| meta[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    if let (Some(track), Some(artist)) = (field("track"), field("artist").or_else(|| field("creator"))) {
        return (track, artist.split(',').next().unwrap_or(&artist).trim().to_string());
    }
    let raw = field("title").unwrap_or_default();
    let cleaned = strip_video_noise(&raw);
    match cleaned.split_once(" - ").or_else(|| cleaned.split_once(" – ")) {
        Some((artist, title)) => (title.trim().to_string(), artist.trim().to_string()),
        None => (cleaned, field("uploader").unwrap_or_default().trim_end_matches(" - Topic").to_string()),
    }
}

/// Drops "(Official Video)", "[Lyrics]" and the like from a video title.
fn strip_video_noise(title: &str) -> String {
    const NOISE: &[&str] = &["official", "video", "audio", "lyric", "letra", "visualizer", "en vivo", "live", "hd", "4k", "oficial"];
    let mut out = String::new();
    let mut rest = title;
    while let Some(open) = rest.find(['(', '[']) {
        let close_char = if rest[open..].starts_with('(') { ')' } else { ']' };
        let Some(len) = rest[open..].find(close_char) else { break };
        let inner = rest[open + 1..open + len].to_lowercase();
        out.push_str(&rest[..open]);
        if !NOISE.iter().any(|n| inner.contains(n)) {
            out.push_str(&rest[open..=open + len]);
        }
        rest = &rest[open + len + 1..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The language key of uploaded (not automatic) Spanish captions, if any.
fn uploaded_spanish_captions(meta: &Value) -> Option<String> {
    meta["subtitles"]
        .as_object()?
        .keys()
        .find(|k| *k == "es" || k.starts_with("es-") || k.starts_with("es_"))
        .cloned()
}

/// YouTube now and then answers a download with 403 Forbidden; a fresh try usually works.
async fn download_audio(tools: &Tools, url: &str, dir: &Path, progress: impl Fn(f64)) -> Result<()> {
    let mut tries = 0;
    loop {
        let mut cmd = Command::new(&tools.yt_dlp);
        cmd.args(["--no-playlist", "--newline", "-x", "--audio-format", "wav", "-o"])
            .arg(dir.join("audio.%(ext)s"))
            .args(["--", url]);
        let result = run(cmd, |_, line| {
            if let Some(pct) = percent_in(line.strip_prefix("[download]").unwrap_or("")) {
                progress(pct);
            }
        })
        .await;
        match result {
            Err(Error::Invalid(msg)) if tries < 3 && msg.contains("HTTP Error 403") => {
                tries += 1;
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            other => return other.map(|_| ()),
        }
    }
}

async fn caption_lines(tools: &Tools, url: &str, dir: &Path, lang: &str) -> Result<Vec<DraftLine>> {
    let mut cmd = Command::new(&tools.yt_dlp);
    cmd.args(["--no-playlist", "--skip-download", "--write-subs", "--sub-format", "vtt", "--sub-langs", lang, "-o"])
        .arg(dir.join("captions.%(ext)s"))
        .args(["--", url]);
    run(cmd, |_, _| {}).await?;
    let path = dir.join(format!("captions.{lang}.vtt"));
    let vtt = tokio::fs::read_to_string(&path).await.map_err(io)?;
    Ok(parse_vtt(&vtt))
}

async fn separate_vocals(tools: &Tools, audio: &Path, dir: &Path, progress: impl Fn(f64)) -> Result<PathBuf> {
    let vocals = dir.join("sep/htdemucs/audio/vocals.wav");
    if vocals.is_file() {
        return Ok(vocals);
    }
    let attempt = |device: Option<&'static str>| {
        let mut cmd = Command::new(&tools.uv);
        cmd.args(["tool", "run", "--python", PYTHON, "--from", "demucs"])
            .args(DEMUCS_PINS)
            .args(["demucs", "--two-stems=vocals", "-n", "htdemucs", "-o"])
            .arg(dir.join("sep"));
        if let Some(device) = device {
            cmd.args(["-d", device]);
        }
        cmd.arg(audio);
        cmd
    };
    let on_line = |_: bool, line: &str| {
        if let Some(pct) = percent_in(line) {
            progress(pct);
        }
    };
    // The GPU is much faster, but a 4 GB card can run out of memory on long songs.
    if run(attempt(None), on_line).await.is_err() {
        run(attempt(Some("cpu")), on_line).await?;
    }
    if vocals.is_file() { Ok(vocals) } else { Err(Error::BadResponse("Demucs produced no vocals track".into())) }
}

#[derive(Debug, Deserialize)]
struct Segment {
    start: f64,
    end: f64,
    text: String,
    no_speech_prob: f64,
    words: Vec<SegmentWord>,
}

#[derive(Debug, Deserialize)]
struct SegmentWord {
    start: f64,
    end: f64,
    word: String,
    probability: f64,
}

async fn whisper(
    tools: &Tools,
    vocals: &Path,
    dir: &Path,
    prompt: &str,
    progress: impl Fn(f64),
) -> Result<(Vec<Segment>, Option<String>)> {
    let script = dir.join("transcribe.py");
    tokio::fs::write(&script, WHISPER_SCRIPT).await.map_err(io)?;

    let mut cmd = Command::new(&tools.uv);
    cmd.args(["run", "--no-project", "--python", PYTHON, "--with", "faster-whisper"])
        .args(WHISPER_CUDA_LIBS)
        .arg(&script)
        .arg(vocals)
        .args(["es", prompt]);

    let segments = std::sync::Mutex::new(Vec::new());
    let device = std::sync::Mutex::new(None);
    run(cmd, |is_stderr, line| {
        if is_stderr {
            return;
        }
        let Ok(event) = serde_json::from_str::<Value>(line) else { return };
        match event["event"].as_str() {
            Some("info") => *device.lock().unwrap() = event["device"].as_str().map(String::from),
            Some("reset") => segments.lock().unwrap().clear(),
            Some("segment") => {
                if let Ok(segment) = serde_json::from_value::<Segment>(event) {
                    progress(segment.end);
                    segments.lock().unwrap().push(segment);
                }
            }
            _ => {}
        }
    })
    .await?;
    Ok((segments.into_inner().unwrap(), device.into_inner().unwrap()))
}

fn whisper_lines(segments: Vec<Segment>) -> Vec<DraftLine> {
    let mut lines: Vec<DraftLine> = Vec::new();
    let mut stanza = 0;
    for s in segments {
        let text = s.text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        if lines.last().is_some_and(|prev| s.start - prev.end > STANZA_GAP_SECS) {
            stanza += 1;
        }
        let words: Vec<Word> = s
            .words
            .into_iter()
            .map(|w| Word { text: w.word.trim().to_string(), start: w.start, end: w.end, prob: w.probability })
            .filter(|w| !w.text.is_empty())
            .collect();
        let mut flags = Vec::new();
        if words.iter().any(|w| w.prob < UNSURE_BELOW) {
            flags.push("unsure".to_string());
        }
        if s.no_speech_prob > 0.6 {
            flags.push("no-speech".to_string());
        }
        if is_stock_phrase(&text) {
            flags.push("stock-phrase".to_string());
        }
        lines.push(DraftLine { text, start: s.start, end: s.end, stanza, words, flags });
    }
    flag_loops(&mut lines);
    lines
}

/// Phrases Whisper invents over music or silence, learned from subtitle credits in its training data.
fn is_stock_phrase(text: &str) -> bool {
    const STOCK: &[&str] = &[
        "amara.org",
        "subtítulos",
        "subtitulos",
        "subtitulado",
        "gracias por ver",
        "suscríbete",
        "suscribete",
        "dale like",
        "www.",
        ".com",
    ];
    let lower = text.to_lowercase();
    STOCK.iter().any(|s| lower.contains(s))
}

/// Four or more identical lines in a row is Whisper looping, not a chorus.
fn flag_loops(lines: &mut [DraftLine]) {
    let mut i = 0;
    while i < lines.len() {
        let mut j = i + 1;
        while j < lines.len() && lines[j].text.eq_ignore_ascii_case(&lines[i].text) {
            j += 1;
        }
        if j - i >= 4 {
            for line in &mut lines[i..j] {
                line.flags.push("loop".to_string());
            }
        }
        i = j;
    }
}

fn parse_vtt(vtt: &str) -> Vec<DraftLine> {
    let mut lines: Vec<DraftLine> = Vec::new();
    let mut stanza = 0;
    let mut timing: Option<(f64, f64)> = None;
    for raw in vtt.lines() {
        let raw = raw.trim();
        if let Some((a, b)) = raw.split_once("-->") {
            timing = Some((vtt_time(a), vtt_time(b.split_whitespace().next().unwrap_or(""))));
            continue;
        }
        let Some((start, end)) = timing else { continue };
        if raw.is_empty() {
            timing = None;
            continue;
        }
        let text = strip_tags(raw).replace(['♪', '♫'], "").trim().to_string();
        if text.is_empty() || lines.last().is_some_and(|l| l.text == text) {
            continue;
        }
        if lines.last().is_some_and(|prev| start - prev.end > STANZA_GAP_SECS) {
            stanza += 1;
        }
        lines.push(DraftLine { text, start, end, stanza, words: Vec::new(), flags: Vec::new() });
    }
    lines
}

fn vtt_time(s: &str) -> f64 {
    s.trim().split(':').fold(0.0, |acc, part| acc * 60.0 + part.replace(',', ".").parse::<f64>().unwrap_or(0.0))
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// The first "NN%" or "NN.N%" in a progress line, as a fraction.
fn percent_in(line: &str) -> Option<f64> {
    let at = line.find('%')?;
    let start = line[..at].rfind(|c: char| !(c.is_ascii_digit() || c == '.')).map_or(0, |i| i + 1);
    line[start..at].parse::<f64>().ok().map(|p| (p / 100.0).clamp(0.0, 1.0))
}

/// Runs a command, handing each stdout/stderr line (split on \n or \r) to `on_line`.
/// Returns stdout; on failure, the error carries the end of stderr.
async fn run(mut cmd: Command, mut on_line: impl FnMut(bool, &str)) -> Result<String> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let program = cmd.as_std().get_program().to_string_lossy().into_owned();
    let mut child = cmd.spawn().map_err(|e| Error::Invalid(format!("couldn't start {program}: {e}")))?;
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");

    let (mut out_all, mut out_pending, mut err_pending) = (Vec::new(), Vec::new(), Vec::new());
    let mut err_tail: Vec<String> = Vec::new();
    let (mut out_buf, mut err_buf) = ([0u8; 8192], [0u8; 8192]);
    let (mut out_open, mut err_open) = (true, true);
    while out_open || err_open {
        tokio::select! {
            n = stdout.read(&mut out_buf), if out_open => match n.map_err(io)? {
                0 => out_open = false,
                n => {
                    out_all.extend_from_slice(&out_buf[..n]);
                    out_pending.extend_from_slice(&out_buf[..n]);
                    drain_lines(&mut out_pending, |line| on_line(false, line));
                }
            },
            n = stderr.read(&mut err_buf), if err_open => match n.map_err(io)? {
                0 => err_open = false,
                n => {
                    err_pending.extend_from_slice(&err_buf[..n]);
                    drain_lines(&mut err_pending, |line| {
                        on_line(true, line);
                        err_tail.push(line.to_string());
                        if err_tail.len() > 12 { err_tail.remove(0); }
                    });
                }
            },
        }
    }
    let status = child.wait().await.map_err(io)?;
    if !status.success() {
        let name = Path::new(&program).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(program);
        return Err(Error::Invalid(format!("{name} failed: {}", err_tail.join("\n").trim())));
    }
    Ok(String::from_utf8_lossy(&out_all).into_owned())
}

fn drain_lines(pending: &mut Vec<u8>, mut each: impl FnMut(&str)) {
    while let Some(i) = pending.iter().position(|&b| b == b'\n' || b == b'\r') {
        let line: Vec<u8> = pending.drain(..=i).collect();
        let text = String::from_utf8_lossy(&line[..line.len() - 1]);
        if !text.trim().is_empty() {
            each(text.trim_end());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Runs the real pipeline: LETRAS_TEST_URL=... cargo test -p letras-core live_transcribe -- --ignored --nocapture
    /// Prints timings and counts only, never the transcribed text.
    #[tokio::test]
    #[ignore]
    async fn live_transcribe() {
        let url = std::env::var("LETRAS_TEST_URL").expect("set LETRAS_TEST_URL");
        let cache = std::env::var("LETRAS_TEST_CACHE").expect("set LETRAS_TEST_CACHE");
        let started = std::time::Instant::now();
        let last = std::sync::Mutex::new(String::new());
        let draft = transcribe(&url, Path::new(&cache), |p| {
            let mut last = last.lock().unwrap();
            if *last != p.stage {
                println!("[{:>5.1}s] stage {}", started.elapsed().as_secs_f64(), p.stage);
                *last = p.stage.to_string();
            }
        })
        .await
        .unwrap();
        let words: Vec<f64> = draft.lines.iter().flat_map(|l| l.words.iter().map(|w| w.prob)).collect();
        let count = |f: &str| draft.lines.iter().filter(|l| l.flags.iter().any(|x| x == f)).count();
        println!(
            "total {:.1}s | method {} | device {:?} | {} lines in {} stanzas | {} words, mean confidence {:.2}, {} unsure | flags: unsure {}, no-speech {}, stock-phrase {}, loop {}",
            started.elapsed().as_secs_f64(),
            draft.method,
            draft.device,
            draft.lines.len(),
            draft.lines.last().map_or(0, |l| l.stanza + 1),
            words.len(),
            words.iter().sum::<f64>() / words.len().max(1) as f64,
            words.iter().filter(|p| **p < UNSURE_BELOW).count(),
            count("unsure"),
            count("no-speech"),
            count("stock-phrase"),
            count("loop"),
        );
    }

    #[test]
    fn titles() {
        let m = json!({"title": "Grupo Ejemplo - Mi Canción (Video Oficial) [HD]", "uploader": "GrupoVEVO"});
        assert_eq!(title_and_artist(&m), ("Mi Canción".into(), "Grupo Ejemplo".into()));
        let m = json!({"title": "x", "track": "La Pista", "artist": "Uno, Dos"});
        assert_eq!(title_and_artist(&m), ("La Pista".into(), "Uno".into()));
        let m = json!({"title": "Canción (feat. Alguien)", "uploader": "Artista - Topic"});
        assert_eq!(title_and_artist(&m), ("Canción (feat. Alguien)".into(), "Artista".into()));
    }

    #[test]
    fn progress_percentages() {
        assert_eq!(percent_in(" 45.0% of 3.2MiB at 1MiB/s"), Some(0.45));
        assert_eq!(percent_in(" 37%|███▋      | 12/32"), Some(0.37));
        assert_eq!(percent_in("no number"), None);
    }

    #[test]
    fn vtt_cues_become_lines() {
        let vtt = "WEBVTT\nKind: captions\n\n00:00:01.000 --> 00:00:03.000\n♪ Primera <c>línea</c> ♪\n\n00:00:03.000 --> 00:00:05.000\nPrimera línea\n\n00:00:10.000 --> 00:00:12.000\nSegunda\n";
        let lines = parse_vtt(vtt);
        assert_eq!(lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["Primera línea", "Segunda"]);
        assert_eq!(lines[1].stanza, 1);
        assert_eq!(lines[1].start, 10.0);
    }

    #[test]
    fn whisper_flags() {
        let seg = |start: f64, text: &str, prob: f64| Segment {
            start,
            end: start + 2.0,
            text: text.into(),
            no_speech_prob: 0.0,
            words: vec![SegmentWord { start, end: start + 1.0, word: text.into(), probability: prob }],
        };
        let lines = whisper_lines(vec![
            seg(0.0, "hola", 0.9),
            seg(2.0, "quizá", 0.2),
            seg(10.0, "Subtítulos por la comunidad de Amara.org", 0.9),
            seg(12.0, "eh", 0.9),
            seg(14.0, "eh", 0.9),
            seg(16.0, "eh", 0.9),
            seg(18.0, "eh", 0.9),
        ]);
        assert!(lines[0].flags.is_empty());
        assert_eq!(lines[1].flags, ["unsure"]);
        assert_eq!(lines[2].stanza, 1);
        assert!(lines[2].flags.contains(&"stock-phrase".to_string()));
        assert!(lines[6].flags.contains(&"loop".to_string()));
    }
}
