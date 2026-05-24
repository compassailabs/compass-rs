use std::collections::HashMap;

use alloy::primitives::Address;
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::automation::evaluator::Position;

#[async_trait]
pub trait PositionStore: Send + Sync {
    async fn get(&self, user: Address) -> Result<Option<Position>>;
    async fn put(&self, user: Address, position: Position) -> Result<()>;
}

#[async_trait]
pub trait PositionFetcher: Send + Sync {
    async fn fetch(&self, user: Address) -> Result<Position>;
}

#[derive(Default)]
pub struct InMemoryPositionStore {
    inner: RwLock<HashMap<Address, Position>>,
}

impl InMemoryPositionStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl PositionStore for InMemoryPositionStore {
    async fn get(&self, user: Address) -> Result<Option<Position>> {
        Ok(self.inner.read().await.get(&user).cloned())
    }

    async fn put(&self, user: Address, position: Position) -> Result<()> {
        self.inner.write().await.insert(user, position);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::policy::{ChainId, ProtocolId, VenueRef};
    use alloy::primitives::U256;

    fn position_with_idle(amount: u128) -> Position {
        let mut h = HashMap::new();
        h.insert(
            VenueRef {
                chain: ChainId::Arc,
                protocol: ProtocolId::Idle,
            },
            U256::from(amount),
        );
        Position {
            holdings: h,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn empty_for_unknown_user() {
        let store = InMemoryPositionStore::new();
        assert!(store.get(Address::ZERO).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn put_then_get_round_trip() {
        let store = InMemoryPositionStore::new();
        let u = Address::repeat_byte(0x11);
        store.put(u, position_with_idle(5_000)).await.unwrap();
        let p = store.get(u).await.unwrap().unwrap();
        assert_eq!(p.holdings.values().next().unwrap(), &U256::from(5_000u128));
    }

    #[tokio::test]
    async fn put_overwrites_existing() {
        let store = InMemoryPositionStore::new();
        let u = Address::repeat_byte(0x22);
        store.put(u, position_with_idle(1)).await.unwrap();
        store.put(u, position_with_idle(2)).await.unwrap();
        let p = store.get(u).await.unwrap().unwrap();
        assert_eq!(p.holdings.values().next().unwrap(), &U256::from(2u128));
    }

    #[tokio::test]
    async fn isolates_users() {
        let store = InMemoryPositionStore::new();
        let u1 = Address::repeat_byte(0x33);
        let u2 = Address::repeat_byte(0x44);
        store.put(u1, position_with_idle(10)).await.unwrap();
        store.put(u2, position_with_idle(20)).await.unwrap();
        assert_eq!(
            store.get(u1).await.unwrap().unwrap().holdings.values().next().unwrap(),
            &U256::from(10u128)
        );
        assert_eq!(
            store.get(u2).await.unwrap().unwrap().holdings.values().next().unwrap(),
            &U256::from(20u128)
        );
    }
}
