//! Google sign-in shared by Calendar and Gmail (build plan 8.4): loopback redirect + PKCE,
//! refresh token in the Keychain, access token in memory only.
pub mod loopback;
#[cfg(test)]
pub mod mock;
pub mod secrets;

use crate::config::GoogleConfig;
use crate::connectors::ConnectorStatus;
use crate::opener::Opener;
use secrets::Secrets;
use std::sync::{Arc, Mutex};
use oauth2::basic::{BasicClient, BasicErrorResponse, BasicErrorResponseType};
use oauth2::{AuthType, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet, PkceCodeChallenge, RedirectUrl, RefreshToken, RequestTokenError, Scope, TokenResponse, TokenUrl};
use std::time::{Duration, Instant};

pub const SCOPES: [&str; 3] = [
    "https://www.googleapis.com/auth/calendar.events",
    "https://www.googleapis.com/auth/gmail.readonly",
    "https://www.googleapis.com/auth/gmail.compose",
];
pub const REFRESH_KEY: &str = "google-refresh-token";
/// The scopes Google granted, space separated. Older sign-ins have none: they got `SCOPES`.
pub const SCOPES_KEY: &str = "google-scopes";
/// Asked for on every sign-in, so bleep knows the account whatever else is on.
pub const BASE_SCOPES: [&str; 2] = ["openid", "email"];
pub const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive.readonly";

/// What each Google connector needs. Turning one on asks Google only for these.
pub fn scopes_for(connector: &str) -> &'static [&'static str] {
    match connector {
        "gmail" => &SCOPES[1..3],
        "calendar" => &SCOPES[0..1],
        "drive" => std::slice::from_ref(&DRIVE_SCOPE),
        _ => &[],
    }
}

/// The Google connectors, in catalog order.
pub const GOOGLE_CONNECTORS: [&str; 3] = ["gmail", "calendar", "drive"];

/// bleep's own Google OAuth client (Desktop app type), compiled in from
/// `BLEEP_GOOGLE_CLIENT_ID` and `BLEEP_GOOGLE_CLIENT_SECRET`. Google treats a desktop
/// client's secret as public; PKCE protects each sign-in. None in builds without them.
pub fn builtin_client() -> Option<GoogleConfig> {
    builtin_from(option_env!("BLEEP_GOOGLE_CLIENT_ID"), option_env!("BLEEP_GOOGLE_CLIENT_SECRET"))
}

pub fn builtin_from(id: Option<&str>, secret: Option<&str>) -> Option<GoogleConfig> {
    let (id, secret) = (id?.trim(), secret?.trim());
    (!id.is_empty() && !secret.is_empty()).then(|| GoogleConfig { client_id: id.into(), client_secret: secret.into() })
}

fn is_set(c: &GoogleConfig) -> bool {
    !c.client_id.trim().is_empty() && !c.client_secret.trim().is_empty()
}
pub const ACCOUNT_KEY: &str = "google-account";

#[derive(Clone, Debug)]
pub struct GoogleEndpoints {
    pub auth: String,
    pub token: String,
    pub revoke: String,
    pub api: String,
    /// OpenID user info: the account's email after a sign-in without Calendar.
    pub userinfo: String,
}

impl GoogleEndpoints {
    pub fn google() -> Self {
        Self {
            auth: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token: "https://oauth2.googleapis.com/token".into(),
            revoke: "https://oauth2.googleapis.com/revoke".into(),
            api: "https://www.googleapis.com/calendar/v3".into(),
            userinfo: "https://openidconnect.googleapis.com/v1/userinfo".into(),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum AuthError {
    NotConnected,
    Other(String),
}

pub const NOT_CONNECTED: &str = "Google is not connected. Ask the user to connect it in bleep settings.";

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::NotConnected => write!(f, "{NOT_CONNECTED}"),
            AuthError::Other(s) => write!(f, "{s}"),
        }
    }
}

type OAuthClient = BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

struct Cached {
    token: String,
    until: Instant,
}

