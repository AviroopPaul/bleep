//! A local stand-in for Google's token and REST endpoints. Tests only.
use axum::body::Bytes;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::IntoResponse;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::GoogleEndpoints;

#[derive(Clone, Debug)]
pub struct Hit {
    pub method: String,
    pub path: String,
    pub query: String,
    pub body: String,
    pub auth: Option<String>,
}

#[derive(Default)]
struct Routes {
    /// "METHOD /path" -> replies; the last one repeats.
    map: HashMap<String, Vec<(u16, Vec<u8>)>>,
    /// ("METHOD /path", text the query must contain) -> reply. Checked before `map`.
    by_query: Vec<(String, String, (u16, Vec<u8>))>,
    /// "METHOD /path" -> delay before replying.
    delay: HashMap<String, std::time::Duration>,
}

#[derive(Clone)]
pub struct MockGoogle {
    pub base: String,
    pub hits: Arc<Mutex<Vec<Hit>>>,
    routes: Arc<Mutex<Routes>>,
}

pub fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/google/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

impl MockGoogle {
    pub async fn start() -> MockGoogle {
        let hits: Arc<Mutex<Vec<Hit>>> = Default::default();
        let routes: Arc<Mutex<Routes>> = Default::default();
        let (h, r) = (hits.clone(), routes.clone());
        let app = axum::Router::new().fallback(move |method: Method, uri: Uri, headers: HeaderMap, body: Bytes| {
            let (h, r) = (h.clone(), r.clone());
            async move {
                h.lock().unwrap().push(Hit {
                    method: method.to_string(),
                    path: uri.path().to_string(),
                    query: uri.query().unwrap_or_default().to_string(),
                    body: String::from_utf8_lossy(&body).into_owned(),
                    auth: headers.get("authorization").and_then(|v| v.to_str().ok()).map(String::from),
                });
                let key = format!("{method} {}", uri.path());
                let wait = r.lock().unwrap().delay.get(&key).copied();
                if let Some(d) = wait {
                    tokio::time::sleep(d).await;
                }
                let mut routes = r.lock().unwrap();
                let query = uri.query().unwrap_or_default().to_string();
                let by_query = routes.by_query.iter().find(|(k, needle, _)| *k == key && query.contains(needle.as_str())).map(|(_, _, r)| r.clone());
                let reply = match by_query {
                    Some(r) => Some(r),
                    None => match routes.map.get_mut(&key) {
                    Some(q) if q.len() > 1 => Some(q.remove(0)),
                    Some(q) => q.first().cloned(),
                    None => None,
                    },
                };
                match reply {
                    Some((code, body)) => (StatusCode::from_u16(code).unwrap(), [("content-type", "application/json")], body).into_response(),
                    None => (StatusCode::NOT_FOUND, format!("no mock for {key}")).into_response(),
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        MockGoogle { base, hits, routes }
    }

    /// Replies for "METHOD /path", in order; the last one repeats.
    pub fn on(&self, key: &str, replies: Vec<(u16, String)>) {
        self.routes.lock().unwrap().map.insert(key.into(), replies.into_iter().map(|(c, b)| (c, b.into_bytes())).collect());
    }

    /// A reply for "METHOD /path" when the raw query contains `needle`.
    pub fn on_query(&self, key: &str, needle: &str, reply: (u16, String)) {
        self.on_query_bytes(key, needle, (reply.0, reply.1.into_bytes()));
    }

    /// Like `on_query`, for a binary body (a PDF).
    pub fn on_query_bytes(&self, key: &str, needle: &str, reply: (u16, Vec<u8>)) {
        self.routes.lock().unwrap().by_query.push((key.into(), needle.into(), reply));
    }

    /// Replies for "METHOD /path" arrive only after `d`.
    pub fn slow(&self, key: &str, d: std::time::Duration) {
        self.routes.lock().unwrap().delay.insert(key.into(), d);
    }

    pub fn endpoints(&self) -> GoogleEndpoints {
        GoogleEndpoints {
            auth: format!("{}/auth", self.base),
            token: format!("{}/token", self.base),
            revoke: format!("{}/revoke", self.base),
            api: format!("{}/calendar/v3", self.base),
            userinfo: format!("{}/userinfo", self.base),
        }
    }

    pub fn hits_for(&self, key: &str) -> Vec<Hit> {
        self.hits.lock().unwrap().iter().filter(|h| format!("{} {}", h.method, h.path) == key).cloned().collect()
    }
}
