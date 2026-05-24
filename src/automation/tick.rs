use alloy::primitives::Address;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;

use crate::automation::audit::{AuditStore, EventType, NewAuditEvent};
use crate::automation::evaluator::{Decision, Position, evaluate};
use crate::automation::policy::{PolicyStatus, PolicyStore};
use crate::automation::position::PositionStore;
use crate::automation::snapshot::SnapshotStore;

pub async fn tick_once(
    policies: &dyn PolicyStore,
    snapshots: &dyn SnapshotStore,
    positions: &dyn PositionStore,
    audit: &dyn AuditStore,
    user: Address,
    trigger_kind: &str,
    now: DateTime<Utc>,
) -> Result<TickOutcome> {
    let trigger_id = audit
        .append(NewAuditEvent::new(
            user,
            EventType::TriggerFired,
            json!({ "trigger_kind": trigger_kind }),
            now,
        ))
        .await?;

    let Some(policy) = policies.get(user).await? else {
        return Ok(TickOutcome {
            trigger_event_id: trigger_id,
            skipped: Some(SkipReason::NoPolicy),
            decision: None,
            decision_event_id: None,
        });
    };
    if policy.status == PolicyStatus::Paused {
        return Ok(TickOutcome {
            trigger_event_id: trigger_id,
            skipped: Some(SkipReason::Paused),
            decision: None,
            decision_event_id: None,
        });
    }

    let Some(snapshot) = snapshots.latest().await? else {
        return Ok(TickOutcome {
            trigger_event_id: trigger_id,
            skipped: Some(SkipReason::NoSnapshot),
            decision: None,
            decision_event_id: None,
        });
    };

    let position = positions.get(user).await?.unwrap_or_default();
    let outcome = evaluate(&policy, &snapshot, &position, now);

    for (i, thought) in outcome.thoughts.iter().enumerate() {
        let ts = now + chrono::Duration::milliseconds(i as i64 + 1);
        audit
            .append(
                NewAuditEvent::new(
                    user,
                    EventType::EvaluatorThought,
                    serde_json::to_value(thought)?,
                    ts,
                )
                .with_policy_version(policy.version),
            )
            .await?;
    }

    let decision_ts = now
        + chrono::Duration::milliseconds(outcome.thoughts.len() as i64 + 1);
    let decision_id = audit
        .append(
            NewAuditEvent::new(
                user,
                EventType::EvaluatorDecision,
                serde_json::to_value(&outcome.decision)?,
                decision_ts,
            )
            .with_policy_version(policy.version),
        )
        .await?;
    let decision = outcome.decision;

    Ok(TickOutcome {
        trigger_event_id: trigger_id,
        skipped: None,
        decision: Some(decision),
        decision_event_id: Some(decision_id),
    })
}

