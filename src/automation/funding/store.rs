use alloy::primitives::{Address, U256};
use anyhow::Result;
use async_trait::async_trait;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

use super::schema::{FundingEvent, FundingKind, NewFundingEvent};

#[async_trait]
pub trait FundingStore: Send + Sync {
    async fn append(&self, event: NewFundingEvent) -> Result<u64>;
    async fn sum(&self, user: Address, kind: FundingKind) -> Result<U256>;
    async fn list_for_user(&self, user: Address, limit: usize) -> Result<Vec<FundingEvent>>;
}

pub async fn net_deposited(store: &dyn FundingStore, user: Address) -> Result<U256> {
    let deposits = store.sum(user, FundingKind::Deposit).await?;
    let withdrawals = store.sum(user, FundingKind::WithdrawToEoa).await?;
    Ok(deposits.saturating_sub(withdrawals))
}

pub struct InMemoryFundingStore {
    next_id: AtomicU64,
    events: RwLock<Vec<FundingEvent>>,
}

impl InMemoryFundingStore {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            events: RwLock::new(Vec::new()),
        }
    }
}

impl Default for InMemoryFundingStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FundingStore for InMemoryFundingStore {
    async fn append(&self, event: NewFundingEvent) -> Result<u64> {
        let mut guard = self.events.write().await;
        if let Some(existing) = guard.iter().find(|e| {
            e.user == event.user
                && e.chain == event.chain
                && e.kind == event.kind
                && e.tx_hash == event.tx_hash
        }) {
            return Ok(existing.id);
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        guard.push(FundingEvent {
            id,
            ts: event.ts,
            user: event.user,
            chain: event.chain,
            kind: event.kind,
            amount_6dec: event.amount.to_string(),
            tx_hash: event.tx_hash,
        });
        Ok(id)
    }

    async fn sum(&self, user: Address, kind: FundingKind) -> Result<U256> {
        let guard = self.events.read().await;
        let mut total = U256::ZERO;
        for e in guard.iter().filter(|e| e.user == user && e.kind == kind) {
            total += U256::from_str(&e.amount_6dec).unwrap_or(U256::ZERO);
        }
        Ok(total)
    }

    async fn list_for_user(&self, user: Address, limit: usize) -> Result<Vec<FundingEvent>> {
        let guard = self.events.read().await;
        let mut out: Vec<FundingEvent> = guard
            .iter()
            .filter(|e| e.user == user)
            .cloned()
            .collect();
        out.sort_by(|a, b| b.id.cmp(&a.id));
        out.truncate(limit);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    fn now() -> DateTime<chrono::Utc> {
        DateTime::from_timestamp(2_000_000_000, 0).unwrap()
    }

    fn ev(
        user: Address,
        chain: crate::automation::policy::ChainId,
        kind: FundingKind,
        amount: u64,
        tx: &str,
    ) -> NewFundingEvent {
        NewFundingEvent::new(user, chain, kind, U256::from(amount), tx, now())
    }

    #[tokio::test]
    async fn append_assigns_sequential_ids() {
        let store = InMemoryFundingStore::new();
        let u = Address::repeat_byte(0x11);
        let chain = crate::automation::policy::ChainId::Arc;
        let id1 = store.append(ev(u, chain, FundingKind::Deposit, 100, "0xa")).await.unwrap();
        let id2 = store.append(ev(u, chain, FundingKind::Deposit, 200, "0xb")).await.unwrap();
        assert_eq!((id1, id2), (1, 2));
    }

    #[tokio::test]
    async fn append_is_idempotent_per_tx_hash() {
        let store = InMemoryFundingStore::new();
        let u = Address::repeat_byte(0x22);
        let chain = crate::automation::policy::ChainId::Arc;
        let id1 = store.append(ev(u, chain, FundingKind::Deposit, 100, "0xdupe")).await.unwrap();
        let id2 = store.append(ev(u, chain, FundingKind::Deposit, 999, "0xdupe")).await.unwrap();
        assert_eq!(id1, id2);
        let total = store.sum(u, FundingKind::Deposit).await.unwrap();
        assert_eq!(total, U256::from(100u64));
    }

    #[tokio::test]
    async fn sum_filters_by_user_and_kind() {
        let store = InMemoryFundingStore::new();
        let u1 = Address::repeat_byte(0x33);
        let u2 = Address::repeat_byte(0x44);
        let chain = crate::automation::policy::ChainId::Arc;
        store.append(ev(u1, chain, FundingKind::Deposit, 100, "0x1")).await.unwrap();
        store.append(ev(u1, chain, FundingKind::Deposit, 200, "0x2")).await.unwrap();
        store.append(ev(u1, chain, FundingKind::WithdrawToEoa, 50, "0x3")).await.unwrap();
        store.append(ev(u2, chain, FundingKind::Deposit, 999, "0x4")).await.unwrap();

        assert_eq!(store.sum(u1, FundingKind::Deposit).await.unwrap(), U256::from(300u64));
        assert_eq!(store.sum(u1, FundingKind::WithdrawToEoa).await.unwrap(), U256::from(50u64));
        assert_eq!(store.sum(u2, FundingKind::Deposit).await.unwrap(), U256::from(999u64));
    }

    #[tokio::test]
    async fn net_deposited_clamps_to_zero() {
        let store = InMemoryFundingStore::new();
        let u = Address::repeat_byte(0x55);
        let chain = crate::automation::policy::ChainId::Arc;
        store.append(ev(u, chain, FundingKind::Deposit, 50, "0x1")).await.unwrap();
        store.append(ev(u, chain, FundingKind::WithdrawToEoa, 999, "0x2")).await.unwrap();
        let net = net_deposited(&store, u).await.unwrap();
        assert_eq!(net, U256::ZERO);
    }
}
