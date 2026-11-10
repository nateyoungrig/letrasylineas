//! Provider transports. Only tutoring requests enter this module, never lyric lookup.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Anthropic,
    Openai,
    Chatgpt,
    Openrouter,
}

impl Provider {
    pub fn default_model(self) -> &'static str {
        match self {
            Self::Anthropic => crate::claude::DEFAULT_MODEL,
            Self::Openai | Self::Chatgpt => "gpt-6.1-sol",
            Self::Openrouter => "openai/gpt-6.1-sol",
        }
    }
}

/// Shared prompt representation; provider-specific fields belong in the adapters.
pub(crate) struct Request {
    pub system: String,
    pub messages: Value,
    pub effort: &'static str,
    pub schema: Option<Value>,
}

pub(crate) struct Transport {
    provider: Provider,
    model: String,
    api_key: Option<String>,
    http: reqwest::Client,
}

pub fn validate_model(model: &str) -> Result<()> {
    if model.is_empty()
        || model.len() > 200
        || !model.chars().all(|c| c.is_ascii_alphanumeric() || "-._/:~".contains(c))
    {
        return Err(Error::Invalid("Enter a valid model ID".into()));
    }
    Ok(())
}

impl Transport {
    pub fn new(provider: Provider, model: String, api_key: Option<String>) -> Result<Self> {
        validate_model(&model)?;
        if provider != Provider::Chatgpt && api_key.as_ref().is_none_or(|k| k.trim().is_empty()) {
            return Err(Error::Unavailable("Connect this AI provider in Settings first".into()));
        }
        Ok(Self {
            provider,
            model,
            api_key,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(180))
                .build()?,
        })
    }

    pub async fn send(&self, request: &Request) -> Result<String> {
        if self.provider == Provider::Chatgpt {
            return crate::codex::send(&self.model, request).await;
        }
        let body = request_body(self.provider, &self.model, request);
        let key = self.api_key.as_deref().unwrap_or_default();
        let req = match self.provider {
            Provider::Anthropic => self
                .http
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01")
                .header("anthropic-beta", "server-side-fallback-2026-07-01"),
            Provider::Openai => self.http.post("https://api.openai.com/v1/responses").bearer_auth(key),
            Provider::Openrouter => self
                .http
                .post("https://openrouter.ai/api/v1/chat/completions")
                .bearer_auth(key)
                .header("X-Title", "Letras y Líneas"),
            Provider::Chatgpt => unreachable!(),
        };
        let response = req.json(&body).send().await?;
        let status = response.status();
        // Don't expose an upstream body: gateways sometimes echo request credentials.
        if !status.is_success() {
            let message = match status.as_u16() {
                401 | 403 => "Authentication failed; reconnect this provider in Settings",
                429 => "Usage limit reached; check your plan or credits and try later",
                400 | 404 | 422 => {
                    "The provider rejected this model or request; check the model's reasoning and structured-output support"
                }
                _ => "The provider is unavailable; try again later",
            };
            return Err(Error::Api {
                status: status.as_u16(),
                message: message.into(),
            });
        }
        response_text(self.provider, &response.json().await?)
    }
}

pub(crate) fn request_body(provider: Provider, model: &str, r: &Request) -> Value {
    match provider {
        Provider::Anthropic => {
            let mut body = json!({
                "model": model, "max_tokens": 16000, "thinking": {"type": "adaptive"},
                "output_config": {"effort": r.effort}, "fallbacks": "default",
                "system": r.system, "messages": r.messages,
            });
            if let Some(schema) = &r.schema {
                body["output_config"]["format"] = json!({"type": "json_schema", "schema": schema});
            }
            body
        }
        Provider::Openai => {
            let mut body = json!({
                "model": model, "max_output_tokens": 16000, "store": false,
                "reasoning": {"effort": r.effort}, "instructions": r.system, "input": r.messages,
            });
            if let Some(schema) = &r.schema {
                body["text"] = json!({"format": {"type": "json_schema", "name": "tutor_result", "strict": true, "schema": schema}});
            }
            body
        }
        Provider::Openrouter => {
            let mut messages = vec![json!({"role": "system", "content": r.system})];
            messages.extend(r.messages.as_array().into_iter().flatten().cloned());
            let mut body = json!({
                "model": model, "max_tokens": 16000, "messages": messages,
                "reasoning": {"effort": r.effort}, "provider": {"require_parameters": true},
            });
            if let Some(schema) = &r.schema {
                body["response_format"] = json!({"type": "json_schema", "json_schema": {"name": "tutor_result", "strict": true, "schema": schema}});
            }
            body
        }
        Provider::Chatgpt => unreachable!("Codex owns its wire protocol"),
    }
}

