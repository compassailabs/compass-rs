pub mod types;

use std::cmp::Ordering;

use alloy::primitives::U256;
use chrono::{DateTime, Duration, Utc};

use crate::automation::policy::{ChainId, Policy, ProtocolId, VenueRef};

pub use types::{
    Action, ActionPlan, BreakReason, Decision, EscalateReason, EvaluationOutcome,
    EvaluatorThought, GatewayHealth, NoopReason, Position, Snapshot, VenueState,
};

const SNAPSHOT_MAX_AGE_MIN: i64 = 5;

pub fn evaluate(
    policy: &Policy,
    snapshot: &Snapshot,
    position: &Position,
    now: DateTime<Utc>,
) -> EvaluationOutcome {
    let mut thoughts: Vec<EvaluatorThought> = Vec::new();
    thoughts.push(audit_entry_thought(policy));
    thoughts.push(position_thought(position));

    if let Some(d) = check_snapshot_freshness(snapshot, now) {
        return finish(thoughts, d);
    }
    if let Some(d) = check_usdc_peg(policy, snapshot) {
        return finish(thoughts, d);
    }
    if let Some(d) = check_held_venue_health(policy, snapshot, position) {
        return finish(thoughts, d);
    }
    if snapshot.gateway_health == GatewayHealth::Down {
        return finish(
            thoughts,
            Decision::Noop {
                reason: NoopReason::GatewayDown,
            },
        );
    }
    if position.actions_today >= policy.caps.max_actions_per_day {
        return finish(
            thoughts,
            Decision::Noop {
                reason: NoopReason::DailyQuotaReached,
            },
        );
    }

    let total: U256 = sum_holdings(position);
    if !total.is_zero() {
        let candidates = rank_candidates(policy, snapshot);
        thoughts.push(scan_thought(&candidates));
        thoughts.push(best_opportunity_thought(&candidates));
    }

    let decision = propose_rebalance(policy, snapshot, position, now);
    finish(thoughts, decision)
}

fn finish(mut thoughts: Vec<EvaluatorThought>, decision: Decision) -> EvaluationOutcome {
    thoughts.push(session_complete_thought(&decision));
    EvaluationOutcome { thoughts, decision }
}

fn audit_entry_thought(policy: &Policy) -> EvaluatorThought {
    let chains: Vec<&str> = policy
        .chains
        .whitelist
        .iter()
        .map(|c| chain_name(*c))
        .collect();
    let n_venues = policy.protocols.whitelist.len();
    EvaluatorThought::new(format!(
        "Auditing markets across {} venue{} on {}.",
        n_venues,
        if n_venues == 1 { "" } else { "s" },
        chains.join(", "),
    ))
}

fn position_thought(position: &Position) -> EvaluatorThought {
    let total = sum_holdings(position);
    if total.is_zero() {
        return EvaluatorThought::new(
            "Wallet is empty — no capital to evaluate yet.".to_string(),
        );
    }
    let nonzero: Vec<&VenueRef> = position
        .holdings
        .iter()
        .filter(|(_, a)| !a.is_zero())
        .map(|(v, _)| v)
        .collect();
    let venue_str = if nonzero.len() == 1 {
        format!(
            "{} on {}",
            protocol_name(nonzero[0].protocol),
            chain_name(nonzero[0].chain),
        )
    } else {
        format!("{} venues", nonzero.len())
    };
    EvaluatorThought::new(format!(
        "Detected active position of {} USDC in {}.",
        format_usdc(total),
        venue_str,
    ))
}

fn scan_thought(candidates: &[(VenueRef, f64)]) -> EvaluatorThought {
    let names: Vec<String> = candidates
        .iter()
        .map(|(v, _)| {
            format!(
                "{} on {}",
                protocol_name(v.protocol),
                chain_name(v.chain),
            )
        })
        .collect();
    EvaluatorThought::new(format!(
        "Scanned APR across {} candidate venue{}: {}.",
        candidates.len(),
        if candidates.len() == 1 { "" } else { "s" },
        names.join(", "),
    ))
}

