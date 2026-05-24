use alloy::primitives::Address;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

use super::schema::{AuditEvent, NewAuditEvent};

#[async_trait]
pub trait AuditStore: Send + Sync {
    /// Append an event. Returns the assigned id.
    async fn append(&self, event: NewAuditEvent) -> Result<u64>;
    async fn get(&self, id: u64) -> Result<Option<AuditEvent>>;
    async fn list_for_user(
        &self,
        user: Address,
        since: Option<DateTime<Utc>>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>>;
}

pub struct InMemoryAuditStore {
    next_id: AtomicU64,
    events: RwLock<Vec<AuditEvent>>,
}

impl InMemoryAuditStore {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            events: RwLock::new(Vec::new()),
        }
    }
}

impl Default for InMemoryAuditStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AuditStore for InMemoryAuditStore {
    async fn append(&self, event: NewAuditEvent) -> Result<u64> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let stored = AuditEvent {
            id,
            ts: event.ts,
            user: event.user,
            event_type: event.event_type,
            policy_version: event.policy_version,
            payload: event.payload,
            tx_hash: event.tx_hash,
            chain: event.chain,
            cost_usd: event.cost_usd,
        };
        self.events.write().await.push(stored);
        Ok(id)
    }

    async fn get(&self, id: u64) -> Result<Option<AuditEvent>> {
        Ok(self
            .events
            .read()
            .await
            .iter()
            .find(|e| e.id == id)
            .cloned())
    }

    async fn list_for_user(
        &self,
        user: Address,
        since: Option<DateTime<Utc>>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>> {
        let guard = self.events.read().await;
        let mut out: Vec<AuditEvent> = guard
            .iter()
            .filter(|e| e.user == user)
            .filter(|e| since.map_or(true, |s| e.ts > s))
            .cloned()
            .collect();
        out.sort_by(|a, b| b.id.cmp(&a.id));
        out.truncate(limit);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::super::schema::EventType;
    use super::*;
    use chrono::Duration;
    use serde_json::json;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(2_000_000_000, 0).unwrap()
    }

    fn ev(user: Address, ts: DateTime<Utc>, et: EventType) -> NewAuditEvent {
        NewAuditEvent::new(user, et, json!({"hello": "world"}), ts)
    }

    #[tokio::test]
    async fn append_assigns_sequential_ids() {
        let store = InMemoryAuditStore::new();
        let u = Address::repeat_byte(0x11);
        let id1 = store
            .append(ev(u, now(), EventType::TriggerFired))
            .await
            .unwrap();
        let id2 = store
            .append(ev(u, now(), EventType::EvaluatorDecision))
            .await
            .unwrap();
        let id3 = store
            .append(ev(u, now(), EventType::ExecutorActionDone))
            .await
            .unwrap();
        assert_eq!((id1, id2, id3), (1, 2, 3));
    }

    #[tokio::test]
    async fn get_returns_stored_event() {
        let store = InMemoryAuditStore::new();
        let u = Address::repeat_byte(0x22);
        let id = store
            .append(
                NewAuditEvent::new(u, EventType::PolicyChange, json!({"v": 5}), now())
                    .with_policy_version(5)
                    .with_chain(crate::automation::policy::ChainId::Arc)
                    .with_cost_usd(0.0),
            )
            .await
            .unwrap();
        let got = store.get(id).await.unwrap().unwrap();
        assert_eq!(got.id, id);
        assert_eq!(got.user, u);
        assert_eq!(got.policy_version, Some(5));
        assert_eq!(got.chain, Some(crate::automation::policy::ChainId::Arc));
        assert_eq!(got.payload, json!({"v": 5}));
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let store = InMemoryAuditStore::new();
        assert!(store.get(999).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_filters_by_user_and_returns_newest_first() {
        let store = InMemoryAuditStore::new();
        let u1 = Address::repeat_byte(0x33);
        let u2 = Address::repeat_byte(0x44);
        store
            .append(ev(u1, now(), EventType::TriggerFired))
            .await
            .unwrap();
        store
            .append(ev(u2, now(), EventType::TriggerFired))
            .await
            .unwrap();
        store
            .append(ev(u1, now(), EventType::EvaluatorDecision))
            .await
            .unwrap();

        let out = store.list_for_user(u1, None, 10).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].event_type, EventType::EvaluatorDecision);
        assert_eq!(out[1].event_type, EventType::TriggerFired);
    }

    #[tokio::test]
    async fn list_respects_limit() {
        let store = InMemoryAuditStore::new();
        let u = Address::repeat_byte(0x55);
        for _ in 0..5 {
            store
                .append(ev(u, now(), EventType::TriggerFired))
                .await
                .unwrap();
        }
        let out = store.list_for_user(u, None, 2).await.unwrap();
        assert_eq!(out.len(), 2);
    }

    #[tokio::test]
    async fn list_respects_since() {
        let store = InMemoryAuditStore::new();
        let u = Address::repeat_byte(0x66);
        let t0 = now();
        let t1 = t0 + Duration::seconds(10);
        let t2 = t0 + Duration::seconds(20);
        store.append(ev(u, t0, EventType::TriggerFired)).await.unwrap();
        store.append(ev(u, t1, EventType::TriggerFired)).await.unwrap();
        store.append(ev(u, t2, EventType::TriggerFired)).await.unwrap();
        let out = store.list_for_user(u, Some(t1), 10).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].ts, t2);
    }

    #[tokio::test]
    async fn list_empty_for_unknown_user() {
        let store = InMemoryAuditStore::new();
        let out = store
            .list_for_user(Address::repeat_byte(0x77), None, 10)
            .await
            .unwrap();
        assert!(out.is_empty());
    }
}
