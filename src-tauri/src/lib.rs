//! Tauri shell: thin commands over `letras_core`. The Anthropic API key lives in the
//! OS keyring (or `ANTHROPIC_API_KEY` for development) and never reaches the webview.

use std::sync::{Arc, Mutex, MutexGuard};

use letras_core::models::{Attempt, EditPlan, ReviewItem, Song, SongSummary, Stats, ThreadMessage, TutorMessage};
use letras_core::lookup::{self, LookupResult};
use letras_core::proofread::{ChatTurn, Proofread, Suggestion};
use letras_core::transcribe::{self, Draft, DraftLine};
use letras_core::{Claude, Db, User, Users, service};
use tauri::{AppHandle, Emitter, Manager, State};

const KEYRING_SERVICE: &str = "letrasylineas";
const KEYRING_USER: &str = "anthropic-api-key";

struct AppState {
    users: Mutex<Users>,
    /// The current user's database. Commands take a clone of the Arc, so a grade still in
    /// flight when the user switches is saved for the user who submitted it.
    db: Mutex<Arc<Mutex<Db>>>,
}

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn current_db(state: &State<'_, AppState>) -> Arc<Mutex<Db>> {
    lock(&state.db).clone()
}

fn with_db<T>(state: &State<'_, AppState>, f: impl FnOnce(&mut Db) -> letras_core::Result<T>) -> CmdResult<T> {
    f(&mut lock(&current_db(state))).map_err(err)
}

fn key_entry() -> CmdResult<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(err)
}

fn api_key() -> Option<String> {
    key_entry()
        .ok()
        .and_then(|e| e.get_password().ok())
        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
        .filter(|k| !k.trim().is_empty())
}

fn claude() -> CmdResult<Claude> {
    api_key().map(Claude::new).ok_or_else(|| "Add your Anthropic API key in Settings first".to_string())
}

#[tauri::command]
fn has_api_key() -> bool {
    api_key().is_some()
}

#[tauri::command]
fn set_api_key(key: String) -> CmdResult<()> {
    let entry = key_entry()?;
    let key = key.trim();
    if key.is_empty() {
        return match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(err(e)),
        };
    }
    entry.set_password(key).map_err(err)
}

#[derive(serde::Serialize)]
struct UserList {
    current: String,
    users: Vec<User>,
}

fn user_list(users: &Users) -> UserList {
    UserList { current: users.current().id.clone(), users: users.list().to_vec() }
}

#[tauri::command]
fn list_users(state: State<'_, AppState>) -> UserList {
    user_list(&lock(&state.users))
}

#[tauri::command]
fn switch_user(state: State<'_, AppState>, id: String) -> CmdResult<UserList> {
    let mut users = lock(&state.users);
    let db = users.switch(&id).map_err(err)?;
    *lock(&state.db) = Arc::new(Mutex::new(db));
    Ok(user_list(&users))
}

#[tauri::command]
fn list_songs(state: State<'_, AppState>) -> CmdResult<Vec<SongSummary>> {
    with_db(&state, |db| db.list_songs())
}

#[tauri::command]
fn create_song(
    state: State<'_, AppState>,
    title: String,
    artist: String,
    lyrics: String,
    source: Option<String>,
) -> CmdResult<i64> {
    let source = source.unwrap_or_else(|| "pasted".to_string());
    if !valid_source(&source) {
        return Err(format!("unknown lyrics source {source:?}"));
    }
    with_db(&state, |db| db.create_song(&title, &artist, &lyrics, &source))
}

/// pasted, or kind:id where id is an LRCLIB number or a YouTube video id.
fn valid_source(source: &str) -> bool {
    match source.split_once(':') {
        None => source == "pasted",
        Some((kind, id)) => {
            matches!(kind, "lrclib" | "youtube-subs" | "whisper")
                && !id.is_empty()
                && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        }
    }
}

#[tauri::command]
async fn transcribe_youtube(app: AppHandle, url: String) -> CmdResult<Draft> {
    let cache = app.path().app_cache_dir().map_err(err)?;
    transcribe::transcribe(url.trim(), &cache, |p| {
        let _ = app.emit("transcribe-progress", p);
    })
    .await
    .map_err(err)
}

/// Appends a line from the UI to <app log dir>/client.log, for diagnosing webview issues.
#[tauri::command]
fn log_client(app: AppHandle, message: String) -> CmdResult<()> {
    use std::io::Write;
    let dir = app.path().app_log_dir().map_err(err)?;
    std::fs::create_dir_all(&dir).map_err(err)?;
    let path = dir.join("client.log");
    // Start over past 1 MB so the log can't grow without bound.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1_000_000) {
        let _ = std::fs::remove_file(&path);
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(err)?;
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    writeln!(file, "{secs} {}", message.replace('\n', " ")).map_err(err)
}