fn best_opportunity_thought(candidates: &[(VenueRef, f64)]) -> EvaluatorThought {
    let (best, apr) = &candidates[0];
    EvaluatorThought::new(format!(
        "Best opportunity: {} on {} at {:.2}% APR.",
        protocol_name(best.protocol),
        chain_name(best.chain),
        apr * 100.0,
    ))
}

fn session_complete_thought(decision: &Decision) -> EvaluatorThought {
    let tail = match decision {
        Decision::Act { .. } => "Execution plan ready.",
        Decision::Noop { .. } => "No action required.",
        Decision::CircuitBreak { .. } => "Engine paused for review.",
        Decision::Escalate { .. } => "Handing off to LLM.",
    };
    EvaluatorThought::new(format!("Session complete — {tail}"))
}

fn chain_name(c: ChainId) -> &'static str {
    match c {
        ChainId::Arc => "Arc",
        ChainId::ArbitrumSepolia => "Arbitrum Sepolia",
    }
}

fn protocol_name(p: ProtocolId) -> &'static str {
    match p {
        ProtocolId::Idle => "Wallet",
        ProtocolId::AaveV3 => "AAVE v3",
    }
}

fn sum_holdings(position: &Position) -> U256 {
    position
        .holdings
        .values()
        .copied()
        .fold(U256::ZERO, |a, b| a + b)
}

fn format_usdc(raw: U256) -> String {
    let whole = raw / U256::from(1_000_000u64);
    let frac = raw % U256::from(1_000_000u64);
    let frac_str = format!("{:06}", u64::try_from(frac).unwrap_or(0));
    format!("{whole}.{}", &frac_str[..2])
}

fn check_snapshot_freshness(snapshot: &Snapshot, now: DateTime<Utc>) -> Option<Decision> {
    let age = now.signed_duration_since(snapshot.built_at);
    if age > Duration::minutes(SNAPSHOT_MAX_AGE_MIN) {
        return Some(Decision::CircuitBreak {
            reason: BreakReason::SnapshotStale,
            drain_to: None,
        });
    }
    None
}

fn check_usdc_peg(policy: &Policy, snapshot: &Snapshot) -> Option<Decision> {
    if snapshot.usdc_usd < policy.circuit_breakers.usdc_peg_min {
        return Some(Decision::CircuitBreak {
            reason: BreakReason::UsdcDepeg {
                observed: snapshot.usdc_usd,
            },
            drain_to: Some(pick_idle(policy)),
        });
    }
    None
}

fn check_held_venue_health(policy: &Policy, snapshot: &Snapshot, position: &Position) -> Option<Decision> {
    for (venue, amount) in &position.holdings {
        if amount.is_zero() || venue.protocol == ProtocolId::Idle {
            continue;
        }
        let state = match snapshot.venues.get(venue) {
            Some(s) => s,
            None => {
                return Some(Decision::Escalate {
                    reason: EscalateReason::NewListingNoData {
                        venue: venue.clone(),
                    },
                });
            }
        };
        if state.utilization > policy.circuit_breakers.utilization_max {
            return Some(Decision::CircuitBreak {
                reason: BreakReason::VenueUnhealthy {
                    venue: venue.clone(),
                    signal: "utilization".into(),
                },
                drain_to: Some(pick_idle(policy)),
            });
        }
        if state.tvl_drop_pct_1h > policy.circuit_breakers.tvl_drop_pct_1h {
            return Some(Decision::CircuitBreak {
                reason: BreakReason::VenueUnhealthy {
                    venue: venue.clone(),
                    signal: "tvl_drop".into(),
                },
                drain_to: Some(pick_idle(policy)),
            });
        }
    }
    None
}

