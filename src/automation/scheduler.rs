use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::json;
use tokio::task::JoinHandle;

use crate::automation::audit::{AuditStore, EventType, NewAuditEvent};
use crate::automation::evaluator::Decision;
use crate::automation::executor::execute_plan;
use crate::automation::policy::PolicyStore;
use crate::automation::position::{PositionFetcher, PositionStore};
use crate::automation::risk_gate::{self, GateDecision};
use crate::automation::snapshot::SnapshotStore;
use crate::automation::tick::tick_once;
use crate::state::AppState;

pub async fn run_cron_cycle(
    policies: &dyn PolicyStore,
    snapshots: &dyn SnapshotStore,
    positions: &dyn PositionStore,
    audit: &dyn AuditStore,
    fetcher: Option<&dyn PositionFetcher>,
    executor_state: Option<&AppState>,
) -> usize {
    let users = match policies.list_active_users().await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(error = %e, "failed to list active users");
            return 0;
        }
    };
    let now = Utc::now();
    let mut count = 0usize;
    for user in users {
        if let Some(f) = fetcher {
            match f.fetch(user).await {
                Ok(pos) => {
                    if let Err(e) = positions.put(user, pos).await {
                        tracing::warn!(user = %user, error = %e, "position put failed");
                    }
                }
                Err(e) => {
                    tracing::warn!(user = %user, error = %e, "position fetch failed; using stored")
                }
            }
        }
        let outcome =
            match tick_once(policies, snapshots, positions, audit, user, "cron", now).await {
                Ok(o) => {
                    count += 1;
                    o
                }
                Err(e) => {
                    tracing::error!(user = %user, error = %e, "tick failed");
                    continue;
                }
            };

        let Some(state) = executor_state else { continue };
        let Some(Decision::Act { plan }) = &outcome.decision else {
            continue;
        };
        let policy = match policies.get(user).await {
            Ok(Some(p)) => p,
            _ => continue,
        };
        match risk_gate::check(&policy, plan) {
            GateDecision::Pass => {
                if let Err(e) = execute_plan(state, user, policy.version, plan).await {
                    tracing::error!(user = %user, error = %e, "execute_plan failed");
                }
            }
            GateDecision::Reject { reason } => {
                tracing::warn!(user = %user, reason = %reason, "risk gate rejected plan");
                let _ = audit
                    .append(
                        NewAuditEvent::new(
                            user,
                            EventType::RiskGateDecision,
                            json!({ "decision": "reject", "reason": reason }),
                            Utc::now(),
                        )
                        .with_policy_version(policy.version),
                    )
                    .await;
            }
        }
    }
    count
}

pub async fn tick_user_now(state: &AppState, user: alloy::primitives::Address) {
    let now = Utc::now();

    match state.position_fetcher.fetch(user).await {
        Ok(pos) => {
            if let Err(e) = state.positions.put(user, pos).await {
                tracing::warn!(user = %user, error = %e, "[TICK_NOW] position put failed");
            }
        }
        Err(e) => {
            tracing::warn!(user = %user, error = %e, "[TICK_NOW] position fetch failed; using stored");
        }
    }

    let outcome = match tick_once(
        state.policies.as_ref(),
        state.snapshots.as_ref(),
        state.positions.as_ref(),
        state.audit.as_ref(),
        user,
        "policy_change",
        now,
    )
    .await
    {
        Ok(o) => o,
        Err(e) => {
            tracing::error!(user = %user, error = %e, "[TICK_NOW] tick failed");
            return;
        }
    };

    let Some(Decision::Act { plan }) = &outcome.decision else {
        tracing::info!(
            user = %user,
            decision = ?outcome.decision,
            skipped = ?outcome.skipped,
            "[TICK_NOW] no Act — engine left the position as-is"
        );
        return;
    };
    let policy = match state.policies.get(user).await {
        Ok(Some(p)) => p,
        _ => return,
    };
    match risk_gate::check(&policy, plan) {
        GateDecision::Pass => {
            if let Err(e) = execute_plan(state, user, policy.version, plan).await {
                tracing::error!(user = %user, error = %e, "[TICK_NOW] execute_plan failed");
            }
        }
        GateDecision::Reject { reason } => {
            tracing::warn!(user = %user, reason = %reason, "[TICK_NOW] risk gate rejected plan");
            let _ = state
                .audit
                .append(
                    NewAuditEvent::new(
                        user,
                        EventType::RiskGateDecision,
                        json!({ "decision": "reject", "reason": reason }),
                        Utc::now(),
                    )
                    .with_policy_version(policy.version),
                )
                .await;
        }
    }
}

