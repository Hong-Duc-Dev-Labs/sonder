//! OAuth2 PKCE authentication with loopback redirect (`http://127.0.0.1:<port>/callback`),
//! secure token persistence, and single-flight automatic access-token refresh.

use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::error::GdriveError;

/// Environment variable overriding/supplying the Google OAuth Desktop Client ID.
pub const ENV_CLIENT_ID: &str = "SONDER_GDRIVE_CLIENT_ID";
/// Optional environment variable supplying the Google OAuth Desktop Client Secret.
pub const ENV_CLIENT_SECRET: &str = "SONDER_GDRIVE_CLIENT_SECRET";

/// Default Google OAuth2 endpoints.
pub const GOOGLE_AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_REVOKE_ENDPOINT: &str = "https://oauth2.googleapis.com/revoke";
pub const DRIVE_READONLY_SCOPE: &str = "https://www.googleapis.com/auth/drive.readonly";

/// Refresh access tokens this many seconds before their nominal expiry.
const EXPIRY_SKEW_SECS: u64 = 60;
/// Maximum time to wait for the user to complete OAuth in their browser.
const AUTH_TIMEOUT: Duration = Duration::from_secs(300);

/// User-configured Google OAuth Desktop client credentials.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: Option<String>,
}

impl fmt::Debug for OAuthConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthConfig")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

impl OAuthConfig {
    /// Resolves effective OAuth credentials: explicit saved config takes precedence when non-empty,
    /// falling back to `SONDER_GDRIVE_CLIENT_ID` / `SONDER_GDRIVE_CLIENT_SECRET`.
    pub fn resolve(saved_client_id: Option<&str>, saved_client_secret: Option<&str>) -> Option<Self> {
        let saved_id = saved_client_id.map(str::trim).filter(|s| !s.is_empty());
        let saved_secret = saved_client_secret.map(str::trim).filter(|s| !s.is_empty());
        if let Some(id) = saved_id {
            return Some(Self {
                client_id: id.to_owned(),
                client_secret: saved_secret.map(str::to_owned),
            });
        }
        let env_id = std::env::var(ENV_CLIENT_ID)
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())?;
        let env_secret = std::env::var(ENV_CLIENT_SECRET)
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        Some(Self {
            client_id: env_id,
            client_secret: env_secret,
        })
    }

    pub fn env_client_id_present() -> bool {
        std::env::var(ENV_CLIENT_ID)
            .ok()
            .is_some_and(|s| !s.trim().is_empty())
    }
}

/// Persisted OAuth2 token bundle. Never printed in Debug output.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix timestamp in seconds when `access_token` expires.
    pub expires_at_unix: u64,
    pub account_email: Option<String>,
}

impl fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenSet")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("expires_at_unix", &self.expires_at_unix)
            .field("account_email", &self.account_email)
            .finish()
    }
}

impl TokenSet {
    pub fn is_expired(&self) -> bool {
        unix_now().saturating_add(EXPIRY_SKEW_SECS) >= self.expires_at_unix
    }
}

/// Public, non-sensitive summary of the current Google Drive authentication state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthStatus {
    pub configured: bool,
    pub authenticated: bool,
    pub account_email: Option<String>,
}

/// Non-public Google Drive configuration persisted separately from `settings.toml` with
/// restricted file permissions (`0o600` on Unix), never serialized to the webview.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedGdriveConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub cache_max_mb: Option<u32>,
}

impl fmt::Debug for PersistedGdriveConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PersistedGdriveConfig")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("cache_max_mb", &self.cache_max_mb)
            .finish()
    }
}

/// Persists [`PersistedGdriveConfig`] to a restricted-permission JSON file.
#[derive(Debug, Clone)]
pub struct OAuthConfigStore {
    path: PathBuf,
}

impl OAuthConfigStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load(&self) -> PersistedGdriveConfig {
        match fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => PersistedGdriveConfig::default(),
        }
    }

    pub fn save(&self, cfg: &PersistedGdriveConfig) -> Result<(), GdriveError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(cfg)?;
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp = self
            .path
            .with_extension(format!("json.{}.{seq}.tmp", std::process::id()));
        write_restricted(&tmp, json.as_bytes())?;
        if let Err(err) = fs::rename(&tmp, &self.path) {
            let _ = fs::remove_file(&tmp);
            return Err(GdriveError::Io(err));
        }
        Ok(())
    }
}

