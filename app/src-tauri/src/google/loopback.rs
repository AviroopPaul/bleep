//! Catches Google's redirect to http://127.0.0.1:<port>/ once.
use anyhow::bail;
use axum::extract::Query;
use axum::response::Html;
use std::collections::HashMap;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

const PAGE: &str = "<!doctype html><meta charset=utf-8><title>bleep</title><body style=\"font:16px -apple-system,sans-serif;display:grid;place-items:center;height:90vh;color:#2b3440\"><p>{}</p></body>";
pub const DONE: &str = "bleep is connected. You can close this tab.";
pub const CANCELLED: &str = "Sign-in was cancelled.";
pub const MISMATCH: &str = "Sign-in failed: this reply did not match the request from bleep.";

fn page(text: &str) -> Html<String> {
    Html(PAGE.replace("{}", text))
}

/// Serves `/` until one reply arrives, then stops. Returns the authorization code,
/// or an error in plain words. A reply with the wrong `state` is refused.
pub async fn catch(listener: TcpListener, state: &str, wait: Duration) -> anyhow::Result<String> {
    let q = catch_at(listener, "/", state, wait).await?;
    Ok(q.get("code").cloned().unwrap_or_default())
}

/// Like `catch` on any path, and returns the whole query (`code`, `state`, `iss`).
pub async fn catch_at(listener: TcpListener, path: &str, state: &str, wait: Duration) -> anyhow::Result<HashMap<String, String>> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<HashMap<String, String>, String>>(1);
    let expected = state.to_string();
    let app = axum::Router::new().route(
        path,
        axum::routing::get(move |Query(q): Query<HashMap<String, String>>| {
            let (tx, expected) = (tx.clone(), expected.clone());
            async move {
                let (res, text): (Result<HashMap<String, String>, String>, &str) = if q.get("state") != Some(&expected) {
                    (Err(MISMATCH.into()), MISMATCH)
                } else if let Some(e) = q.get("error") {
                    let why = if e == "access_denied" { CANCELLED.to_string() } else { format!("Sign-in failed: {e}.") };
                    (Err(why), CANCELLED)
                } else if q.contains_key("code") {
                    (Ok(q.clone()), DONE)
                } else {
                    (Err(MISMATCH.into()), MISMATCH)
                };
                let _ = tx.try_send(res);
                page(text)
            }
        }),
    );
    let stop = CancellationToken::new();
    let s = stop.clone();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).with_graceful_shutdown(async move { s.cancelled_owned().await }).await;
    });
    let got = tokio::time::timeout(wait, rx.recv()).await;
    stop.cancel();
    match got {
        Err(_) | Ok(None) => bail!("No answer from the browser in {} minutes.", wait.as_secs().div_ceil(60).max(1)),
        Ok(Some(Ok(q))) => Ok(q),
        Ok(Some(Err(why))) => bail!(why),
    }
}