pub fn spawn_cron(
    policies: Arc<dyn PolicyStore>,
    snapshots: Arc<dyn SnapshotStore>,
    positions: Arc<dyn PositionStore>,
    audit: Arc<dyn AuditStore>,
    fetcher: Option<Arc<dyn PositionFetcher>>,
    executor_state: Option<AppState>,
    interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await; // burn the immediate first tick
        loop {
            ticker.tick().await;
            let n = run_cron_cycle(
                &*policies,
                &*snapshots,
                &*positions,
                &*audit,
                fetcher.as_deref(),
                executor_state.as_ref(),
            )
            .await;
            tracing::info!(ticked = n, "cron cycle complete");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::audit::{EventType, InMemoryAuditStore};
    use crate::automation::evaluator::{GatewayHealth, Position, Snapshot, VenueState};
    use crate::automation::policy::{
        CapsConfig, ChainId, ChainsConfig, CircuitBreakersConfig, GasConfig,
        InMemoryPolicyStore, Policy, PolicyStatus, ProtocolId, ProtocolsConfig,
        TriggersConfig, VenueRef,
    };
    use crate::automation::position::InMemoryPositionStore;
    use crate::automation::snapshot::InMemorySnapshotStore;
    use alloy::primitives::{Address, U256};
    use chrono::DateTime;
    use std::collections::HashMap;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(2_000_000_000, 0).unwrap()
    }

    fn aave() -> VenueRef {
        VenueRef {
            chain: ChainId::ArbitrumSepolia,
            protocol: ProtocolId::AaveV3,
        }
    }
    fn idle_arc() -> VenueRef {
        VenueRef {
            chain: ChainId::Arc,
            protocol: ProtocolId::Idle,
        }
    }

    fn good_policy(user: Address) -> Policy {
        Policy {
            version: 1,
            user,
            risk_label: "balanced".into(),
            created_at: now(),
            compiled_from: None,
            status: PolicyStatus::Active,
            protocols: ProtocolsConfig {
                whitelist: vec![idle_arc(), aave()],
                per_protocol_cap_pct: 100,
            },
            chains: ChainsConfig {
                whitelist: vec![ChainId::Arc, ChainId::ArbitrumSepolia],
            },
            triggers: TriggersConfig {
                apr_delta_bps: 100,
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

    fn good_snapshot() -> Snapshot {
        let mut venues = HashMap::new();
        venues.insert(
            aave(),
            VenueState {
                apr: 0.05,
                apr_smoothed_1h: 0.05,
                utilization: 0.5,
                tvl_usd: 1_000_000.0,
                tvl_drop_pct_1h: 0.0,
            },
        );
        let mut gas = HashMap::new();
        gas.insert(ChainId::Arc, 0.50);
        gas.insert(ChainId::ArbitrumSepolia, 0.50);
        Snapshot {
            built_at: Utc::now(),
            usdc_usd: 1.0,
            gateway_health: GatewayHealth::Ok,
            venues,
            gas_usd_per_userop: gas,
            gateway_fee_usd: 0.10,
        }
    }

    #[tokio::test]
    async fn empty_cycle_is_noop() {
        let policies = InMemoryPolicyStore::new();
        let snapshots = InMemorySnapshotStore::new();
        let positions = InMemoryPositionStore::new();
        let audit = InMemoryAuditStore::new();
        let n = run_cron_cycle(&policies, &snapshots, &positions, &audit, None, None).await;
        assert_eq!(n, 0);
    }

    struct FakeFetcher(Position);

    #[async_trait::async_trait]
    impl PositionFetcher for FakeFetcher {
        async fn fetch(&self, _user: Address) -> anyhow::Result<Position> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn cycle_with_fetcher_writes_fresh_position() {
        let policies = InMemoryPolicyStore::new();
        let snapshots = InMemorySnapshotStore::new();
        let positions = InMemoryPositionStore::new();
        let audit = InMemoryAuditStore::new();

        let user = Address::repeat_byte(0xF1);
        policies.put(good_policy(user)).await.unwrap();
        snapshots.put(good_snapshot()).await.unwrap();

        let mut h = HashMap::new();
        h.insert(idle_arc(), U256::from(7_777_777_777u128));
        let fake = FakeFetcher(Position {
            holdings: h,
            ..Default::default()
        });

        // No position pre-seeded; fetcher provides it.
        let n =
            run_cron_cycle(&policies, &snapshots, &positions, &audit, Some(&fake), None).await;
        assert_eq!(n, 1);

        let stored = positions.get(user).await.unwrap().unwrap();
        assert_eq!(
            stored.holdings.get(&idle_arc()).copied(),
            Some(U256::from(7_777_777_777u128))
        );
    }

    #[tokio::test]
    async fn cycle_ticks_active_skips_paused() {
        let policies = InMemoryPolicyStore::new();
        let snapshots = InMemorySnapshotStore::new();
        let positions = InMemoryPositionStore::new();
        let audit = InMemoryAuditStore::new();

        let u_active1 = Address::repeat_byte(0xA1);
        let u_active2 = Address::repeat_byte(0xA2);
        let u_paused = Address::repeat_byte(0xB0);
        policies.put(good_policy(u_active1)).await.unwrap();
        policies.put(good_policy(u_active2)).await.unwrap();
        policies.put(good_policy(u_paused)).await.unwrap();
        policies
            .set_status(u_paused, PolicyStatus::Paused)
            .await
            .unwrap();

        snapshots.put(good_snapshot()).await.unwrap();
        let mut h = HashMap::new();
        h.insert(idle_arc(), U256::from(10_000_000_000u128));
        let pos = Position {
            holdings: h,
            ..Default::default()
        };
        positions.put(u_active1, pos.clone()).await.unwrap();
        positions.put(u_active2, pos).await.unwrap();

        let n = run_cron_cycle(&policies, &snapshots, &positions, &audit, None, None).await;
        assert_eq!(n, 2);

        let e1 = audit.list_for_user(u_active1, None, 20).await.unwrap();
        let e2 = audit.list_for_user(u_active2, None, 20).await.unwrap();
        let ep = audit.list_for_user(u_paused, None, 20).await.unwrap();
        assert_eq!(e1[0].event_type, EventType::EvaluatorDecision);
        assert_eq!(e1.last().unwrap().event_type, EventType::TriggerFired);
        assert_eq!(e2[0].event_type, EventType::EvaluatorDecision);
        assert_eq!(ep.len(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn spawn_cron_fires_after_interval() {
        let policies: Arc<dyn PolicyStore> = Arc::new(InMemoryPolicyStore::new());
        let snapshots: Arc<dyn SnapshotStore> = Arc::new(InMemorySnapshotStore::new());
        let positions: Arc<dyn PositionStore> = Arc::new(InMemoryPositionStore::new());
        let audit_store = Arc::new(InMemoryAuditStore::new());
        let audit: Arc<dyn AuditStore> = audit_store.clone();

        let user = Address::repeat_byte(0xC1);
        policies.put(good_policy(user)).await.unwrap();
        snapshots.put(good_snapshot()).await.unwrap();

        let handle = spawn_cron(
            policies.clone(),
            snapshots.clone(),
            positions.clone(),
            audit.clone(),
            None,
            None,
            Duration::from_secs(60),
        );

        assert_eq!(audit_store.list_for_user(user, None, 10).await.unwrap().len(), 0);

        tokio::time::advance(Duration::from_secs(61)).await;
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(61)).await;
        tokio::task::yield_now().await;

        let events = audit_store.list_for_user(user, None, 10).await.unwrap();
        assert!(events.len() >= 2, "expected ≥1 trigger+decision, got {}", events.len());

        handle.abort();
    }
}