/// Persists [`TokenSet`] to a JSON file with restricted permissions.
#[derive(Debug, Clone)]
pub struct TokenStore {
    path: PathBuf,
}

impl TokenStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Option<TokenSet>, GdriveError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => {
                let tokens: TokenSet = serde_json::from_str(&text)?;
                Ok(Some(tokens))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(GdriveError::Io(e)),
        }
    }

    pub fn save(&self, tokens: &TokenSet) -> Result<(), GdriveError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(tokens)?;
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp = self
            .path
            .with_extension(format!("json.{}.{seq}.tmp", std::process::id()));
        write_restricted(&tmp, json.as_bytes())?;
        if let Err(err) = fs::rename(&tmp, &self.path) {
            let _ = fs::remove_file(&tmp);
            return Err(GdriveError::Io(err));
        }
        Ok(())
    }

    pub fn clear(&self) -> Result<(), GdriveError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(GdriveError::Io(e)),
        }
    }
}

fn write_restricted(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(bytes)?;
        f.sync_all()
    }
    #[cfg(not(unix))]
    {
        fs::write(path, bytes)
    }
}

/// PKCE verifier + S256 challenge pair.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn generate() -> Result<Self, GdriveError> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)
            .map_err(|e| GdriveError::AuthFailed(format!("OS randomness failed: {e}")))?;
        let verifier = URL_SAFE_NO_PAD.encode(bytes);
        let digest = Sha256::digest(verifier.as_bytes());
        let challenge = URL_SAFE_NO_PAD.encode(digest);
        Ok(Self {
            verifier,
            challenge,
        })
    }
}