fn propose_rebalance(
    policy: &Policy,
    snapshot: &Snapshot,
    position: &Position,
    now: DateTime<Utc>,
) -> Decision {
    let total: U256 = position
        .holdings
        .values()
        .copied()
        .fold(U256::ZERO, |a, b| a + b);
    if total.is_zero() {
        return Decision::Noop {
            reason: NoopReason::NoCapital,
        };
    }

    let candidates = rank_candidates(policy, snapshot);
    let (best_venue, best_apr) = candidates[0].clone();

    let (worst_venue, worst_apr, worst_amount) = match worst_holding(policy, snapshot, position) {
        Some(t) => t,
        None => {
            return Decision::Noop {
                reason: NoopReason::NoCapital,
            };
        }
    };

    if worst_venue == best_venue {
        return Decision::Noop {
            reason: NoopReason::AlreadyAtBestVenue,
        };
    }

    let apr_delta = best_apr - worst_apr;
    let delta_bps = (apr_delta * 10_000.0).max(0.0) as u32;
    if delta_bps < policy.triggers.apr_delta_bps {
        return Decision::Noop {
            reason: NoopReason::AprDeltaBelowThreshold,
        };
    }

    if let Some(last) = position.last_action_at.get(&best_venue) {
        let elapsed = now.signed_duration_since(*last);
        if elapsed < Duration::minutes(policy.triggers.min_idle_minutes as i64) {
            return Decision::Noop {
                reason: NoopReason::DestinationInCooldown,
            };
        }
    }

    let max_move = mul_pct(total, policy.caps.max_move_pct_per_action);
    let best_current = position
        .holdings
        .get(&best_venue)
        .copied()
        .unwrap_or(U256::ZERO);
    let best_cap = mul_pct(total, policy.protocols.per_protocol_cap_pct);
    let best_room = best_cap.saturating_sub(best_current);
    if best_room.is_zero() {
        return Decision::Noop {
            reason: NoopReason::BestVenueAtCap,
        };
    }

    let move_amount = worst_amount.min(max_move).min(best_room);
    if move_amount.is_zero() {
        return Decision::Noop {
            reason: NoopReason::BestVenueAtCap,
        };
    }

    let move_amount_usd = u256_to_usdc_f64(move_amount) * snapshot.usdc_usd;
    let hold_days = policy.gas.estimated_hold_days as f64;
    let expected_profit_usd = move_amount_usd * apr_delta * (hold_days / 365.0);

    let from_gas = snapshot
        .gas_usd_per_userop
        .get(&worst_venue.chain)
        .copied()
        .unwrap_or(0.0);
    let to_gas = snapshot
        .gas_usd_per_userop
        .get(&best_venue.chain)
        .copied()
        .unwrap_or(0.0);
    let bridge_cost = if worst_venue.chain != best_venue.chain {
        snapshot.gateway_fee_usd
    } else {
        0.0
    };
    let estimated_cost_usd = from_gas + to_gas + bridge_cost;

    if estimated_cost_usd > policy.gas.max_gas_usd_per_action {
        return Decision::Noop {
            reason: NoopReason::GasExceedsCap,
        };
    }

    let net = expected_profit_usd - estimated_cost_usd;

    if policy.caps.min_net_profit_usd > 0.0 && net < policy.caps.min_net_profit_usd {
        return Decision::Noop {
            reason: NoopReason::EvBelowThreshold,
        };
    }

    Decision::Act {
        plan: ActionPlan {
            actions: vec![Action {
                from: worst_venue,
                to: best_venue,
                amount: move_amount,
            }],
            expected_profit_usd,
            estimated_cost_usd,
        },
    }
}

fn rank_candidates(policy: &Policy, snapshot: &Snapshot) -> Vec<(VenueRef, f64)> {
    let mut c: Vec<(VenueRef, f64)> = policy
        .protocols
        .whitelist
        .iter()
        .map(|v| {
            let apr = if v.protocol == ProtocolId::Idle {
                0.0
            } else {
                snapshot
                    .venues
                    .get(v)
                    .map(|s| s.apr_smoothed_1h)
                    .unwrap_or(0.0)
            };
            (v.clone(), apr)
        })
        .collect();
    c.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
    c
}