/// A failed Google API call. `status` is the HTTP status when Google answered.
#[derive(Debug, PartialEq)]
pub struct ApiError {
    pub status: Option<u16>,
    pub message: String,
}

pub struct GoogleAuth {
    /// The user's own client from settings; empty means "use bleep's".
    cfg: Mutex<GoogleConfig>,
    builtin: Mutex<Option<GoogleConfig>>,
    ep: GoogleEndpoints,
    secrets: Arc<dyn Secrets>,
    opener: Arc<dyn Opener>,
    http: reqwest::Client,
    /// Held across a refresh so two callers never refresh at once.
    cache: tokio::sync::Mutex<Option<Cached>>,
    expired: std::sync::atomic::AtomicBool,
    /// The Google connectors that are on (Settings > Connectors).
    on: Mutex<std::collections::BTreeSet<String>>,
}

fn describe(e: &RequestTokenError<oauth2::HttpClientError<reqwest::Error>, BasicErrorResponse>) -> String {
    match e {
        RequestTokenError::ServerResponse(r) => r.error_description().cloned().unwrap_or_else(|| r.error().to_string()),
        other => other.to_string(),
    }
}

impl GoogleAuth {
    pub fn new(cfg: GoogleConfig, ep: GoogleEndpoints, secrets: Arc<dyn Secrets>, opener: Arc<dyn Opener>) -> Arc<GoogleAuth> {
        let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().expect("http client");
        Arc::new(GoogleAuth { cfg: Mutex::new(cfg), builtin: Mutex::new(None), ep, secrets, opener, http, cache: Default::default(), expired: Default::default(), on: Default::default() })
    }

    /// The user's own client from settings. An empty one means "use bleep's".
    /// A refresh token only works with the client that issued it, so a different
    /// client forgets the sign-in.
    pub async fn set_client(&self, cfg: GoogleConfig) {
        let before = self.effective().map(|c| c.client_id);
        *self.cfg.lock().unwrap() = cfg;
        if self.effective().map(|c| c.client_id) != before {
            let _ = self.secrets.delete(REFRESH_KEY);
            let _ = self.secrets.delete(ACCOUNT_KEY);
            *self.cache.lock().await = None;
        }
    }

    pub fn set_builtin(&self, builtin: Option<GoogleConfig>) {
        *self.builtin.lock().unwrap() = builtin;
    }

    /// True when settings hold the user's own client, which wins over the built-in one.
    pub fn using_own(&self) -> bool {
        is_set(&self.cfg.lock().unwrap())
    }

    pub fn has_builtin(&self) -> bool {
        self.builtin.lock().unwrap().is_some()
    }

    /// The client in use: the user's own if set, else bleep's built-in one.
    fn effective(&self) -> Option<GoogleConfig> {
        let own = self.cfg.lock().unwrap().clone();
        if is_set(&own) { Some(own) } else { self.builtin.lock().unwrap().clone() }
    }

