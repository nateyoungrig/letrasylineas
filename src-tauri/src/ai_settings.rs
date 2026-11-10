//! AI configuration and credentials stay in the native backend.
use crate::{CmdResult, err, lock};
use letras_core::{
    Ai,
    provider::Provider,
    settings::{AiSettings, PROVIDERS, SettingsStore},
};
use std::sync::Mutex;
use tauri::State;

pub struct AiState {
    pub store: Mutex<SettingsStore>,
    // Serializes login/logout so a late callback can't undo a disconnect.
    auth: tokio::sync::Mutex<()>,
    cancel: tokio::sync::Notify,
}
impl AiState {
    pub fn new(store: SettingsStore) -> Self {
        Self {
            store: Mutex::new(store),
            auth: tokio::sync::Mutex::new(()),
            cancel: tokio::sync::Notify::new(),
        }
    }
}

fn entry(provider: Provider) -> CmdResult<keyring::Entry> {
    let name = match provider {
        Provider::Anthropic => "anthropic-api-key", // Preserve the existing credential.
        Provider::Openai => "openai-api-key",
        Provider::Openrouter => "openrouter-api-key",
        Provider::Chatgpt => return Err("ChatGPT credentials are managed by Codex CLI".into()),
    };
    keyring::Entry::new("letrasylineas", name).map_err(err)
}
fn env_name(provider: Provider) -> Option<&'static str> {
    match provider {
        Provider::Anthropic => Some("ANTHROPIC_API_KEY"),
        Provider::Openai => Some("OPENAI_API_KEY"),
        Provider::Openrouter => Some("OPENROUTER_API_KEY"),
        Provider::Chatgpt => None,
    }
}
fn credential(provider: Provider) -> CmdResult<Option<(String, &'static str)>> {
    let saved = entry(provider).and_then(|entry| match entry.get_password() {
        Ok(key) => Ok(Some(key).filter(|key| !key.trim().is_empty())),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("Cannot read the OS keyring; unlock it and try again".into()),
    });
    if let Ok(Some(key)) = &saved {
        return Ok(Some((key.clone(), "keyring")));
    }
    // Preserve the development environment fallback even on machines without a keyring.
    if let Some(key) = env_name(provider)
        .and_then(|name| std::env::var(name).ok())
        .filter(|key| !key.trim().is_empty())
    {
        return Ok(Some((key, "environment")));
    }
    saved.map(|_| None)
}

fn store_key(provider: Provider, key: &str) -> CmdResult<()> {
    let entry = entry(provider)?;
    if key.trim().is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(err(e)),
        }
    } else {
        entry.set_password(key.trim()).map_err(err)
    }
}

pub fn client(state: &AiState) -> CmdResult<Ai> {
    let settings = lock(&state.store).settings.clone();
    let key = if settings.provider == Provider::Chatgpt {
        None
    } else {
        credential(settings.provider)?.map(|(key, _)| key)
    };
    Ai::new(settings.provider, settings.model(settings.provider).into(), key).map_err(err)
}

#[derive(serde::Serialize)]
pub struct Connection {
    provider: Provider,
    connected: bool,
    source: Option<&'static str>,
    error: Option<String>,
}
#[derive(serde::Serialize)]
pub struct AiStatus {
    settings: AiSettings,
    connections: Vec<Connection>,
}
#[tauri::command]
pub async fn ai_status(state: State<'_, AiState>) -> CmdResult<AiStatus> {
    let settings = lock(&state.store).settings.clone();
    let mut connections = Vec::new();
    for provider in PROVIDERS {
        let result = if provider == Provider::Chatgpt {
            letras_core::codex::logged_in()
                .await
                .map(|connected| connected.then_some("codex"))
                .map_err(err)
        } else {
            credential(provider).map(|key| key.map(|(_, source)| source))
        };
        let (source, error) = match result {
            Ok(source) => (source, None),
            Err(error) => (None, Some(error)),
        };
        connections.push(Connection {
            provider,
            connected: source.is_some(),
            source,
            error,
        });
    }
    Ok(AiStatus { settings, connections })
}
#[tauri::command]
pub fn save_ai_settings(state: State<'_, AiState>, provider: Provider, model: String) -> CmdResult<()> {
    lock(&state.store).save(provider, model.trim().into()).map_err(err)
}
#[tauri::command]
pub async fn set_provider_key(state: State<'_, AiState>, provider: Provider, key: String) -> CmdResult<()> {
    let _guard = state
        .auth
        .try_lock()
        .map_err(|_| "Finish or cancel the current sign-in first")?;
    store_key(provider, &key)
}
#[tauri::command]
pub async fn login_provider(state: State<'_, AiState>, provider: Provider) -> CmdResult<()> {
    let _guard = state.auth.try_lock().map_err(|_| "A sign-in is already in progress")?;
    let login = async {
        match provider {
            Provider::Chatgpt => letras_core::codex::login().await.map_err(err),
            Provider::Openrouter => {
                let key = crate::openrouter_auth::login().await?;
                store_key(provider, &key)
            }
            _ => Err("Enter an API key for this provider".into()),
        }
    };
    // register cancellation before starting the browser flow
    tokio::select! {
        biased;
        _ = state.cancel.notified() => Err("Sign-in cancelled".into()),
        result = login => result,
    }
}
#[tauri::command]
pub fn cancel_ai_login(state: State<'_, AiState>) {
    state.cancel.notify_waiters();
}
#[tauri::command]
pub async fn logout_provider(state: State<'_, AiState>, provider: Provider) -> CmdResult<()> {
    let _guard = state
        .auth
        .try_lock()
        .map_err(|_| "Finish or cancel the current sign-in first")?;
    if provider == Provider::Chatgpt {
        letras_core::codex::logout().await.map_err(err)
    } else {
        store_key(provider, "")
    }
}