fn random_state() -> Result<String, GdriveError> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes)
        .map_err(|e| GdriveError::AuthFailed(format!("OS randomness failed: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[derive(Debug, Deserialize)]
struct TokenEndpointResponse {
    access_token: String,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenErrorResponse {
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

struct AuthInner {
    config: Option<OAuthConfig>,
    tokens: Option<TokenSet>,
}

/// Thread-safe OAuth2 manager handling PKCE sign-in, token persistence, and single-flight refresh.
pub struct AuthManager {
    inner: Mutex<AuthInner>,
    store: TokenStore,
    agent: ureq::Agent,
    auth_endpoint: String,
    token_endpoint: String,
    revoke_endpoint: String,
    about_endpoint: String,
}

impl AuthManager {
    pub fn new(store: TokenStore, config: Option<OAuthConfig>, user_agent: &str) -> Self {
        let tokens = store.load().ok().flatten();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .user_agent(user_agent)
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            inner: Mutex::new(AuthInner { config, tokens }),
            store,
            agent,
            auth_endpoint: GOOGLE_AUTH_ENDPOINT.to_owned(),
            token_endpoint: GOOGLE_TOKEN_ENDPOINT.to_owned(),
            revoke_endpoint: GOOGLE_REVOKE_ENDPOINT.to_owned(),
            about_endpoint: "https://www.googleapis.com/drive/v3/about".to_owned(),
        }
    }

    /// Overrides OAuth endpoints (used in integration tests with a local mock server).
    pub fn with_endpoints(
        mut self,
        auth_endpoint: impl Into<String>,
        token_endpoint: impl Into<String>,
        revoke_endpoint: impl Into<String>,
        about_endpoint: impl Into<String>,
    ) -> Self {
        self.auth_endpoint = auth_endpoint.into();
        self.token_endpoint = token_endpoint.into();
        self.revoke_endpoint = revoke_endpoint.into();
        self.about_endpoint = about_endpoint.into();
        self
    }

    pub fn set_config(&self, config: Option<OAuthConfig>) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.config = config;
    }

    /// Injects a token set directly (primarily useful for tests).
    pub fn set_tokens(&self, tokens: TokenSet) -> Result<(), GdriveError> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.store.save(&tokens)?;
        inner.tokens = Some(tokens);
        Ok(())
    }

    pub fn status(&self) -> AuthStatus {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        AuthStatus {
            configured: inner.config.is_some(),
            authenticated: inner.tokens.is_some(),
            account_email: inner.tokens.as_ref().and_then(|t| t.account_email.clone()),
        }
    }

    /// Returns a valid access token, refreshing it automatically if expired.
    pub fn access_token(&self) -> Result<String, GdriveError> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tokens) = inner.tokens.as_ref() else {
            return Err(GdriveError::NotAuthenticated);
        };
        if !tokens.is_expired() {
            return Ok(tokens.access_token.clone());
        }
        self.refresh_locked(&mut inner)
    }

    /// Forces a token refresh after an HTTP 401 response.
    ///
    /// If another thread already refreshed the token (`failed_token` differs from the current
    /// token), returns the fresh token without issuing a duplicate refresh request.
    pub fn force_refresh(&self, failed_token: &str) -> Result<String, GdriveError> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(current) = inner.tokens.as_ref()
            && current.access_token != failed_token
            && !current.is_expired()
        {
            return Ok(current.access_token.clone());
        }
        self.refresh_locked(&mut inner)
    }

    fn refresh_locked(&self, inner: &mut AuthInner) -> Result<String, GdriveError> {
        let config = inner.config.clone().ok_or(GdriveError::NotConfigured)?;
        let existing = inner.tokens.clone().ok_or(GdriveError::NotAuthenticated)?;
        let refresh_token = existing
            .refresh_token
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or(GdriveError::Unauthorized)?;

        let mut form: Vec<(&str, &str)> = vec![
            ("client_id", config.client_id.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ];
        if let Some(secret) = config.client_secret.as_deref().filter(|s| !s.is_empty()) {
            form.push(("client_secret", secret));
        }

        let mut resp = self
            .agent
            .post(&self.token_endpoint)
            .send_form(form)
            .map_err(|e| GdriveError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        if status == 400 || status == 401 {
            // Refresh token revoked or invalid; clear persisted tokens so UI prompts sign-in.
            inner.tokens = None;
            let _ = self.store.clear();
            return Err(GdriveError::Unauthorized);
        }
        if !(200..300).contains(&status) {
            return Err(GdriveError::AuthFailed(format!(
                "token refresh returned HTTP {status}"
            )));
        }

        let parsed: TokenEndpointResponse = resp
            .body_mut()
            .read_json()
            .map_err(|e| GdriveError::Protocol(format!("invalid token refresh response: {e}")))?;
        let updated = TokenSet {
            access_token: parsed.access_token,
            refresh_token: parsed.refresh_token.or(existing.refresh_token),
            expires_at_unix: unix_now().saturating_add(parsed.expires_in.max(60)),
            account_email: existing.account_email,
        };
        let access = updated.access_token.clone();
        self.store.save(&updated)?;
        inner.tokens = Some(updated);
        Ok(access)
    }

    /// Runs the interactive OAuth2 PKCE loopback flow: binds `127.0.0.1:0`, opens the user's
    /// system browser, waits for the callback, exchanges the auth code, and persists tokens.
    pub fn sign_in_interactive(&self, cancel: Arc<AtomicBool>) -> Result<AuthStatus, GdriveError> {
        let config = {
            let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.config.clone().ok_or(GdriveError::NotConfigured)?
        };
        let pkce = Pkce::generate()?;
        let state = random_state()?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let redirect_uri = format!("http://127.0.0.1:{port}/callback");

        let mut auth_url = Url::parse(&self.auth_endpoint)
            .map_err(|e| GdriveError::AuthFailed(format!("invalid auth endpoint: {e}")))?;
        auth_url
            .query_pairs_mut()
            .append_pair("client_id", &config.client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", DRIVE_READONLY_SCOPE)
            .append_pair("code_challenge", &pkce.challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent");

        open_browser(auth_url.as_str())?;

        let code = wait_for_loopback_code(listener, &state, &cancel)?;
        let mut tokens = self.exchange_code(&config, &code, &redirect_uri, &pkce.verifier)?;
        tokens.account_email = self.fetch_account_email(&tokens.access_token).ok().flatten();
        self.set_tokens(tokens)?;
        Ok(self.status())
    }

    fn exchange_code(
        &self,
        config: &OAuthConfig,
        code: &str,
        redirect_uri: &str,
        verifier: &str,
    ) -> Result<TokenSet, GdriveError> {
        let mut form: Vec<(&str, &str)> = vec![
            ("client_id", config.client_id.as_str()),
            ("code", code),
            ("code_verifier", verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
        ];
        if let Some(secret) = config.client_secret.as_deref().filter(|s| !s.is_empty()) {
            form.push(("client_secret", secret));
        }

        let mut resp = self
            .agent
            .post(&self.token_endpoint)
            .send_form(form)
            .map_err(|e| GdriveError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let detail = resp
                .body_mut()
                .read_json::<TokenErrorResponse>()
                .ok()
                .map(|e| {
                    e.error_description
                        .filter(|d| !d.is_empty())
                        .unwrap_or(e.error)
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("HTTP {status}"));
            return Err(GdriveError::AuthFailed(detail));
        }

        let parsed: TokenEndpointResponse = resp
            .body_mut()
            .read_json()
            .map_err(|e| GdriveError::Protocol(format!("invalid token response: {e}")))?;
        Ok(TokenSet {
            access_token: parsed.access_token,
            refresh_token: parsed.refresh_token,
            expires_at_unix: unix_now().saturating_add(parsed.expires_in.max(60)),
            account_email: None,
        })
    }

    fn fetch_account_email(&self, access_token: &str) -> Result<Option<String>, GdriveError> {
        #[derive(Deserialize)]
        struct AboutResp {
            user: Option<AboutUser>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct AboutUser {
            email_address: Option<String>,
        }
        let url = format!("{}?fields=user(emailAddress)", self.about_endpoint);
        let mut resp = self
            .agent
            .get(&url)
            .header("Authorization", &format!("Bearer {access_token}"))
            .call()
            .map_err(|e| GdriveError::Http(e.to_string()))?;
        if !(200..300).contains(&resp.status().as_u16()) {
            return Ok(None);
        }
        let about: AboutResp = resp
            .body_mut()
            .read_json()
            .map_err(|e| GdriveError::Protocol(e.to_string()))?;
        Ok(about.user.and_then(|u| u.email_address))
    }

    /// Revokes the stored token (best-effort) and clears persisted credentials.
    pub fn disconnect(&self) -> Result<(), GdriveError> {
        let token_to_revoke = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let prev = inner.tokens.take();
            prev.and_then(|t| t.refresh_token.or(Some(t.access_token)))
        };
        self.store.clear()?;
        if let Some(token) = token_to_revoke {
            let _ = self
                .agent
                .post(&self.revoke_endpoint)
                .send_form([("token", token.as_str())]);
        }
        Ok(())
    }
}

fn wait_for_loopback_code(
    listener: TcpListener,
    expected_state: &str,
    cancel: &AtomicBool,
) -> Result<String, GdriveError> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + AUTH_TIMEOUT;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(GdriveError::AuthCancelled);
        }
        if Instant::now() >= deadline {
            return Err(GdriveError::AuthFailed(
                "timed out waiting for Google sign-in".into(),
            ));
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut request_line = String::new();
                {
                    let mut reader = BufReader::new(&stream);
                    if reader.read_line(&mut request_line).is_err() || request_line.trim().is_empty() {
                        // Browser speculative preconnect or closed socket; keep waiting.
                        continue;
                    }
                    // Drain remaining HTTP request headers so browsers do not see a TCP RST.
                    let mut header = String::new();
                    for _ in 0..64 {
                        header.clear();
                        match reader.read_line(&mut header) {
                            Ok(0) | Err(_) => break,
                            Ok(_) if header == "\r\n" || header == "\n" => break,
                            Ok(_) => {}
                        }
                    }
                }
                let path = request_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/");
                let Ok(parsed) = Url::parse(&format!("http://127.0.0.1{path}")) else {
                    let _ = write_http_response(&mut stream, "400 Bad Request", "Invalid request.");
                    continue;
                };
                if parsed.path() != "/callback" {
                    let _ = write_http_response(&mut stream, "404 Not Found", "Not found");
                    continue;
                }
                let mut code = None;
                let mut state = None;
                let mut error = None;
                for (k, v) in parsed.query_pairs() {
                    match k.as_ref() {
                        "code" => code = Some(v.into_owned()),
                        "state" => state = Some(v.into_owned()),
                        "error" => error = Some(v.into_owned()),
                        _ => {}
                    }
                }
                if let Some(err) = error {
                    let _ = write_http_response(
                        &mut stream,
                        "200 OK",
                        "Sign-in cancelled. You can close this tab and return to Sonder.",
                    );
                    return Err(GdriveError::AuthFailed(format!("Google returned: {err}")));
                }
                if state.as_deref() != Some(expected_state) {
                    let _ = write_http_response(
                        &mut stream,
                        "400 Bad Request",
                        "State mismatch. Please return to Sonder.",
                    );
                    // Ignore stray/CSRF requests without aborting the user's in-progress login.
                    continue;
                }
                let Some(code) = code.filter(|c| !c.is_empty()) else {
                    let _ = write_http_response(
                        &mut stream,
                        "400 Bad Request",
                        "Missing authorization code.",
                    );
                    continue;
                };
                let _ = write_http_response(
                    &mut stream,
                    "200 OK",
                    "Connected to Google Drive. You can close this tab and return to Sonder.",
                );
                return Ok(code);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(GdriveError::Io(e)),
        }
    }
}

fn write_http_response(stream: &mut impl Write, status: &str, message: &str) -> std::io::Result<()> {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Sonder — Google Drive</title>\
         <style>body{{background:#000;color:#f4f4f6;font-family:system-ui,sans-serif;display:grid;\
         place-items:center;min-height:100vh;margin:0;}}main{{padding:28px 32px;border-radius:16px;\
         background:rgba(255,255,255,0.06);border:1px solid rgba(255,255,255,0.08);max-width:420px;\
         text-align:center;}}h1{{font-size:18px;margin:0 0 8px;}}p{{font-size:14px;color:#b8b8c0;\
         margin:0;line-height:1.5;}}</style></head><body><main><h1>Sonder</h1><p>{message}</p></main></body></html>"
    );
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

fn open_browser(url: &str) -> Result<(), GdriveError> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()
            .map_err(|e| GdriveError::AuthFailed(format!("cannot open browser: {e}")))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| GdriveError::AuthFailed(format!("cannot open browser: {e}")))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| GdriveError::AuthFailed(format!("cannot open browser: {e}")))?;
    }
    Ok(())
}

pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_generates_valid_s256_challenge_and_debug_redacts_secrets() {
        let pkce = Pkce::generate().unwrap();
        assert!(pkce.verifier.len() >= 43);
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(pkce.verifier.as_bytes()));
        assert_eq!(pkce.challenge, expected);

        let cfg = OAuthConfig {
            client_id: "id-123".into(),
            client_secret: Some("super-secret-value".into()),
        };
        let dbg = format!("{cfg:?}");
        assert!(!dbg.contains("super-secret-value"));
        assert!(dbg.contains("<redacted>"));

        let tokens = TokenSet {
            access_token: "ya29.secret-access".into(),
            refresh_token: Some("1//secret-refresh".into()),
            expires_at_unix: unix_now() + 3600,
            account_email: Some("user@example.com".into()),
        };
        let tdbg = format!("{tokens:?}");
        assert!(!tdbg.contains("ya29.secret-access"));
        assert!(!tdbg.contains("1//secret-refresh"));
    }

    #[test]
    fn token_store_round_trips_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path().join("tokens.json"));
        assert_eq!(store.load().unwrap(), None);
        let tokens = TokenSet {
            access_token: "acc".into(),
            refresh_token: Some("ref".into()),
            expires_at_unix: 9_999_999_999,
            account_email: Some("a@b.c".into()),
        };
        store.save(&tokens).unwrap();
        assert_eq!(store.load().unwrap(), Some(tokens));
        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
    }
}