/// The bytes of a downloaded audio file, for the webview to play from a blob URL.
/// WebKitGTK's media player won't load from the `asset://` protocol, so audio goes over IPC.
#[tauri::command]
async fn read_audio(app: AppHandle, path: String) -> CmdResult<tauri::ipc::Response> {
    let root = app.path().app_cache_dir().map_err(err)?.join("transcribe");
    let root = std::fs::canonicalize(root).map_err(err)?;
    let path = std::fs::canonicalize(&path).map_err(err)?;
    if !path.starts_with(&root) {
        return Err("that file isn't one the app downloaded".into());
    }
    let bytes = tauri::async_runtime::spawn_blocking(move || std::fs::read(path)).await.map_err(err)?.map_err(err)?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[derive(serde::Serialize)]
struct AudioSupport {
    ok: bool,
    missing: Vec<String>,
}

/// On Linux the webview plays audio through GStreamer, and WebKit aborts the whole
/// page (rather than failing the <audio> element) when an element it needs is absent.
/// Both of these come in gst-plugins-good.
#[tauri::command]
fn audio_support() -> AudioSupport {
    if !cfg!(target_os = "linux") {
        return AudioSupport { ok: true, missing: vec![] };
    }
    let missing: Vec<String> = ["autoaudiosink", "wavparse"]
        .into_iter()
        .filter(|element| {
            // If gst-inspect itself is missing we can't tell, so don't block playback.
            std::process::Command::new("gst-inspect-1.0")
                .args(["--exists", element])
                .status()
                .is_ok_and(|status| !status.success())
        })
        .map(String::from)
        .collect();
    AudioSupport { ok: missing.is_empty(), missing }
}

#[tauri::command]
async fn proofread_transcript(title: String, artist: String, lines: Vec<DraftLine>) -> CmdResult<Proofread> {
    claude()?.proofread(&title, &artist, &lines).await.map_err(err)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn discuss_line(
    title: String,
    artist: String,
    lines: Vec<DraftLine>,
    line: usize,
    suggestions: Vec<Suggestion>,
    suspicious: Option<String>,
    history: Vec<ChatTurn>,
    question: String,
) -> CmdResult<String> {
    if line >= lines.len() {
        return Err("that line isn't in the transcript".into());
    }
    if history.first().is_some_and(|m| m.role != "user") {
        return Err("a discussion has to start with your message".into());
    }
    claude()?
        .discuss(&title, &artist, &lines, line, &suggestions, suspicious.as_deref(), &history, &question)
        .await
        .map_err(err)
}

#[tauri::command]
async fn lookup_lyrics(track: String, artist: String) -> CmdResult<LookupResult> {
    lookup::search(&track, &artist).await.map_err(err)
}

#[tauri::command]
fn get_song(state: State<'_, AppState>, id: i64) -> CmdResult<Song> {
    with_db(&state, |db| db.get_song(id))
}

/// With `apply` false, only reports what saving the edited lyrics would change.
#[tauri::command]
fn update_song(
    state: State<'_, AppState>,
    id: i64,
    title: String,
    artist: String,
    lyrics: String,
    apply: bool,
) -> CmdResult<EditPlan> {
    with_db(&state, |db| db.update_song(id, &title, &artist, &lyrics, apply))
}

#[tauri::command]
fn edit_line(state: State<'_, AppState>, line_id: i64, text: String) -> CmdResult<()> {
    with_db(&state, |db| db.edit_line(line_id, &text))
}

#[tauri::command]
fn delete_song(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    with_db(&state, |db| db.delete_song(id))
}

#[tauri::command]
async fn submit_translation(
    state: State<'_, AppState>,
    line_id: i64,
    translation: String,
    source: String,
) -> CmdResult<Attempt> {
    let source = if source == "repaso" { "repaso" } else { "board" };
    service::grade_line(&current_db(&state), &claude()?, line_id, &translation, source).await.map_err(err)
}

#[tauri::command]
async fn regrade_attempt(state: State<'_, AppState>, attempt_id: i64) -> CmdResult<Attempt> {
    service::regrade(&current_db(&state), &claude()?, attempt_id).await.map_err(err)
}

#[tauri::command]
fn get_thread(state: State<'_, AppState>, attempt_id: i64) -> CmdResult<Vec<ThreadMessage>> {
    with_db(&state, |db| db.thread(attempt_id))
}

#[tauri::command]
async fn ask_followup(state: State<'_, AppState>, attempt_id: i64, question: String) -> CmdResult<Vec<ThreadMessage>> {
    service::ask(&current_db(&state), &claude()?, attempt_id, &question).await.map_err(err)
}

#[tauri::command]
fn tutor_thread(state: State<'_, AppState>) -> CmdResult<Vec<TutorMessage>> {
    with_db(&state, |db| db.tutor_thread(200))
}

#[tauri::command]
async fn ask_tutor(state: State<'_, AppState>, question: String) -> CmdResult<Vec<TutorMessage>> {
    service::ask_tutor(&current_db(&state), &claude()?, &question).await.map_err(err)
}

#[tauri::command]
fn clear_tutor(state: State<'_, AppState>) -> CmdResult<()> {
    with_db(&state, |db| db.clear_tutor())
}

#[tauri::command]
fn get_stats(state: State<'_, AppState>) -> CmdResult<Stats> {
    with_db(&state, |db| db.stats())
}

#[tauri::command]
fn review_queue(state: State<'_, AppState>, tag: Option<String>) -> CmdResult<Vec<ReviewItem>> {
    with_db(&state, |db| db.review_queue(tag.as_deref(), 20))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let users = Users::load(&dir)?;
            let db = users.open_current()?;
            app.manage(AppState { users: Mutex::new(users), db: Mutex::new(Arc::new(Mutex::new(db))) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            has_api_key,
            set_api_key,
            list_users,
            switch_user,
            list_songs,
            create_song,
            lookup_lyrics,
            transcribe_youtube,
            proofread_transcript,
            audio_support,
            log_client,
            read_audio,
            discuss_line,
            get_song,
            delete_song,
            update_song,
            edit_line,
            submit_translation,
            regrade_attempt,
            get_thread,
            ask_followup,
            get_stats,
            tutor_thread,
            ask_tutor,
            clear_tutor,
            review_queue,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
