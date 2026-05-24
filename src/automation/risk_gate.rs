use serde::Serialize;

use crate::automation::evaluator::ActionPlan;
use crate::automation::policy::{Policy, PolicyStatus};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GateDecision {
    Pass,
    Reject { reason: String },
}

pub fn check(policy: &Policy, plan: &ActionPlan) -> GateDecision {
    if policy.status == PolicyStatus::Paused {
        return GateDecision::Reject {
            reason: "policy is paused".into(),
        };
    }
    if plan.actions.is_empty() {
        return GateDecision::Reject {
            reason: "plan has no actions".into(),
        };
    }
    if plan.estimated_cost_usd > policy.gas.max_gas_usd_per_action {
        return GateDecision::Reject {
            reason: format!(
                "estimated cost ${:.2} exceeds policy max_gas_usd_per_action ${:.2}",
                plan.estimated_cost_usd, policy.gas.max_gas_usd_per_action
            ),
        };
    }
    GateDecision::Pass
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::evaluator::{Action, ActionPlan};
    use crate::automation::policy::{
        CapsConfig, ChainId, ChainsConfig, CircuitBreakersConfig, GasConfig, Policy,
        ProtocolId, ProtocolsConfig, TriggersConfig, VenueRef,
    };
    use alloy::primitives::{Address, U256};
    use chrono::Utc;

    fn aave() -> VenueRef {
        VenueRef {
            chain: ChainId::ArbitrumSepolia,
            protocol: ProtocolId::AaveV3,
        }
    }
    fn idle() -> VenueRef {
        VenueRef {
            chain: ChainId::ArbitrumSepolia,
            protocol: ProtocolId::Idle,
        }
    }

    fn policy(status: PolicyStatus, max_gas: f64) -> Policy {
        Policy {
            version: 1,
            user: Address::ZERO,
            risk_label: "balanced".into(),
            created_at: Utc::now(),
            compiled_from: None,
            status,
            protocols: ProtocolsConfig {
                whitelist: vec![idle(), aave()],
                per_protocol_cap_pct: 100,
            },
            chains: ChainsConfig {
                whitelist: vec![ChainId::ArbitrumSepolia],
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
                max_gas_usd_per_action: max_gas,
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

    fn plan(cost: f64) -> ActionPlan {
        ActionPlan {
            actions: vec![Action {
                from: idle(),
                to: aave(),
                amount: U256::from(1_000_000u128),
            }],
            expected_profit_usd: 10.0,
            estimated_cost_usd: cost,
        }
    }

    #[test]
    fn happy_path_passes() {
        assert_eq!(
            check(&policy(PolicyStatus::Active, 5.0), &plan(2.0)),
            GateDecision::Pass
        );
    }

    #[test]
    fn paused_rejected() {
        let d = check(&policy(PolicyStatus::Paused, 5.0), &plan(2.0));
        match d {
            GateDecision::Reject { reason } => assert!(reason.contains("paused")),
            _ => panic!("expected reject"),
        }
    }

    #[test]
    fn empty_plan_rejected() {
        let mut p = plan(2.0);
        p.actions.clear();
        let d = check(&policy(PolicyStatus::Active, 5.0), &p);
        match d {
            GateDecision::Reject { reason } => assert!(reason.contains("no actions")),
            _ => panic!("expected reject"),
        }
    }

    #[test]
    fn gas_over_cap_rejected() {
        let d = check(&policy(PolicyStatus::Active, 5.0), &plan(10.0));
        match d {
            GateDecision::Reject { reason } => assert!(reason.contains("exceeds")),
            _ => panic!("expected reject"),
        }
    }
}
