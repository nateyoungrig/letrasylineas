//! Backend-only S256 PKCE with a short-lived loopback callback. No token reaches IPC.
use crate::{CmdResult, err};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use url::Url;

fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}
fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn authorization_url(callback: &str, verifier: &str, state: &str) -> Url {
    let mut url = Url::parse("https://openrouter.ai/auth").expect("static URL");
    url.query_pairs_mut().extend_pairs([
        ("callback_url", callback),
        ("code_challenge", &challenge(verifier)),
        ("code_challenge_method", "S256"),
        ("state", state),
        ("key_label", "Letras y Líneas"),
    ]);
    url
}

async fn open_browser(url: &Url) -> CmdResult<()> {
    #[cfg(target_os = "linux")]
    let mut cmd = tokio::process::Command::new("xdg-open");
    #[cfg(target_os = "macos")]
    let mut cmd = tokio::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut cmd = tokio::process::Command::new("rundll32.exe");
        cmd.arg("url.dll,FileProtocolHandler");
        cmd
    };
    let status = tokio::time::timeout(
        Duration::from_secs(15),
        cmd.arg(url.as_str())
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
    )
    .await
    .map_err(|_| "Browser launch timed out")?
    .map_err(err)?;
    if status.success() {
        Ok(())
    } else {
        Err("Could not open your browser".into())
    }
}

/// Returns None for unrelated/forged callbacks, so they don't consume the login.
fn callback_code(target: &str, state: &str) -> Option<String> {
    if !target.starts_with("/callback?") {
        return None;
    }
    let url = Url::parse(&format!("http://localhost{target}")).ok()?;
    let states: Vec<_> = url.query_pairs().filter(|(key, _)| key == "state").collect();
    let codes: Vec<_> = url.query_pairs().filter(|(key, _)| key == "code").collect();
    if states.len() != 1 || states[0].1 != state || codes.len() != 1 || codes[0].1.is_empty() {
        return None;
    }
    Some(codes[0].1.to_string())
}

pub async fn login() -> CmdResult<String> {
    tokio::time::timeout(Duration::from_secs(180), login_inner())
        .await
        .map_err(|_| "OpenRouter sign-in timed out; try again".to_string())?
}
async fn login_inner() -> CmdResult<String> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(err)?;
    let port = listener.local_addr().map_err(err)?.port();
    if port == 1420 {
        return Err("Could not allocate an authentication callback port".into());
    }
    let callback = format!("http://127.0.0.1:{port}/callback");
    let verifier = random_token();
    let state = random_token();
    open_browser(&authorization_url(&callback, &verifier, &state)).await?;
    let code = loop {
        let (mut socket, _) = listener.accept().await.map_err(err)?;
        let request = tokio::time::timeout(Duration::from_secs(3), async {
            let mut bytes = Vec::new();
            let mut buf = [0; 1024];
            while bytes.len() < 8192 && !bytes.windows(4).any(|b| b == b"\r\n\r\n") {
                let n = socket.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buf[..n]);
            }
            Ok::<_, std::io::Error>(bytes)
        })
        .await;
        let Ok(Ok(bytes)) = request else { continue };
        let request = String::from_utf8_lossy(&bytes);
        let mut line = request.lines().next().unwrap_or_default().split_whitespace();
        let code = if line.next() == Some("GET") {
            line.next().and_then(|target| callback_code(target, &state))
        } else {
            None
        };
        let (status, message) = if code.is_some() {
            (
                "200 OK",
                "Authorization received. Return to Letras y Líneas to check the connection.",
            )
        } else {
            (
                "400 Bad Request",
                "Invalid sign-in callback. Return to the app or complete the browser sign-in.",
            )
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{message}",
            message.len()
        );
        let _ = tokio::time::timeout(Duration::from_secs(2), socket.write_all(response.as_bytes())).await;
        if let Some(code) = code {
            break code;
        }
    };
    drop(listener);
    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(err)?
        .post("https://openrouter.ai/api/v1/auth/keys")
        .json(&serde_json::json!({"code": code, "code_verifier": verifier, "code_challenge_method": "S256"}))
        .send()
        .await
        .map_err(|_| "Could not reach OpenRouter to complete sign-in")?;
    if !response.status().is_success() {
        return Err("OpenRouter rejected the authorization. Start sign-in again".into());
    }
    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|_| "Invalid OpenRouter authorization response")?;
    payload["key"]
        .as_str()
        .filter(|key| !key.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "OpenRouter did not return a key".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pkce_uses_rfc7636_s256_vector() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let url = authorization_url("http://127.0.0.1:50000/callback", "verifier", "state");
        assert_eq!(url.host_str(), Some("openrouter.ai"));
        assert!(
            url.query_pairs()
                .any(|(k, v)| k == "code_challenge_method" && v == "S256")
        );
        assert!(!url.as_str().contains("verifier"));
    }
    #[test]
    fn callback_requires_matching_state_and_unique_code() {
        assert_eq!(
            callback_code("/callback?code=abc&state=secret", "secret"),
            Some("abc".into())
        );
        for target in [
            "/callback?code=abc",
            "/callback?code=abc&state=wrong",
            "/other?code=abc&state=secret",
            "/callback?code=a&code=b&state=secret",
            "/callback?code=&state=secret",
        ] {
            assert!(callback_code(target, "secret").is_none());
        }
    }
}
