//! Non-secret AI preferences. API keys and OAuth tokens never belong here.
use crate::{
    Error, Result,
    provider::{Provider, validate_model},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub const PROVIDERS: [Provider; 4] = [
    Provider::Anthropic,
    Provider::Openai,
    Provider::Chatgpt,
    Provider::Openrouter,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    pub provider: Provider,
    pub models: BTreeMap<Provider, String>,
}
impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: Provider::Anthropic,
            models: PROVIDERS.into_iter().map(|p| (p, p.default_model().into())).collect(),
        }
    }
}
impl AiSettings {
    pub fn model(&self, provider: Provider) -> &str {
        self.models
            .get(&provider)
            .map(String::as_str)
            .unwrap_or_else(|| provider.default_model())
    }
}

pub struct SettingsStore {
    pub settings: AiSettings,
    path: PathBuf,
}
impl SettingsStore {
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join("ai-settings.json");
        let settings: AiSettings = match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|_| Error::Invalid("AI settings file is invalid".into()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => AiSettings::default(),
            Err(e) => return Err(e.into()),
        };
        for model in settings.models.values() {
            validate_model(model)?;
        }
        Ok(Self { settings, path })
    }
    pub fn save(&mut self, provider: Provider, model: String) -> Result<()> {
        validate_model(&model)?;
        let mut next = self.settings.clone();
        next.provider = provider;
        next.models.insert(provider, model);
        let mut file = tempfile::NamedTempFile::new_in(self.path.parent().expect("settings directory"))?;
        use std::io::Write;
        file.write_all(&serde_json::to_vec_pretty(&next).expect("settings serialize"))?;
        file.as_file().sync_all()?;
        file.persist(&self.path).map_err(|e| Error::Io(e.error))?;
        self.settings = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_model_choices_survive_switching_and_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SettingsStore::load(dir.path()).unwrap();
        assert_eq!(store.settings.provider, Provider::Anthropic);
        store.save(Provider::Openai, "my-model".into()).unwrap();
        store.save(Provider::Chatgpt, "another-model".into()).unwrap();
        let store = SettingsStore::load(dir.path()).unwrap();
        assert_eq!(store.settings.provider, Provider::Chatgpt);
        assert_eq!(store.settings.model(Provider::Openai), "my-model");
    }
    #[test]
    fn invalid_model_does_not_change_settings() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SettingsStore::load(dir.path()).unwrap();
        assert!(store.save(Provider::Chatgpt, "--config something".into()).is_err());
        assert_eq!(store.settings.provider, Provider::Anthropic);
        assert!(!dir.path().join("ai-settings.json").exists());
    }
}
