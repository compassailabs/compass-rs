use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::automation::evaluator::Snapshot;

#[async_trait]
pub trait SnapshotStore: Send + Sync {
    async fn latest(&self) -> Result<Option<Snapshot>>;
    async fn put(&self, snapshot: Snapshot) -> Result<()>;
}

#[derive(Default)]
pub struct InMemorySnapshotStore {
    inner: RwLock<Option<Snapshot>>,
}

impl InMemorySnapshotStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SnapshotStore for InMemorySnapshotStore {
    async fn latest(&self) -> Result<Option<Snapshot>> {
        Ok(self.inner.read().await.clone())
    }

    async fn put(&self, snapshot: Snapshot) -> Result<()> {
        *self.inner.write().await = Some(snapshot);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::evaluator::GatewayHealth;
    use chrono::DateTime;
    use std::collections::HashMap;

    fn snap() -> Snapshot {
        Snapshot {
            built_at: DateTime::from_timestamp(2_000_000_000, 0).unwrap(),
            usdc_usd: 1.0,
            gateway_health: GatewayHealth::Ok,
            venues: HashMap::new(),
            gas_usd_per_userop: HashMap::new(),
            gateway_fee_usd: 0.1,
        }
    }

    #[tokio::test]
    async fn empty_initially() {
        let store = InMemorySnapshotStore::new();
        assert!(store.latest().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn put_then_latest() {
        let store = InMemorySnapshotStore::new();
        store.put(snap()).await.unwrap();
        assert!((store.latest().await.unwrap().unwrap().usdc_usd - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn put_overwrites() {
        let store = InMemorySnapshotStore::new();
        store.put(snap()).await.unwrap();
        let mut s2 = snap();
        s2.usdc_usd = 0.99;
        store.put(s2).await.unwrap();
        assert!((store.latest().await.unwrap().unwrap().usdc_usd - 0.99).abs() < 1e-9);
    }
}