    /// True after Google refused the stored sign-in, until the next sign-in.
    pub fn expired(&self) -> bool {
        self.expired.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn endpoints(&self) -> &GoogleEndpoints {
        &self.ep
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    fn client(&self) -> anyhow::Result<OAuthClient> {
        let Some(c) = self.effective() else {
            anyhow::bail!("This build of bleep has no Google app. Add your own client ID and secret in bleep settings.");
        };
        Ok(BasicClient::new(ClientId::new(c.client_id))
            .set_client_secret(ClientSecret::new(c.client_secret))
            .set_auth_uri(AuthUrl::new(self.ep.auth.clone())?)
            .set_token_uri(TokenUrl::new(self.ep.token.clone())?)
            .set_auth_type(AuthType::RequestBody))
    }

    pub fn status(&self) -> ConnectorStatus {
        if self.effective().is_none() {
            return ConnectorStatus::NeedsSetup;
        }
        if self.secrets.get(REFRESH_KEY).is_none() {
            return ConnectorStatus::NeedsSignIn;
        }
        ConnectorStatus::Connected { account: self.secrets.get(ACCOUNT_KEY).unwrap_or_else(|| "your Google account".into()) }
    }

    pub fn set_on(&self, ids: impl IntoIterator<Item = String>) {
        *self.on.lock().unwrap() = ids.into_iter().collect();
    }

    pub fn is_on(&self, connector: &str) -> bool {
        self.on.lock().unwrap().contains(connector)
    }

    /// The scopes this sign-in holds. A sign-in from before per-connector scopes holds `SCOPES`.
    pub fn granted(&self) -> std::collections::BTreeSet<String> {
        match self.secrets.get(SCOPES_KEY) {
            Some(s) => s.split_whitespace().map(String::from).collect(),
            None if self.secrets.get(REFRESH_KEY).is_some() => SCOPES.iter().map(|s| s.to_string()).collect(),
            None => Default::default(),
        }
    }

    pub fn has_scopes(&self, connector: &str) -> bool {
        let g = self.granted();
        scopes_for(connector).iter().all(|s| g.contains(*s))
    }

    /// Like `status`, but "needs sign-in" when the sign-in lacks this connector's scopes.
    pub fn status_for(&self, connector: &str) -> ConnectorStatus {
        match self.status() {
            ConnectorStatus::Connected { .. } if !self.has_scopes(connector) => ConnectorStatus::NeedsSignIn,
            s => s,
        }
    }

    /// On, signed in, and allowed: the watchers for this connector may run.
    pub fn ready(&self, connector: &str) -> bool {
        self.is_on(connector) && matches!(self.status_for(connector), ConnectorStatus::Connected { .. })
    }

    /// Signs in with every scope of the old all-in-one sign-in. Tests and older callers.
    pub async fn sign_in(&self, wait: Duration) -> anyhow::Result<String> {
        self.sign_in_for(&SCOPES, wait).await
    }

    /// Opens the consent page in the default browser and waits for the loopback reply.
    /// Asks for `scopes` plus the account's email; Google keeps what was granted before
    /// (`include_granted_scopes`). Returns the account email.
    pub async fn sign_in_for(&self, scopes: &[&str], wait: Duration) -> anyhow::Result<String> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let redirect = format!("http://127.0.0.1:{}/", listener.local_addr()?.port());
        let client = self.client()?.set_redirect_uri(RedirectUrl::new(redirect)?);
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, csrf) = client
            .authorize_url(CsrfToken::new_random)
            .add_scopes(BASE_SCOPES.iter().chain(scopes).map(|s| Scope::new(s.to_string())))
            .add_extra_param("include_granted_scopes", "true")
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent")
            .set_pkce_challenge(challenge)
            .url();
        self.opener.open(url.as_str())?;
        let code = loopback::catch(listener, csrf.secret(), wait).await?;
        let tok = client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(verifier)
            .request_async(&self.http)
            .await
            .map_err(|e| anyhow::anyhow!("Google sign-in failed: {}", describe(&e)))?;
        let Some(refresh) = tok.refresh_token() else {
            anyhow::bail!("Google sent no refresh token. Remove bleep from your Google account access page and connect again.");
        };
        let mut granted: std::collections::BTreeSet<String> = match tok.scopes() {
            Some(list) => list.iter().map(|s| s.to_string()).collect(),
            None => self.granted().into_iter().chain(scopes.iter().map(|s| s.to_string())).collect(),
        };
        granted.retain(|s| !s.is_empty());
        self.secrets.set(REFRESH_KEY, refresh.secret())?;
        self.secrets.set(SCOPES_KEY, &granted.into_iter().collect::<Vec<_>>().join(" "))?;
        self.expired.store(false, std::sync::atomic::Ordering::SeqCst);
        let access = tok.access_token().secret().clone();
        *self.cache.lock().await = Some(Cached { token: access.clone(), until: expiry(tok.expires_in()) });
        let account = self.fetch_account(&access).await.unwrap_or_else(|_| "your Google account".into());
        self.secrets.set(ACCOUNT_KEY, &account)?;
        Ok(account)
    }