pub(crate) fn response_text(provider: Provider, payload: &Value) -> Result<String> {
    let text = match provider {
        Provider::Anthropic => {
            match payload["stop_reason"].as_str() {
                Some("refusal") => return Err(Error::Refusal),
                Some("max_tokens") => return Err(Error::Truncated),
                _ => {}
            }
            payload["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect::<String>()
        }
        Provider::Openai => {
            if payload["status"] == "incomplete" {
                return Err(if payload["incomplete_details"]["reason"] == "content_filter" {
                    Error::Refusal
                } else {
                    Error::Truncated
                });
            }
            if payload["status"] != "completed" {
                return Err(Error::BadResponse("response did not complete".into()));
            }
            let mut text = String::new();
            for item in payload["output"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|i| i["type"] == "message")
            {
                for part in item["content"].as_array().into_iter().flatten() {
                    if part["type"] == "refusal" {
                        return Err(Error::Refusal);
                    }
                    if part["type"] == "output_text" {
                        text.push_str(part["text"].as_str().unwrap_or_default());
                    }
                }
            }
            text
        }
        Provider::Openrouter => {
            let choice = &payload["choices"][0];
            match choice["finish_reason"].as_str() {
                Some("length") => return Err(Error::Truncated),
                Some("content_filter") => return Err(Error::Refusal),
                Some("stop") => {}
                _ => return Err(Error::BadResponse("response did not complete".into())),
            }
            if choice["message"]["refusal"].as_str().is_some_and(|s| !s.is_empty()) {
                return Err(Error::Refusal);
            }
            choice["message"]["content"].as_str().unwrap_or_default().to_string()
        }
        Provider::Chatgpt => unreachable!(),
    };
    if text.trim().is_empty() {
        return Err(Error::BadResponse("empty reply".into()));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(schema: bool) -> Request {
        Request {
            system: "tutor".into(),
            messages: json!([{"role":"user","content":"hola"},{"role":"assistant","content":"hello"},{"role":"user","content":"why?"}]),
            effort: "medium",
            schema: schema.then(crate::proofread::schema),
        }
    }
    #[test]
    fn anthropic_preserves_quality_settings() {
        let body = request_body(Provider::Anthropic, "custom", &request(true));
        assert_eq!(body["model"], "custom");
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(body["max_tokens"], 16000);
        assert_eq!(body["output_config"]["format"]["schema"], crate::proofread::schema());
        assert_eq!(response_text(Provider::Anthropic, &json!({"stop_reason":"end_turn","content":[{"type":"thinking","thinking":"hidden"},{"type":"text","text":"OK"}]})).unwrap(), "OK");
    }
    #[test]
    fn responses_strict_schema_and_conversation() {
        let body = request_body(Provider::Openai, "custom", &request(true));
        assert_eq!(body["text"]["format"]["strict"], true);
        assert_eq!(body["text"]["format"]["schema"], crate::proofread::schema());
        assert_eq!(body["reasoning"]["effort"], "medium");
        assert_eq!(body["input"][1]["role"], "assistant");
        assert_eq!(body["store"], false);
        let payload = json!({"status":"completed","output":[{"type":"reasoning","summary":[]},{"type":"message","content":[{"type":"output_text","text":"OK"}]}]});
        assert_eq!(response_text(Provider::Openai, &payload).unwrap(), "OK");
    }
    #[test]
    fn router_requires_supported_parameters() {
        let body = request_body(Provider::Openrouter, "vendor/model", &request(true));
        assert_eq!(body["provider"]["require_parameters"], true);
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][2]["role"], "assistant");
        assert_eq!(
            response_text(
                Provider::Openrouter,
                &json!({"choices":[{"finish_reason":"stop","message":{"content":"OK"}}]})
            )
            .unwrap(),
            "OK"
        );
    }
    #[test]
    fn low_effort_and_plain_text_for_conversations() {
        let mut r = request(false);
        r.effort = "low";
        let openai = request_body(Provider::Openai, "m", &r);
        assert_eq!(openai["reasoning"]["effort"], "low");
        assert!(openai.get("text").is_none());
        assert!(
            request_body(Provider::Anthropic, "m", &r)["output_config"]
                .get("format")
                .is_none()
        );
        assert!(
            request_body(Provider::Openrouter, "m", &r)
                .get("response_format")
                .is_none()
        );
    }
    #[test]
    fn failure_mapping() {
        assert!(matches!(
            response_text(Provider::Anthropic, &json!({"stop_reason":"refusal"})),
            Err(Error::Refusal)
        ));
        assert!(matches!(
            response_text(Provider::Anthropic, &json!({"stop_reason":"max_tokens"})),
            Err(Error::Truncated)
        ));
        assert!(matches!(
            response_text(
                Provider::Openai,
                &json!({"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"}})
            ),
            Err(Error::Truncated)
        ));
        assert!(matches!(
            response_text(
                Provider::Openai,
                &json!({"status":"completed","output":[{"type":"message","content":[{"type":"refusal"}]}]})
            ),
            Err(Error::Refusal)
        ));
        assert!(matches!(
            response_text(Provider::Openrouter, &json!({"choices":[{"finish_reason":"length"}]})),
            Err(Error::Truncated)
        ));
        for provider in [Provider::Anthropic, Provider::Openai, Provider::Openrouter] {
            assert!(response_text(provider, &json!({})).is_err());
        }
    }
}
