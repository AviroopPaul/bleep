//! Approval cards (build plan 8.2). A write tool call waits here until the user taps
//! Approve or Decline, for up to 10 minutes. The payload stored is the payload that runs.
use crate::mcp::host::{Decision, Gate};
use crate::store::{Approval, ApprovalStatus, Store};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

pub const WAIT: Duration = Duration::from_secs(600);
pub const NOT_WAITING: &str = "This card is no longer waiting.";
pub const LOST: &str = "bleep lost this request. Nothing was changed.";

pub type ApprovalEmit = Arc<dyn Fn(Approval) + Send + Sync>;

pub struct Approvals {
    store: Arc<Store>,
    emit: ApprovalEmit,
    wait: Duration,
    waiters: Mutex<HashMap<i64, oneshot::Sender<bool>>>,
    waiting: Arc<AtomicUsize>,
}

impl Approvals {
    pub fn new(store: Arc<Store>, emit: ApprovalEmit, wait: Duration) -> Arc<Approvals> {
        Arc::new(Approvals { store, emit, wait, waiters: Default::default(), waiting: Default::default() })
    }

    /// Above zero while any card waits. The chat runtime pauses its idle timer meanwhile.
    pub fn waiting(&self) -> Arc<AtomicUsize> {
        self.waiting.clone()
    }

    /// The user's tap. Refused when the card is not waiting (already answered, expired,
    /// or left over from before a restart), so the write runs at most once.
    pub fn decide(&self, id: i64, approve: bool) -> Result<(), String> {
        // Taking the waiter first means a second tap finds nothing.
        let Some(tx) = self.waiters.lock().unwrap().remove(&id) else { return Err(NOT_WAITING.into()) };
        // Nobody is listening any more (Claude Code gave up on the call): the change can never run.
        if tx.is_closed() {
            self.expire(id);
            return Err(NOT_WAITING.into());
        }
        let to = if approve { ApprovalStatus::Approved } else { ApprovalStatus::Declined };
        if !self.store.set_approval_status(id, to, None).map_err(|e| e.to_string())? {
            return Err(NOT_WAITING.into());
        }
        self.publish(id);
        if tx.send(approve).is_err() {
            let _ = self.store.set_approval_status(id, ApprovalStatus::Failed, Some(LOST));
            self.publish(id);
            return Err(NOT_WAITING.into());
        }
        Ok(())
    }

    /// Marks a still-pending card expired and tells the UI. No-op for a decided card.
    fn expire(&self, id: i64) {
        self.waiters.lock().unwrap().remove(&id);
        if self.store.set_approval_status(id, ApprovalStatus::Expired, None).unwrap_or(false) {
            self.publish(id);
        }
    }

    fn publish(&self, id: i64) {
        if let Ok(Some(a)) = self.store.get_approval(id) {
            (self.emit)(a);
        }
    }
}

/// Expires the card if the ask ends without an answer, including when Claude Code drops
/// the call and the ask future is dropped mid-wait.
struct CardGuard<'a>(&'a Approvals, i64);
impl Drop for CardGuard<'_> {
    fn drop(&mut self) {
        self.0.expire(self.1);
    }
}

/// Counts one waiting card for as long as it lives.
struct Waiting(Arc<AtomicUsize>);
impl Waiting {
    fn new(n: Arc<AtomicUsize>) -> Self {
        n.fetch_add(1, Ordering::SeqCst);
        Waiting(n)
    }
}
impl Drop for Waiting {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl Gate for Approvals {
    async fn ask(&self, tool: &str, payload: Value, turn: Option<i64>) -> anyhow::Result<(i64, Decision)> {
        let _waiting = Waiting::new(self.waiting.clone());
        let id = self.store.insert_approval(tool, &payload, turn)?;
        let (tx, mut rx) = oneshot::channel();
        self.waiters.lock().unwrap().insert(id, tx);
        let _card = CardGuard(self, id);
        self.publish(id);
        let answer = match tokio::time::timeout(self.wait, &mut rx).await {
            Ok(r) => r.ok(),
            // Timed out. If a tap took the waiter at the same moment, its answer wins.
            Err(_) if self.waiters.lock().unwrap().remove(&id).is_none() => rx.await.ok(),
            Err(_) => None,
        };
        Ok(match answer {
            Some(true) => (id, Decision::Approved),
            Some(false) => (id, Decision::Declined),
            None => {
                self.expire(id);
                (id, Decision::Expired)
            }
        })
    }