    async fn fetch_account(&self, access: &str) -> anyhow::Result<String> {
        if self.has_scopes("calendar") {
            let v: serde_json::Value = self.http.get(format!("{}/calendars/primary", self.ep.api)).bearer_auth(access).send().await?.error_for_status()?.json().await?;
            if let Some(id) = v["id"].as_str() {
                return Ok(id.into());
            }
        }
        let v: serde_json::Value = self.http.get(&self.ep.userinfo).bearer_auth(access).send().await?.error_for_status()?.json().await?;
        v["email"].as_str().map(String::from).ok_or_else(|| anyhow::anyhow!("no email"))
    }

    /// A valid access token, refreshed when needed. A revoked or expired refresh token
    /// is forgotten, and the status becomes "needs sign-in".
    pub async fn access_token(&self) -> Result<String, AuthError> {
        let mut cache = self.cache.lock().await;
        if let Some(c) = cache.as_ref().filter(|c| Instant::now() < c.until) {
            return Ok(c.token.clone());
        }
        let Some(rt) = self.secrets.get(REFRESH_KEY) else { return Err(AuthError::NotConnected) };
        let client = self.client().map_err(|_| AuthError::NotConnected)?;
        match client.exchange_refresh_token(&RefreshToken::new(rt)).request_async(&self.http).await {
            Ok(tok) => {
                if let Some(new_rt) = tok.refresh_token() {
                    let _ = self.secrets.set(REFRESH_KEY, new_rt.secret());
                }
                let token = tok.access_token().secret().clone();
                *cache = Some(Cached { token: token.clone(), until: expiry(tok.expires_in()) });
                Ok(token)
            }
            Err(RequestTokenError::ServerResponse(r)) if *r.error() == BasicErrorResponseType::InvalidGrant => {
                let _ = self.secrets.delete(REFRESH_KEY);
                let _ = self.secrets.delete(ACCOUNT_KEY);
                *cache = None;
                self.expired.store(true, std::sync::atomic::Ordering::SeqCst);
                Err(AuthError::NotConnected)
            }
            Err(e) => Err(AuthError::Other(format!("Could not reach Google: {}", describe(&e)))),
        }
    }

    /// The signed-in account, lowercased, when connected.
    pub fn account(&self) -> Option<String> {
        match self.status() {
            ConnectorStatus::Connected { account } => Some(account.to_lowercase()),
            _ => None,
        }
    }

