use base64::Engine;
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::error::MicroClawError;

pub const OPENAI_CODEX_PROVIDER: &str = "openai-codex";

#[derive(Debug, Deserialize)]
struct CodexAuthFile {
    #[serde(rename = "OPENAI_API_KEY")]
    openai_api_key: Option<String>,
    tokens: Option<CodexAuthTokens>,
}

#[derive(Debug, Deserialize)]
struct CodexAuthTokens {
    access_token: Option<String>,
    #[serde(rename = "refresh_token")]
    _refresh_token: Option<String>,
    account_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CodexAuthResolved {
    pub bearer_token: String,
    pub account_id: Option<String>,
}

pub fn provider_allows_empty_api_key(provider: &str) -> bool {
    provider.eq_ignore_ascii_case("ollama") || provider.eq_ignore_ascii_case(OPENAI_CODEX_PROVIDER)
}

pub fn is_openai_codex_provider(provider: &str) -> bool {
    provider.eq_ignore_ascii_case(OPENAI_CODEX_PROVIDER)
}

pub fn default_codex_auth_path() -> PathBuf {
    let base = std::env::var("CODEX_HOME")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .as_deref()
        .map(expand_tilde)
        .unwrap_or_else(|| expand_tilde("~/.codex"));
    Path::new(&base).join("auth.json")
}

pub fn codex_auth_file_has_access_token() -> Result<bool, MicroClawError> {
    let path = default_codex_auth_path();
    if !path.exists() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(&path).map_err(|e| {
        MicroClawError::Config(format!(
            "Failed to read Codex auth file {}: {e}",
            path.display()
        ))
    })?;
    let parsed: CodexAuthFile = serde_json::from_str(&content).map_err(|e| {
        MicroClawError::Config(format!(
            "Failed to parse Codex auth file {}: {e}",
            path.display()
        ))
    })?;
    Ok(parsed
        .tokens
        .as_ref()
        .and_then(|tokens| tokens.access_token.as_ref())
        .map(|token| !token.trim().is_empty())
        .unwrap_or(false))
}

pub fn resolve_openai_codex_auth(
    fallback_api_key: &str,
) -> Result<CodexAuthResolved, MicroClawError> {
    if let Ok(token) = std::env::var("OPENAI_CODEX_ACCESS_TOKEN") {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return Ok(CodexAuthResolved {
                bearer_token: trimmed.to_string(),
                account_id: None,
            });
        }
    }

    let auth_path = default_codex_auth_path();
    if auth_path.exists() {
        let content = std::fs::read_to_string(&auth_path).map_err(|e| {
            MicroClawError::Config(format!(
                "Failed to read Codex auth file {}: {e}",
                auth_path.display()
            ))
        })?;
        let parsed: CodexAuthFile = serde_json::from_str(&content).map_err(|e| {
            MicroClawError::Config(format!(
                "Failed to parse Codex auth file {}: {e}",
                auth_path.display()
            ))
        })?;
        if let Some(token) = parsed
            .tokens
            .as_ref()
            .and_then(|tokens| tokens.access_token.as_ref())
            .map(|token| token.trim())
            .filter(|token| !token.is_empty())
        {
            return Ok(CodexAuthResolved {
                bearer_token: token.to_string(),
                account_id: parsed
                    .tokens
                    .as_ref()
                    .and_then(|tokens| tokens.account_id.clone())
                    .map(|id| id.trim().to_string())
                    .filter(|id| !id.is_empty()),
            });
        }

        if let Some(api_key) = parsed
            .openai_api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
        {
            return Ok(CodexAuthResolved {
                bearer_token: api_key.to_string(),
                account_id: parsed
                    .tokens
                    .as_ref()
                    .and_then(|tokens| tokens.account_id.clone())
                    .map(|id| id.trim().to_string())
                    .filter(|id| !id.is_empty()),
            });
        }
    }