    fn finish(&self, id: i64, ok: bool, result: &str) {
        let to = if ok { ApprovalStatus::Done } else { ApprovalStatus::Failed };
        if let Err(e) = self.store.set_approval_status(id, to, Some(result)) {
            eprintln!("bleep: could not record approval {id}: {e}");
        }
        self.publish(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Rig {
        _d: tempfile::TempDir,
        store: Arc<Store>,
        ap: Arc<Approvals>,
        seen: Arc<Mutex<Vec<ApprovalStatus>>>,
    }

    fn rig(wait: Duration) -> Rig {
        let d = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
        let seen: Arc<Mutex<Vec<ApprovalStatus>>> = Default::default();
        let s = seen.clone();
        let ap = Approvals::new(store.clone(), Arc::new(move |a: Approval| s.lock().unwrap().push(a.status)), wait);
        Rig { _d: d, store, ap, seen }
    }

    /// Starts an ask and waits until its card exists. Returns the card id and the ask task.
    async fn ask(r: &Rig) -> (i64, tokio::task::JoinHandle<(i64, Decision)>) {
        let ap = r.ap.clone();
        let t = tokio::spawn(async move { ap.ask("calendar_create", json!({"title": "Pay rent"}), None).await.unwrap() });
        for _ in 0..200 {
            if let Some(id) = r.ap.waiters.lock().unwrap().keys().next().copied() {
                return (id, t);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("no card was created");
    }

    #[tokio::test]
    async fn approve_resolves_the_waiting_ask() {
        let r = rig(WAIT);
        let (id, t) = ask(&r).await;
        r.ap.decide(id, true).unwrap();
        assert_eq!(t.await.unwrap(), (id, Decision::Approved));
        assert_eq!(r.store.get_approval(id).unwrap().unwrap().status, ApprovalStatus::Approved);
        assert_eq!(*r.seen.lock().unwrap(), vec![ApprovalStatus::Pending, ApprovalStatus::Approved]);
        r.ap.finish(id, true, "{\"id\":\"ev1\"}");
        assert_eq!(r.store.get_approval(id).unwrap().unwrap().status, ApprovalStatus::Done);
        assert_eq!(r.seen.lock().unwrap().last(), Some(&ApprovalStatus::Done));
    }

    #[tokio::test]
    async fn decline_resolves_declined() {
        let r = rig(WAIT);
        let (id, t) = ask(&r).await;
        r.ap.decide(id, false).unwrap();
        assert_eq!(t.await.unwrap().1, Decision::Declined);
        assert_eq!(r.store.get_approval(id).unwrap().unwrap().status, ApprovalStatus::Declined);
    }

    #[tokio::test]
    async fn no_answer_expires() {
        let r = rig(Duration::from_millis(50));
        let (id, t) = ask(&r).await;
        assert_eq!(t.await.unwrap().1, Decision::Expired);
        assert_eq!(r.store.get_approval(id).unwrap().unwrap().status, ApprovalStatus::Expired);
        assert_eq!(r.seen.lock().unwrap().last(), Some(&ApprovalStatus::Expired));
    }

    #[tokio::test]
    async fn second_decide_is_refused() {
        let r = rig(WAIT);
        let (id, t) = ask(&r).await;
        r.ap.decide(id, true).unwrap();
        assert_eq!(r.ap.decide(id, true), Err(NOT_WAITING.to_string()));
        assert_eq!(r.ap.decide(id, false), Err(NOT_WAITING.to_string()));
        assert_eq!(t.await.unwrap().1, Decision::Approved);
        assert_eq!(r.store.get_approval(id).unwrap().unwrap().status, ApprovalStatus::Approved);
    }

    #[tokio::test]
    async fn decide_after_expiry_is_refused() {
        let r = rig(Duration::from_millis(30));
        let (id, t) = ask(&r).await;
        t.await.unwrap();
        assert_eq!(r.ap.decide(id, true), Err(NOT_WAITING.to_string()));
        assert_eq!(r.store.get_approval(id).unwrap().unwrap().status, ApprovalStatus::Expired);
    }

    #[tokio::test]
    async fn decide_on_a_card_from_before_restart_is_refused() {
        let r = rig(WAIT);
        let id = r.store.insert_approval("calendar_create", &json!({}), None).unwrap();
        assert_eq!(r.ap.decide(id, true), Err(NOT_WAITING.to_string()));
    }

    #[tokio::test]
    async fn dropped_ask_expires_its_card() {
        // Claude Code gave up on the call (killed child, reset connection): the ask future is dropped.
        let r = rig(WAIT);
        let (id, t) = ask(&r).await;
        t.abort();
        let _ = t.await;
        assert_eq!(r.ap.decide(id, true), Err(NOT_WAITING.to_string()));
        assert_eq!(r.store.get_approval(id).unwrap().unwrap().status, ApprovalStatus::Expired);
        assert_eq!(r.ap.waiting().load(Ordering::SeqCst), 0);
        assert_eq!(r.seen.lock().unwrap().last(), Some(&ApprovalStatus::Expired));
    }

    #[tokio::test]
    async fn approve_with_nobody_listening_is_refused() {
        // The waiter exists but its receiver is gone: the tap must not mark the card approved.
        let r = rig(WAIT);
        let id = r.store.insert_approval("calendar_create", &json!({}), None).unwrap();
        let (tx, rx) = oneshot::channel();
        drop(rx);
        r.ap.waiters.lock().unwrap().insert(id, tx);
        assert_eq!(r.ap.decide(id, true), Err(NOT_WAITING.to_string()));
        assert_eq!(r.store.get_approval(id).unwrap().unwrap().status, ApprovalStatus::Expired);
    }

    #[tokio::test]
    async fn waiting_counter_tracks_open_asks() {
        let r = rig(WAIT);
        let w = r.ap.waiting();
        assert_eq!(w.load(Ordering::SeqCst), 0);
        let (id, t) = ask(&r).await;
        assert_eq!(w.load(Ordering::SeqCst), 1);
        r.ap.decide(id, false).unwrap();
        t.await.unwrap();
        assert_eq!(w.load(Ordering::SeqCst), 0);
    }
}