#[derive(Debug, Serialize)]
pub struct TickOutcome {
    pub trigger_event_id: u64,
    pub skipped: Option<SkipReason>,
    pub decision: Option<Decision>,
    pub decision_event_id: Option<u64>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    NoPolicy,
    Paused,
    NoSnapshot,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::audit::InMemoryAuditStore;
    use crate::automation::evaluator::{GatewayHealth, Snapshot, VenueState};
    use crate::automation::policy::{
        CapsConfig, ChainId, ChainsConfig, CircuitBreakersConfig, GasConfig,
        InMemoryPolicyStore, Policy, ProtocolId, ProtocolsConfig, TriggersConfig, VenueRef,
    };
    use crate::automation::position::InMemoryPositionStore;
    use crate::automation::snapshot::InMemorySnapshotStore;
    use alloy::primitives::U256;
    use chrono::Duration;
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
            built_at: now(),
            usdc_usd: 1.0,
            gateway_health: GatewayHealth::Ok,
            venues,
            gas_usd_per_userop: gas,
            gateway_fee_usd: 0.10,
        }
    }

    fn position_idle(usdc: u128) -> Position {
        let mut h = HashMap::new();
        h.insert(idle_arc(), U256::from(usdc));
        Position {
            holdings: h,
            ..Default::default()
        }
    }

    struct Harness {
        policies: InMemoryPolicyStore,
        snapshots: InMemorySnapshotStore,
        positions: InMemoryPositionStore,
        audit: InMemoryAuditStore,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                policies: InMemoryPolicyStore::new(),
                snapshots: InMemorySnapshotStore::new(),
                positions: InMemoryPositionStore::new(),
                audit: InMemoryAuditStore::new(),
            }
        }
    }

    #[tokio::test]
    async fn no_policy_skips_with_only_trigger_audit() {
        let h = Harness::new();
        let user = Address::repeat_byte(0xAA);
        let out = tick_once(&h.policies, &h.snapshots, &h.positions, &h.audit, user, "cron", now())
            .await
            .unwrap();
        assert_eq!(out.skipped, Some(SkipReason::NoPolicy));
        assert!(out.decision.is_none());
        assert_eq!(out.decision_event_id, None);

        let events = h.audit.list_for_user(user, None, 10).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, EventType::TriggerFired);
    }

    #[tokio::test]
    async fn paused_policy_skips() {
        let h = Harness::new();
        let user = Address::repeat_byte(0xBB);
        let mut p = good_policy(user);
        p.status = PolicyStatus::Paused;
        h.policies.put(p).await.unwrap();
        h.snapshots.put(good_snapshot()).await.unwrap();

        let out = tick_once(&h.policies, &h.snapshots, &h.positions, &h.audit, user, "cron", now())
            .await
            .unwrap();
        assert_eq!(out.skipped, Some(SkipReason::Paused));
        assert!(out.decision.is_none());
    }

    #[tokio::test]
    async fn no_snapshot_skips() {
        let h = Harness::new();
        let user = Address::repeat_byte(0xCC);
        h.policies.put(good_policy(user)).await.unwrap();

        let out = tick_once(&h.policies, &h.snapshots, &h.positions, &h.audit, user, "cron", now())
            .await
            .unwrap();
        assert_eq!(out.skipped, Some(SkipReason::NoSnapshot));
    }

    #[tokio::test]
    async fn happy_path_writes_trigger_and_decision() {
        let h = Harness::new();
        let user = Address::repeat_byte(0xDD);
        h.policies.put(good_policy(user)).await.unwrap();
        h.snapshots.put(good_snapshot()).await.unwrap();
        h.positions
            .put(user, position_idle(10_000_000_000))
            .await
            .unwrap();

        let out = tick_once(&h.policies, &h.snapshots, &h.positions, &h.audit, user, "cron", now())
            .await
            .unwrap();
        assert!(out.skipped.is_none());
        assert!(matches!(out.decision, Some(Decision::Act { .. })));

        let events = h.audit.list_for_user(user, None, 20).await.unwrap();
        assert_eq!(events[0].event_type, EventType::EvaluatorDecision);
        assert_eq!(events[0].policy_version, Some(1));
        assert_eq!(
            events.last().unwrap().event_type,
            EventType::TriggerFired
        );
        let thoughts = events
            .iter()
            .filter(|e| e.event_type == EventType::EvaluatorThought)
            .count();
        assert!(thoughts >= 2, "expected ≥2 thoughts, got {thoughts}");
    }

    #[tokio::test]
    async fn missing_position_treated_as_empty_yields_no_capital_noop() {
        let h = Harness::new();
        let user = Address::repeat_byte(0xEE);
        h.policies.put(good_policy(user)).await.unwrap();
        h.snapshots.put(good_snapshot()).await.unwrap();
        // no position written

        let out = tick_once(&h.policies, &h.snapshots, &h.positions, &h.audit, user, "cron", now())
            .await
            .unwrap();
        match out.decision {
            Some(Decision::Noop { reason }) => {
                assert_eq!(
                    serde_json::to_string(&reason).unwrap(),
                    "\"no_capital\""
                );
            }
            other => panic!("expected Noop NoCapital, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stale_snapshot_records_circuit_break() {
        let h = Harness::new();
        let user = Address::repeat_byte(0xFF);
        h.policies.put(good_policy(user)).await.unwrap();
        let mut s = good_snapshot();
        s.built_at = now() - Duration::minutes(10);
        h.snapshots.put(s).await.unwrap();
        h.positions
            .put(user, position_idle(10_000_000_000))
            .await
            .unwrap();

        let out = tick_once(&h.policies, &h.snapshots, &h.positions, &h.audit, user, "cron", now())
            .await
            .unwrap();
        assert!(matches!(
            out.decision,
            Some(Decision::CircuitBreak { .. })
        ));
        let events = h.audit.list_for_user(user, None, 20).await.unwrap();
        assert_eq!(events[0].event_type, EventType::EvaluatorDecision);
        assert_eq!(
            events.last().unwrap().event_type,
            EventType::TriggerFired
        );
    }
}
