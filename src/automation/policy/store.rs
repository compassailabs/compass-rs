use std::collections::HashMap;

use alloy::primitives::Address;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use tokio::sync::RwLock;

use super::schema::{Policy, PolicyStatus};

#[async_trait]
pub trait PolicyStore: Send + Sync {
    async fn get(&self, user: Address) -> Result<Option<Policy>>;
    async fn put(&self, policy: Policy) -> Result<u32>;
    async fn set_status(&self, user: Address, status: PolicyStatus) -> Result<()>;
    async fn list_active_users(&self) -> Result<Vec<Address>>;
}

#[derive(Default)]
pub struct InMemoryPolicyStore {
    inner: RwLock<HashMap<Address, Policy>>,
}

impl InMemoryPolicyStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl PolicyStore for InMemoryPolicyStore {
    async fn get(&self, user: Address) -> Result<Option<Policy>> {
        Ok(self.inner.read().await.get(&user).cloned())
    }

    async fn put(&self, mut policy: Policy) -> Result<u32> {
        policy.validate()?;
        let mut guard = self.inner.write().await;
        let next_version = guard.get(&policy.user).map_or(1, |p| p.version + 1);
        policy.version = next_version;
        let user = policy.user;
        guard.insert(user, policy);
        Ok(next_version)
    }

    async fn set_status(&self, user: Address, status: PolicyStatus) -> Result<()> {
        let mut guard = self.inner.write().await;
        match guard.get_mut(&user) {
            Some(p) => {
                p.status = status;
                Ok(())
            }
            None => Err(anyhow!("no policy for user {user}")),
        }
    }

    async fn list_active_users(&self) -> Result<Vec<Address>> {
        Ok(self
            .inner
            .read()
            .await
            .iter()
            .filter(|(_, p)| p.status == PolicyStatus::Active)
            .map(|(addr, _)| *addr)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::super::schema::{
        CapsConfig, ChainId, ChainsConfig, CircuitBreakersConfig, GasConfig, ProtocolId,
        ProtocolsConfig, TriggersConfig, VenueRef,
    };
    use super::*;
    use chrono::DateTime;

    fn good_policy(user: Address) -> Policy {
        Policy {
            version: 0,
            user,
            risk_label: "balanced".into(),
            created_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            compiled_from: None,
            status: PolicyStatus::Active,
            protocols: ProtocolsConfig {
                whitelist: vec![
                    VenueRef {
                        chain: ChainId::Arc,
                        protocol: ProtocolId::Idle,
                    },
                    VenueRef {
                        chain: ChainId::ArbitrumSepolia,
                        protocol: ProtocolId::AaveV3,
                    },
                ],
                per_protocol_cap_pct: 60,
            },
            chains: ChainsConfig {
                whitelist: vec![ChainId::Arc, ChainId::ArbitrumSepolia],
            },
            triggers: TriggersConfig {
                apr_delta_bps: 10,
                apr_lookback_minutes: 60,
                min_idle_minutes: 30,
            },
            caps: CapsConfig {
                max_move_pct_per_action: 100,
                max_actions_per_day: 6,
                min_net_profit_usd: 0.0,
            },
            gas: GasConfig {
                estimated_hold_days: 7,
                max_gas_usd_per_action: 5.0,
            },
            circuit_breakers: CircuitBreakersConfig {
                usdc_peg_min: 0.98,
                utilization_max: 0.95,
                tvl_drop_pct_1h: 30.0,
                protocol_blacklist_on_event: true,
            },
            notifications: None,
        }
    }

    #[tokio::test]
    async fn put_then_get_round_trip() {
        let store = InMemoryPolicyStore::new();
        let user = Address::repeat_byte(0x11);
        let v = store.put(good_policy(user)).await.unwrap();
        assert_eq!(v, 1);

        let fetched = store.get(user).await.unwrap().unwrap();
        assert_eq!(fetched.version, 1);
        assert_eq!(fetched.user, user);
    }

    #[tokio::test]
    async fn versions_increment_per_user() {
        let store = InMemoryPolicyStore::new();
        let user = Address::repeat_byte(0x22);
        assert_eq!(store.put(good_policy(user)).await.unwrap(), 1);
        assert_eq!(store.put(good_policy(user)).await.unwrap(), 2);
        assert_eq!(store.put(good_policy(user)).await.unwrap(), 3);

        let other = Address::repeat_byte(0x33);
        assert_eq!(store.put(good_policy(other)).await.unwrap(), 1);

        assert_eq!(store.get(user).await.unwrap().unwrap().version, 3);
        assert_eq!(store.get(other).await.unwrap().unwrap().version, 1);
    }

    #[tokio::test]
    async fn put_rejects_invalid_policy() {
        let store = InMemoryPolicyStore::new();
        let user = Address::repeat_byte(0x44);
        let mut bad = good_policy(user);
        bad.protocols.whitelist.clear();
        assert!(store.put(bad).await.is_err());
        assert!(store.get(user).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn set_status_updates_existing() {
        let store = InMemoryPolicyStore::new();
        let user = Address::repeat_byte(0x55);
        store.put(good_policy(user)).await.unwrap();
        store
            .set_status(user, PolicyStatus::Paused)
            .await
            .unwrap();
        let fetched = store.get(user).await.unwrap().unwrap();
        assert_eq!(fetched.status, PolicyStatus::Paused);
    }

    #[tokio::test]
    async fn set_status_errors_for_unknown_user() {
        let store = InMemoryPolicyStore::new();
        let user = Address::repeat_byte(0x66);
        assert!(store.set_status(user, PolicyStatus::Paused).await.is_err());
    }

    #[tokio::test]
    async fn get_returns_none_for_unknown_user() {
        let store = InMemoryPolicyStore::new();
        assert!(
            store
                .get(Address::repeat_byte(0x77))
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn list_active_users_filters_paused() {
        let store = InMemoryPolicyStore::new();
        let u1 = Address::repeat_byte(0x81);
        let u2 = Address::repeat_byte(0x82);
        let u3 = Address::repeat_byte(0x83);
        store.put(good_policy(u1)).await.unwrap();
        store.put(good_policy(u2)).await.unwrap();
        store.put(good_policy(u3)).await.unwrap();
        store
            .set_status(u3, PolicyStatus::Paused)
            .await
            .unwrap();

        let mut active = store.list_active_users().await.unwrap();
        active.sort();
        let mut expected = vec![u1, u2];
        expected.sort();
        assert_eq!(active, expected);
    }

    #[tokio::test]
    async fn list_active_users_empty_when_no_policies() {
        let store = InMemoryPolicyStore::new();
        assert!(store.list_active_users().await.unwrap().is_empty());
    }
}