    /// Sends with a fresh access token; on 401 drops the token and tries once more.
    pub async fn send_json(&self, build: impl Fn() -> reqwest::RequestBuilder) -> Result<serde_json::Value, ApiError> {
        for attempt in 0..2 {
            let token = self.access_token().await.map_err(|e| ApiError { status: None, message: e.to_string() })?;
            let resp = build().bearer_auth(&token).send().await.map_err(|e| ApiError { status: None, message: format!("Could not reach Google: {e}") })?;
            let status = resp.status();
            if status == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                self.invalidate().await;
                continue;
            }
            let text = resp.text().await.unwrap_or_default();
            if status.is_success() {
                return if text.trim().is_empty() { Ok(serde_json::Value::Null) } else { serde_json::from_str(&text).map_err(|e| ApiError { status: Some(status.as_u16()), message: e.to_string() }) };
            }
            let message = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v["error"]["message"].as_str().map(String::from)).unwrap_or(text);
            return Err(ApiError { status: Some(status.as_u16()), message });
        }
        Err(ApiError { status: Some(401), message: "Google refused the sign-in twice.".into() })
    }

    /// Like `send_json`, for a reply that is plain text (a Drive export or file body).
    pub async fn send_text(&self, build: impl Fn() -> reqwest::RequestBuilder) -> Result<String, ApiError> {
        for attempt in 0..2 {
            let token = self.access_token().await.map_err(|e| ApiError { status: None, message: e.to_string() })?;
            let resp = build().bearer_auth(&token).send().await.map_err(|e| ApiError { status: None, message: format!("Could not reach Google: {e}") })?;
            let status = resp.status();
            if status == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                self.invalidate().await;
                continue;
            }
            let text = resp.text().await.unwrap_or_default();
            if status.is_success() {
                return Ok(text);
            }
            let message = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v["error"]["message"].as_str().map(String::from)).unwrap_or(text);
            return Err(ApiError { status: Some(status.as_u16()), message });
        }
        Err(ApiError { status: Some(401), message: "Google refused the sign-in twice.".into() })
    }

    /// Like `send_text`, for a file body (a PDF).
    pub async fn send_bytes(&self, build: impl Fn() -> reqwest::RequestBuilder) -> Result<Vec<u8>, ApiError> {
        for attempt in 0..2 {
            let token = self.access_token().await.map_err(|e| ApiError { status: None, message: e.to_string() })?;
            let resp = build().bearer_auth(&token).send().await.map_err(|e| ApiError { status: None, message: format!("Could not reach Google: {e}") })?;
            let status = resp.status();
            if status == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                self.invalidate().await;
                continue;
            }
            let body = resp.bytes().await.map_err(|e| ApiError { status: None, message: e.to_string() })?.to_vec();
            if status.is_success() {
                return Ok(body);
            }
            let text = String::from_utf8_lossy(&body).into_owned();
            let message = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v["error"]["message"].as_str().map(String::from)).unwrap_or(text);
            return Err(ApiError { status: Some(status.as_u16()), message });
        }
        Err(ApiError { status: Some(401), message: "Google refused the sign-in twice.".into() })
    }

    /// Drops the cached access token (after the API answered 401).
    pub async fn invalidate(&self) {
        *self.cache.lock().await = None;
    }

    /// Revokes the grant at Google (best effort) and forgets the tokens.
    pub async fn disconnect(&self) -> anyhow::Result<()> {
        if let Some(rt) = self.secrets.get(REFRESH_KEY) {
            let _ = self.http.post(&self.ep.revoke).form(&[("token", rt.as_str())]).send().await;
        }
        self.secrets.delete(REFRESH_KEY)?;
        self.secrets.delete(ACCOUNT_KEY)?;
        self.secrets.delete(SCOPES_KEY)?;
        *self.cache.lock().await = None;
        Ok(())
    }
}

fn expiry(expires_in: Option<Duration>) -> Instant {
    Instant::now() + expires_in.unwrap_or(Duration::from_secs(3600)).saturating_sub(Duration::from_secs(60))
}

#[cfg(test)]
mod tests {
    use super::mock::{fixture, MockGoogle};
    use super::secrets::MemorySecrets;
    use super::*;
    use crate::opener::RecordingOpener;
    use std::collections::HashMap;

    fn client() -> GoogleConfig {
        GoogleConfig { client_id: "cid.apps.googleusercontent.com".into(), client_secret: "csecret".into() }
    }

    struct Rig {
        mock: MockGoogle,
        auth: Arc<GoogleAuth>,
        secrets: Arc<MemorySecrets>,
        opener: Arc<RecordingOpener>,
    }

    async fn rig(cfg: GoogleConfig) -> Rig {
        let mock = MockGoogle::start().await;
        mock.on("GET /calendar/v3/calendars/primary", vec![(200, fixture("calendar_primary.json"))]);
        let secrets = Arc::new(MemorySecrets::default());
        let opener = Arc::new(RecordingOpener::default());
        let auth = GoogleAuth::new(cfg, mock.endpoints(), secrets.clone(), opener.clone());
        Rig { mock, auth, secrets, opener }
    }