fn worst_holding(
    policy: &Policy,
    snapshot: &Snapshot,
    position: &Position,
) -> Option<(VenueRef, f64, U256)> {
    position
        .holdings
        .iter()
        .filter(|(_, amt)| !amt.is_zero())
        .filter(|(v, _)| {
            policy.protocols.whitelist.contains(v)
                || v.protocol == ProtocolId::Idle
        })
        .map(|(v, amt)| {
            let apr = if v.protocol == ProtocolId::Idle {
                0.0
            } else {
                snapshot
                    .venues
                    .get(v)
                    .map(|s| s.apr_smoothed_1h)
                    .unwrap_or(0.0)
            };
            (v.clone(), apr, *amt)
        })
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal))
}

fn pick_idle(policy: &Policy) -> VenueRef {
    policy
        .protocols
        .whitelist
        .iter()
        .find(|v| v.protocol == ProtocolId::Idle)
        .cloned()
        .unwrap_or_else(|| VenueRef {
            chain: policy.chains.whitelist[0],
            protocol: ProtocolId::Idle,
        })
}

fn mul_pct(amount: U256, pct: u8) -> U256 {
    amount * U256::from(pct) / U256::from(100u8)
}

fn u256_to_usdc_f64(raw: U256) -> f64 {
    let v: u128 = raw.try_into().unwrap_or(u128::MAX);
    v as f64 / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::policy::{
        CapsConfig, ChainId, ChainsConfig, CircuitBreakersConfig, GasConfig, Policy, PolicyStatus,
        ProtocolsConfig, TriggersConfig, VenueRef,
    };
    use alloy::primitives::Address;
    use std::collections::HashMap;

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
    fn idle_arb() -> VenueRef {
        VenueRef {
            chain: ChainId::ArbitrumSepolia,
            protocol: ProtocolId::Idle,
        }
    }

    fn good_policy() -> Policy {
        Policy {
            version: 1,
            user: Address::ZERO,
            risk_label: "balanced".into(),
            created_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            compiled_from: None,
            status: PolicyStatus::Active,
            protocols: ProtocolsConfig {
                whitelist: vec![idle_arc(), idle_arb(), aave()],
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

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(2_000_000_000, 0).unwrap()
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

    #[test]
    fn snapshot_stale_circuit_breaks() {
        let mut s = good_snapshot();
        s.built_at = now() - Duration::minutes(10);
        let d = evaluate(&good_policy(), &s, &position_idle(10_000_000_000), now()).decision;
        assert!(matches!(
            d,
            Decision::CircuitBreak {
                reason: BreakReason::SnapshotStale,
                ..
            }
        ));
    }

    #[test]
    fn usdc_depeg_drains_to_idle() {
        let mut s = good_snapshot();
        s.usdc_usd = 0.95;
        let d = evaluate(&good_policy(), &s, &position_idle(10_000_000_000), now()).decision;
        match d {
            Decision::CircuitBreak {
                reason: BreakReason::UsdcDepeg { observed },
                drain_to,
            } => {
                assert!((observed - 0.95).abs() < 1e-9);
                assert_eq!(drain_to.unwrap().protocol, ProtocolId::Idle);
            }
            other => panic!("expected CircuitBreak UsdcDepeg, got {other:?}"),
        }
    }

    #[test]
    fn utilization_breaks_for_held_venue() {
        let mut s = good_snapshot();
        s.venues.get_mut(&aave()).unwrap().utilization = 0.99;
        let mut h = HashMap::new();
        h.insert(aave(), U256::from(5_000_000_000u128));
        let pos = Position {
            holdings: h,
            ..Default::default()
        };
        let d = evaluate(&good_policy(), &s, &pos, now()).decision;
        assert!(matches!(
            d,
            Decision::CircuitBreak {
                reason: BreakReason::VenueUnhealthy { signal, .. },
                ..
            } if signal == "utilization"
        ));
    }

    #[test]
    fn tvl_drop_breaks_for_held_venue() {
        let mut s = good_snapshot();
        s.venues.get_mut(&aave()).unwrap().tvl_drop_pct_1h = 50.0;
        let mut h = HashMap::new();
        h.insert(aave(), U256::from(5_000_000_000u128));
        let pos = Position {
            holdings: h,
            ..Default::default()
        };
        let d = evaluate(&good_policy(), &s, &pos, now()).decision;
        assert!(matches!(
            d,
            Decision::CircuitBreak {
                reason: BreakReason::VenueUnhealthy { signal, .. },
                ..
            } if signal == "tvl_drop"
        ));
    }

    #[test]
    fn missing_held_venue_data_escalates() {
        let mut s = good_snapshot();
        s.venues.clear();
        let mut h = HashMap::new();
        h.insert(aave(), U256::from(5_000_000_000u128));
        let pos = Position {
            holdings: h,
            ..Default::default()
        };
        let d = evaluate(&good_policy(), &s, &pos, now()).decision;
        assert!(matches!(
            d,
            Decision::Escalate {
                reason: EscalateReason::NewListingNoData { .. }
            }
        ));
    }

    #[test]
    fn gateway_down_yields_noop() {
        let mut s = good_snapshot();
        s.gateway_health = GatewayHealth::Down;
        let d = evaluate(&good_policy(), &s, &position_idle(10_000_000_000), now()).decision;
        assert!(matches!(
            d,
            Decision::Noop {
                reason: NoopReason::GatewayDown
            }
        ));
    }

    #[test]
    fn daily_quota_yields_noop() {
        let mut pos = position_idle(10_000_000_000);
        pos.actions_today = 6;
        let d = evaluate(&good_policy(), &good_snapshot(), &pos, now()).decision;
        assert!(matches!(
            d,
            Decision::Noop {
                reason: NoopReason::DailyQuotaReached
            }
        ));
    }

    #[test]
    fn no_capital_yields_noop() {
        let pos = Position::default();
        let d = evaluate(&good_policy(), &good_snapshot(), &pos, now()).decision;
        assert!(matches!(
            d,
            Decision::Noop {
                reason: NoopReason::NoCapital
            }
        ));
    }

    #[test]
    fn happy_path_idle_to_aave() {
        let d = evaluate(
            &good_policy(),
            &good_snapshot(),
            &position_idle(10_000_000_000),
            now(),
        )
        .decision;
        match d {
            Decision::Act { plan } => {
                assert_eq!(plan.actions.len(), 1);
                let a = &plan.actions[0];
                assert_eq!(a.from, idle_arc());
                assert_eq!(a.to, aave());
                assert_eq!(a.amount, U256::from(10_000_000_000u128));
                assert!(plan.expected_profit_usd > plan.estimated_cost_usd);
            }
            other => panic!("expected Act, got {other:?}"),
        }
    }

    #[test]
    fn already_at_best_venue_noop() {
        let mut h = HashMap::new();
        h.insert(aave(), U256::from(10_000_000_000u128));
        let pos = Position {
            holdings: h,
            ..Default::default()
        };
        let d = evaluate(&good_policy(), &good_snapshot(), &pos, now()).decision;
        assert!(matches!(
            d,
            Decision::Noop {
                reason: NoopReason::AlreadyAtBestVenue
            }
        ));
    }

    #[test]
    fn apr_delta_below_threshold_noop() {
        let mut s = good_snapshot();
        s.venues.get_mut(&aave()).unwrap().apr_smoothed_1h = 0.005;
        let mut p = good_policy();
        p.triggers.apr_delta_bps = 100;
        let d = evaluate(&p, &s, &position_idle(10_000_000_000), now()).decision;
        assert!(matches!(
            d,
            Decision::Noop {
                reason: NoopReason::AprDeltaBelowThreshold
            }
        ));
    }

    #[test]
    fn cooldown_blocks_move() {
        let mut pos = position_idle(10_000_000_000);
        pos.last_action_at
            .insert(aave(), now() - Duration::minutes(10));
        let d = evaluate(&good_policy(), &good_snapshot(), &pos, now()).decision;
        assert!(matches!(
            d,
            Decision::Noop {
                reason: NoopReason::DestinationInCooldown
            }
        ));
    }

    #[test]
    fn cooldown_elapsed_allows_move() {
        let mut pos = position_idle(10_000_000_000);
        pos.last_action_at
            .insert(aave(), now() - Duration::minutes(60));
        let d = evaluate(&good_policy(), &good_snapshot(), &pos, now()).decision;
        assert!(matches!(d, Decision::Act { .. }));
    }

    #[test]
    fn best_venue_at_cap_noop() {
        let mut p = good_policy();
        p.protocols.per_protocol_cap_pct = 50;
        // 50 × 3 venues = 150 ≥ 100, satisfies validation
        let mut h = HashMap::new();
        h.insert(idle_arc(), U256::from(5_000_000_000u128));
        h.insert(aave(), U256::from(5_000_000_000u128));
        let pos = Position {
            holdings: h,
            ..Default::default()
        };
        // aave already at 50% cap → no room
        let d = evaluate(&p, &good_snapshot(), &pos, now()).decision;
        assert!(matches!(
            d,
            Decision::Noop {
                reason: NoopReason::BestVenueAtCap
            }
        ));
    }

    #[test]
    fn max_move_pct_caps_action_size() {
        let mut p = good_policy();
        p.caps.max_move_pct_per_action = 25;
        let d = evaluate(&p, &good_snapshot(), &position_idle(10_000_000_000), now()).decision;
        match d {
            Decision::Act { plan } => {
                assert_eq!(plan.actions[0].amount, U256::from(2_500_000_000u128));
            }
            other => panic!("expected Act, got {other:?}"),
        }
    }

    #[test]
    fn gas_exceeding_cap_noop() {
        let mut s = good_snapshot();
        s.gas_usd_per_userop.insert(ChainId::ArbitrumSepolia, 10.0);
        let d = evaluate(&good_policy(), &s, &position_idle(10_000_000_000), now()).decision;
        assert!(matches!(
            d,
            Decision::Noop {
                reason: NoopReason::GasExceedsCap
            }
        ));
    }

    #[test]
    fn ev_below_threshold_noop() {
        let mut p = good_policy();
        p.caps.min_net_profit_usd = 1_000.0;
        let d = evaluate(&p, &good_snapshot(), &position_idle(10_000_000_000), now()).decision;
        assert!(matches!(
            d,
            Decision::Noop {
                reason: NoopReason::EvBelowThreshold
            }
        ));
    }

    #[test]
    fn cross_chain_move_includes_bridge_fee() {
        // idle on Arc → AAVE on ArbitrumSepolia; bridge fee should be added.
        let d = evaluate(
            &good_policy(),
            &good_snapshot(),
            &position_idle(10_000_000_000),
            now(),
        )
        .decision;
        match d {
            Decision::Act { plan } => {
                // gas_per_chain 0.50 each + bridge 0.10 = 1.10 expected
                assert!((plan.estimated_cost_usd - 1.10).abs() < 1e-9);
            }
            other => panic!("expected Act, got {other:?}"),
        }
    }

    #[test]
    fn same_chain_move_no_bridge_fee() {
        let mut s = good_snapshot();
        let aave_arc = VenueRef {
            chain: ChainId::Arc,
            protocol: ProtocolId::AaveV3,
        };
        s.venues.insert(
            aave_arc.clone(),
            VenueState {
                apr: 0.05,
                apr_smoothed_1h: 0.05,
                utilization: 0.5,
                tvl_usd: 1_000_000.0,
                tvl_drop_pct_1h: 0.0,
            },
        );
        s.venues.remove(&aave());
        let mut p = good_policy();
        p.protocols.whitelist = vec![idle_arc(), aave_arc];
        let d = evaluate(&p, &s, &position_idle(10_000_000_000), now()).decision;
        match d {
            Decision::Act { plan } => {
                assert!((plan.estimated_cost_usd - 1.0).abs() < 1e-9);
            }
            other => panic!("expected Act, got {other:?}"),
        }
    }
}