    let fallback = fallback_api_key.trim();
    if !fallback.is_empty() {
        return Ok(CodexAuthResolved {
            bearer_token: fallback.to_string(),
            account_id: None,
        });
    }

    Err(MicroClawError::Config(format!(
        "OpenAI Codex provider requires OAuth. Run `codex login` (expected auth file: {}) or set OPENAI_CODEX_ACCESS_TOKEN.",
        auth_path.display()
    )))
}

fn expand_tilde(input: &str) -> String {
    if let Some(rest) = input.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    if input == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return home;
        }
    }
    input.to_string()
}

#[derive(Debug, Deserialize)]
struct CodexRefreshResponse {
    access_token: String,
    refresh_token: Option<String>,
}

pub fn refresh_openai_codex_auth_if_needed() -> Result<(), MicroClawError> {
    let auth_path = default_codex_auth_path();
    if !auth_path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&auth_path).map_err(|e| {
        MicroClawError::Config(format!(
            "Failed to read Codex auth file {}: {e}",
            auth_path.display()
        ))
    })?;
    let mut parsed: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        MicroClawError::Config(format!(
            "Failed to parse Codex auth file {}: {e}",
            auth_path.display()
        ))
    })?;

    let tokens = parsed
        .get("tokens")
        .and_then(|t| t.as_object())
        .cloned()
        .unwrap_or_default();
    let access = tokens
        .get("access_token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let refresh = tokens
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if access.is_empty() || refresh.is_empty() {
        return Ok(());
    }
    if !is_jwt_expired(&access) {
        return Ok(());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh,
        "client_id": "app_EMoamEEZ73f0CkXaXp7hrann",
    });
    let resp = client
        .post("https://auth.openai.com/oauth/token")
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()?;
    if !resp.status().is_success() {
        return Ok(());
    }
    let parsed_resp: CodexRefreshResponse = resp.json().map_err(|e| {
        MicroClawError::Config(format!(
            "Failed to parse OpenAI Codex refresh response: {e}"
        ))
    })?;
    if parsed_resp.access_token.trim().is_empty() {
        return Ok(());
    }

    if let Some(tokens_obj) = parsed.get_mut("tokens").and_then(|t| t.as_object_mut()) {
        tokens_obj.insert(
            "access_token".to_string(),
            serde_json::Value::String(parsed_resp.access_token),
        );
        if let Some(refresh_token) = parsed_resp.refresh_token {
            if !refresh_token.trim().is_empty() {
                tokens_obj.insert(
                    "refresh_token".to_string(),
                    serde_json::Value::String(refresh_token),
                );
            }
        }
    }
    parsed["last_refresh"] = serde_json::Value::String(chrono::Utc::now().to_rfc3339());
    std::fs::write(
        &auth_path,
        serde_json::to_string_pretty(&parsed).map_err(|e| {
            MicroClawError::Config(format!("Failed to serialize refreshed Codex auth: {e}"))
        })?,
    )?;
    Ok(())
}

fn is_jwt_expired(token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return false;
    }
    let mut payload = parts[1].to_string();
    while !payload.len().is_multiple_of(4) {
        payload.push('=');
    }
    let decoded = base64::engine::general_purpose::URL_SAFE
        .decode(payload.as_bytes())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let exp = decoded
        .as_ref()
        .and_then(|v| v.get("exp"))
        .and_then(|v| v.as_i64());
    match exp {
        Some(ts) => chrono::Utc::now().timestamp() >= ts,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_allows_empty_api_key() {
        assert!(provider_allows_empty_api_key("ollama"));
        assert!(provider_allows_empty_api_key("openai-codex"));
        assert!(!provider_allows_empty_api_key("openai"));
    }

    #[test]
    fn test_is_openai_codex_provider() {
        assert!(is_openai_codex_provider("openai-codex"));
        assert!(is_openai_codex_provider("OPENAI-CODEX"));
        assert!(!is_openai_codex_provider("openai"));
    }
}