    #[tokio::test]
    async fn per_connector_scopes_and_userinfo_account() {
        let r = rig(client()).await;
        let tok = r#"{"access_token":"at-d","expires_in":3599,"refresh_token":"rt-2","scope":"openid https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/drive.readonly","token_type":"Bearer"}"#;
        r.mock.on("POST /token", vec![(200, tok.into())]);
        r.mock.on("GET /userinfo", vec![(200, r#"{"email":"me@example.com"}"#.into())]);
        let a = r.auth.clone();
        let t = tokio::spawn(async move { a.sign_in_for(scopes_for("drive"), Duration::from_secs(5)).await });
        let q = browser(&r.opener, |q| format!("code=c1&state={}", q["state"])).await;
        let asked: Vec<&str> = q["scope"].split(' ').collect();
        assert!(asked.contains(&"email") && asked.contains(&DRIVE_SCOPE), "{asked:?}");
        assert!(!asked.iter().any(|s| s.contains("gmail")), "only what Drive needs: {asked:?}");
        assert_eq!(q["include_granted_scopes"], "true");
        assert_eq!(t.await.unwrap().unwrap(), "me@example.com", "account from user info, no Calendar scope");
        assert!(r.auth.has_scopes("drive"));
        assert!(!r.auth.has_scopes("gmail"));
        assert_eq!(r.auth.status_for("gmail"), ConnectorStatus::NeedsSignIn);
        r.auth.set_on(["drive".to_string()]);
        assert!(r.auth.ready("drive") && !r.auth.ready("gmail"));
    }

    #[test]
    fn an_older_sign_in_counts_as_all_scopes() {
        let secrets = Arc::new(MemorySecrets::default());
        secrets.set(REFRESH_KEY, "rt").unwrap();
        let auth = GoogleAuth::new(client(), GoogleEndpoints::google(), secrets, Arc::new(RecordingOpener::default()));
        assert!(auth.has_scopes("gmail") && auth.has_scopes("calendar"));
        assert!(!auth.has_scopes("drive"));
    }

    fn query(url: &str) -> HashMap<String, String> {
        reqwest::Url::parse(url).unwrap().query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
    }

    /// Waits for the auth URL, then plays the browser: hits the redirect with `extra`.
    async fn browser(opener: &RecordingOpener, extra: impl Fn(&HashMap<String, String>) -> String) -> HashMap<String, String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let url = loop {
            if let Some(u) = opener.urls.lock().unwrap().first().cloned() {
                break u;
            }
            assert!(Instant::now() < deadline, "sign_in never opened the browser");
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let q = query(&url);
        let redirect = q["redirect_uri"].clone();
        let _ = reqwest::get(format!("{redirect}?{}", extra(&q))).await;
        q
    }

    fn bleep_client() -> GoogleConfig {
        GoogleConfig { client_id: "bleep.apps.googleusercontent.com".into(), client_secret: "bleep-secret".into() }
    }

    /// Starts a sign-in, reads the client_id from the auth URL, lets it time out.
    async fn client_id_used(r: &Rig) -> String {
        let a = r.auth.clone();
        let t = tokio::spawn(async move { a.sign_in(Duration::from_millis(300)).await });
        let q = browser(&r.opener, |_| "nothing=1".into()).await;
        let _ = t.await;
        q["client_id"].clone()
    }

    #[test]
    fn builtin_needs_both_values() {
        assert_eq!(builtin_from(Some("id"), Some("sec")), Some(GoogleConfig { client_id: "id".into(), client_secret: "sec".into() }));
        assert_eq!(builtin_from(Some("id"), None), None);
        assert_eq!(builtin_from(Some(" "), Some("sec")), None);
        assert_eq!(builtin_from(None, None), None);
    }

    #[tokio::test]
    async fn builtin_client_needs_only_sign_in() {
        let r = rig(GoogleConfig::default()).await;
        assert_eq!(r.auth.status(), ConnectorStatus::NeedsSetup);
        r.auth.set_builtin(Some(bleep_client()));
        assert_eq!(r.auth.status(), ConnectorStatus::NeedsSignIn);
        assert!(r.auth.has_builtin() && !r.auth.using_own());
        assert_eq!(client_id_used(&r).await, "bleep.apps.googleusercontent.com");
    }

    #[tokio::test]
    async fn own_client_overrides_builtin() {
        let r = rig(client()).await;
        r.auth.set_builtin(Some(bleep_client()));
        assert!(r.auth.using_own());
        assert_eq!(client_id_used(&r).await, "cid.apps.googleusercontent.com");
    }

    #[tokio::test]
    async fn changing_client_forgets_the_sign_in() {
        let r = rig(GoogleConfig::default()).await;
        r.auth.set_builtin(Some(bleep_client()));
        r.secrets.set(REFRESH_KEY, "rt-bleep").unwrap();
        r.auth.set_client(GoogleConfig::default()).await;
        assert_eq!(r.secrets.get(REFRESH_KEY).as_deref(), Some("rt-bleep"), "same client, keep the sign-in");
        r.auth.set_client(client()).await;
        assert!(r.secrets.get(REFRESH_KEY).is_none(), "a token from another client cannot be refreshed");
        assert_eq!(r.auth.status(), ConnectorStatus::NeedsSignIn);
    }

    #[test]
    fn status_needs_setup_without_client_id() {
        let auth = GoogleAuth::new(GoogleConfig::default(), GoogleEndpoints::google(), Arc::new(MemorySecrets::default()), Arc::new(RecordingOpener::default()));
        assert_eq!(auth.status(), ConnectorStatus::NeedsSetup);
    }

    #[test]
    fn status_needs_sign_in_with_client_but_no_token() {
        let auth = GoogleAuth::new(client(), GoogleEndpoints::google(), Arc::new(MemorySecrets::default()), Arc::new(RecordingOpener::default()));
        assert_eq!(auth.status(), ConnectorStatus::NeedsSignIn);
    }

    #[tokio::test]
    async fn auth_url_has_pkce_offline_consent_and_all_scopes() {
        let r = rig(client()).await;
        let a = r.auth.clone();
        let t = tokio::spawn(async move { a.sign_in(Duration::from_millis(300)).await });
        let q = browser(&r.opener, |_| "nothing=1".into()).await;
        assert!(t.await.unwrap().is_err(), "no code was sent, so sign-in must fail");
        assert_eq!(q["code_challenge_method"], "S256");
        assert!(q["code_challenge"].len() >= 43);
        assert_eq!(q["access_type"], "offline");
        assert_eq!(q["prompt"], "consent");
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], "cid.apps.googleusercontent.com");
        let scopes: Vec<&str> = q["scope"].split(' ').collect();
        for s in SCOPES {
            assert!(scopes.contains(&s), "{scopes:?}");
        }
        assert!(q["redirect_uri"].starts_with("http://127.0.0.1:") && q["redirect_uri"].ends_with('/'), "{}", q["redirect_uri"]);
        assert!(r.opener.urls.lock().unwrap()[0].starts_with(&format!("{}/auth?", r.mock.base)));
    }

    #[tokio::test]
    async fn sign_in_stores_refresh_token_and_reports_account() {
        let r = rig(client()).await;
        r.mock.on("POST /token", vec![(200, fixture("token.json"))]);
        let a = r.auth.clone();
        let t = tokio::spawn(async move { a.sign_in(Duration::from_secs(5)).await });
        browser(&r.opener, |q| format!("code=abc&state={}", q["state"])).await;
        assert_eq!(t.await.unwrap().unwrap(), "me@example.com");
        assert_eq!(r.secrets.get(REFRESH_KEY).as_deref(), Some("rt-1"));
        assert_eq!(r.auth.status(), ConnectorStatus::Connected { account: "me@example.com".into() });
        let tok = &r.mock.hits_for("POST /token")[0];
        assert!(tok.body.contains("grant_type=authorization_code") && tok.body.contains("code=abc") && tok.body.contains("code_verifier="), "{}", tok.body);
        let api = &r.mock.hits_for("GET /calendar/v3/calendars/primary")[0];
        assert_eq!(api.auth.as_deref(), Some("Bearer at-code"));
    }

    #[tokio::test]
    async fn sign_in_rejects_wrong_state() {
        let r = rig(client()).await;
        r.mock.on("POST /token", vec![(200, fixture("token.json"))]);
        let a = r.auth.clone();
        let t = tokio::spawn(async move { a.sign_in(Duration::from_secs(5)).await });
        browser(&r.opener, |_| "code=abc&state=forged".into()).await;
        assert!(t.await.unwrap().is_err());
        assert!(r.secrets.get(REFRESH_KEY).is_none());
        assert!(r.mock.hits_for("POST /token").is_empty());
    }

    #[tokio::test]
    async fn sign_in_reports_denied_consent() {
        let r = rig(client()).await;
        let a = r.auth.clone();
        let t = tokio::spawn(async move { a.sign_in(Duration::from_secs(5)).await });
        browser(&r.opener, |q| format!("error=access_denied&state={}", q["state"])).await;
        let e = t.await.unwrap().unwrap_err().to_string();
        assert!(e.to_lowercase().contains("cancel"), "{e}");
    }

    #[tokio::test]
    async fn access_token_is_cached_until_expiry() {
        let r = rig(client()).await;
        r.secrets.set(REFRESH_KEY, "rt-1").unwrap();
        r.mock.on("POST /token", vec![(200, fixture("token_refresh.json"))]);
        assert_eq!(r.auth.access_token().await.unwrap(), "at-refresh");
        assert_eq!(r.auth.access_token().await.unwrap(), "at-refresh");
        let hits = r.mock.hits_for("POST /token");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].body.contains("grant_type=refresh_token") && hits[0].body.contains("refresh_token=rt-1"), "{}", hits[0].body);
        r.auth.invalidate().await;
        r.auth.access_token().await.unwrap();
        assert_eq!(r.mock.hits_for("POST /token").len(), 2);
    }

    #[tokio::test]
    async fn invalid_grant_drops_token_and_needs_sign_in() {
        let r = rig(client()).await;
        r.secrets.set(REFRESH_KEY, "rt-old").unwrap();
        r.secrets.set(ACCOUNT_KEY, "me@example.com").unwrap();
        r.mock.on("POST /token", vec![(400, fixture("token_invalid_grant.json"))]);
        assert!(!r.auth.expired());
        assert_eq!(r.auth.access_token().await, Err(AuthError::NotConnected));
        assert!(r.secrets.get(REFRESH_KEY).is_none());
        assert_eq!(r.auth.status(), ConnectorStatus::NeedsSignIn);
        assert!(r.auth.expired(), "settings must be able to say the sign-in expired");
        // A new sign-in clears it.
        r.mock.on("POST /token", vec![(200, fixture("token.json"))]);
        let a = r.auth.clone();
        let t = tokio::spawn(async move { a.sign_in(Duration::from_secs(5)).await });
        browser(&r.opener, |q| format!("code=abc&state={}", q["state"])).await;
        t.await.unwrap().unwrap();
        assert!(!r.auth.expired());
    }

    #[tokio::test]
    async fn access_token_without_sign_in_is_not_connected() {
        let r = rig(client()).await;
        assert_eq!(r.auth.access_token().await, Err(AuthError::NotConnected));
        assert!(r.mock.hits_for("POST /token").is_empty());
        assert_eq!(AuthError::NotConnected.to_string(), NOT_CONNECTED);
    }

    #[tokio::test]
    async fn disconnect_revokes_and_forgets() {
        let r = rig(client()).await;
        r.secrets.set(REFRESH_KEY, "rt-1").unwrap();
        r.secrets.set(ACCOUNT_KEY, "me@example.com").unwrap();
        r.mock.on("POST /revoke", vec![(200, "{}".into())]);
        r.auth.disconnect().await.unwrap();
        assert!(r.secrets.get(REFRESH_KEY).is_none());
        assert_eq!(r.auth.status(), ConnectorStatus::NeedsSignIn);
        assert!(r.mock.hits_for("POST /revoke")[0].body.contains("token=rt-1"));
    }
}
