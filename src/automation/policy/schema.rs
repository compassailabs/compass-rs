use std::collections::HashSet;

use alloy::primitives::Address;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub version: u32,
    pub user: Address,
    pub risk_label: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub compiled_from: Option<String>,
    #[serde(default)]
    pub status: PolicyStatus,
    pub protocols: ProtocolsConfig,
    pub chains: ChainsConfig,
    pub triggers: TriggersConfig,
    pub caps: CapsConfig,
    pub gas: GasConfig,
    pub circuit_breakers: CircuitBreakersConfig,
    #[serde(default)]
    pub notifications: Option<NotificationsConfig>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyStatus {
    #[default]
    Active,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolsConfig {
    pub whitelist: Vec<VenueRef>,
    pub per_protocol_cap_pct: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VenueRef {
    pub chain: ChainId,
    pub protocol: ProtocolId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainId {
    Arc,
    ArbitrumSepolia,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolId {
    Idle,
    AaveV3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainsConfig {
    pub whitelist: Vec<ChainId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggersConfig {
    pub apr_delta_bps: u32,
    pub apr_lookback_minutes: u32,
    pub min_idle_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsConfig {
    pub max_move_pct_per_action: u8,
    pub max_actions_per_day: u32,
    pub min_net_profit_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasConfig {
    pub estimated_hold_days: u32,
    pub max_gas_usd_per_action: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakersConfig {
    pub usdc_peg_min: f64,
    pub utilization_max: f64,
    pub tvl_drop_pct_1h: f64,
    #[serde(default)]
    pub protocol_blacklist_on_event: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationsConfig {
    pub webhook_url: String,
    pub on: Vec<NotificationEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationEvent {
    Action,
    CircuitBreak,
    Escalate,
}

#[derive(Debug, Error, PartialEq)]
pub enum ValidationError {
    #[error("protocols.whitelist is empty")]
    EmptyWhitelist,
    #[error("chains.whitelist is empty")]
    EmptyChainsWhitelist,
    #[error("per_protocol_cap_pct must be in 1..=100, got {0}")]
    InvalidProtocolCapPct(u8),
    #[error("max_move_pct_per_action must be in 1..=100, got {0}")]
    InvalidMoveCapPct(u8),
    #[error(
        "per_protocol_cap_pct ({pct}%) × whitelist size ({n}) = {total}% < 100%; \
         capital cannot be fully placed"
    )]
    UnsatisfiableCap { pct: u8, n: usize, total: u32 },
    #[error("triggers.apr_delta_bps must be ≥ 10 (got {0})")]
    AprDeltaTooSmall(u32),
    #[error("triggers.apr_lookback_minutes must be > 0")]
    ZeroLookback,
    #[error("triggers.min_idle_minutes must be > 0")]
    ZeroMinIdle,
    #[error("caps.max_actions_per_day must be > 0")]
    ZeroMaxActions,
    #[error("caps.min_net_profit_usd must be a finite, non-negative number (got {0})")]
    InvalidMinProfit(f64),
    #[error("gas.estimated_hold_days must be > 0")]
    ZeroHoldDays,
    #[error("gas.max_gas_usd_per_action must be a finite positive number (got {0})")]
    InvalidMaxGas(f64),
    #[error("circuit_breakers.usdc_peg_min must be in (0, 1] (got {0})")]
    InvalidPegMin(f64),
    #[error("circuit_breakers.utilization_max must be in (0, 1] (got {0})")]
    InvalidUtilizationMax(f64),
    #[error("circuit_breakers.tvl_drop_pct_1h must be in (0, 100] (got {0})")]
    InvalidTvlDropPct(f64),
    #[error("venue {chain:?}/{protocol:?} references a chain not in chains.whitelist")]
    VenueChainNotWhitelisted {
        chain: ChainId,
        protocol: ProtocolId,
    },
    #[error("duplicate venue in whitelist: {chain:?}/{protocol:?}")]
    DuplicateVenue {
        chain: ChainId,
        protocol: ProtocolId,
    },
}

impl Policy {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.validate_protocols()?;
        self.validate_chains_whitelist()?;
        self.validate_venues()?;
        self.validate_triggers()?;
        self.validate_caps()?;
        self.validate_gas()?;
        self.validate_circuit_breakers()?;
        Ok(())
    }

    fn validate_protocols(&self) -> Result<(), ValidationError> {
        let p = &self.protocols;
        if p.whitelist.is_empty() {
            return Err(ValidationError::EmptyWhitelist);
        }
        if p.per_protocol_cap_pct == 0 || p.per_protocol_cap_pct > 100 {
            return Err(ValidationError::InvalidProtocolCapPct(
                p.per_protocol_cap_pct,
            ));
        }
        let total = u32::from(p.per_protocol_cap_pct) * p.whitelist.len() as u32;
        if total < 100 {
            return Err(ValidationError::UnsatisfiableCap {
                pct: p.per_protocol_cap_pct,
                n: p.whitelist.len(),
                total,
            });
        }
        Ok(())
    }

    fn validate_chains_whitelist(&self) -> Result<(), ValidationError> {
        if self.chains.whitelist.is_empty() {
            return Err(ValidationError::EmptyChainsWhitelist);
        }
        Ok(())
    }

    fn validate_venues(&self) -> Result<(), ValidationError> {
        let mut seen = HashSet::new();
        for v in &self.protocols.whitelist {
            if !seen.insert(v.clone()) {
                return Err(ValidationError::DuplicateVenue {
                    chain: v.chain,
                    protocol: v.protocol,
                });
            }
            if !self.chains.whitelist.contains(&v.chain) {
                return Err(ValidationError::VenueChainNotWhitelisted {
                    chain: v.chain,
                    protocol: v.protocol,
                });
            }
        }
        Ok(())
    }

    fn validate_triggers(&self) -> Result<(), ValidationError> {
        let t = &self.triggers;
        if t.apr_delta_bps < 10 {
            return Err(ValidationError::AprDeltaTooSmall(t.apr_delta_bps));
        }
        if t.apr_lookback_minutes == 0 {
            return Err(ValidationError::ZeroLookback);
        }
        if t.min_idle_minutes == 0 {
            return Err(ValidationError::ZeroMinIdle);
        }
        Ok(())
    }

    fn validate_caps(&self) -> Result<(), ValidationError> {
        let c = &self.caps;
        if c.max_move_pct_per_action == 0 || c.max_move_pct_per_action > 100 {
            return Err(ValidationError::InvalidMoveCapPct(c.max_move_pct_per_action));
        }
        if c.max_actions_per_day == 0 {
            return Err(ValidationError::ZeroMaxActions);
        }
        if !c.min_net_profit_usd.is_finite() || c.min_net_profit_usd < 0.0 {
            return Err(ValidationError::InvalidMinProfit(c.min_net_profit_usd));
        }
        Ok(())
    }

    fn validate_gas(&self) -> Result<(), ValidationError> {
        let g = &self.gas;
        if g.estimated_hold_days == 0 {
            return Err(ValidationError::ZeroHoldDays);
        }
        if !g.max_gas_usd_per_action.is_finite() || g.max_gas_usd_per_action <= 0.0 {
            return Err(ValidationError::InvalidMaxGas(g.max_gas_usd_per_action));
        }
        Ok(())
    }

    fn validate_circuit_breakers(&self) -> Result<(), ValidationError> {
        let cb = &self.circuit_breakers;
        if !cb.usdc_peg_min.is_finite() || cb.usdc_peg_min <= 0.0 || cb.usdc_peg_min > 1.0 {
            return Err(ValidationError::InvalidPegMin(cb.usdc_peg_min));
        }
        if !cb.utilization_max.is_finite()
            || cb.utilization_max <= 0.0
            || cb.utilization_max > 1.0
        {
            return Err(ValidationError::InvalidUtilizationMax(cb.utilization_max));
        }
        if !cb.tvl_drop_pct_1h.is_finite()
            || cb.tvl_drop_pct_1h <= 0.0
            || cb.tvl_drop_pct_1h > 100.0
        {
            return Err(ValidationError::InvalidTvlDropPct(cb.tvl_drop_pct_1h));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good_policy() -> Policy {
        Policy {
            version: 0,
            user: Address::ZERO,
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
                apr_delta_bps: 100,
                apr_lookback_minutes: 60,
                min_idle_minutes: 30,
            },
            caps: CapsConfig {
                max_move_pct_per_action: 100,
                max_actions_per_day: 6,
                min_net_profit_usd: 1.0,
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

    #[test]
    fn baseline_is_valid() {
        assert!(good_policy().validate().is_ok());
    }

    #[test]
    fn empty_whitelist_rejected() {
        let mut p = good_policy();
        p.protocols.whitelist.clear();
        assert_eq!(p.validate(), Err(ValidationError::EmptyWhitelist));
    }

    #[test]
    fn empty_chains_rejected() {
        let mut p = good_policy();
        p.chains.whitelist.clear();
        assert_eq!(p.validate(), Err(ValidationError::EmptyChainsWhitelist));
    }

    #[test]
    fn protocol_cap_out_of_range() {
        let mut p = good_policy();
        p.protocols.per_protocol_cap_pct = 0;
        assert_eq!(p.validate(), Err(ValidationError::InvalidProtocolCapPct(0)));
        p.protocols.per_protocol_cap_pct = 101;
        assert_eq!(
            p.validate(),
            Err(ValidationError::InvalidProtocolCapPct(101))
        );
    }

    #[test]
    fn unsatisfiable_cap_rejected() {
        let mut p = good_policy();
        p.protocols.per_protocol_cap_pct = 30;
        // 30 * 2 = 60 < 100
        assert_eq!(
            p.validate(),
            Err(ValidationError::UnsatisfiableCap {
                pct: 30,
                n: 2,
                total: 60,
            })
        );
    }

    #[test]
    fn unsatisfiable_cap_passes_when_count_makes_up() {
        let mut p = good_policy();
        p.protocols.per_protocol_cap_pct = 50;
        // 50 * 2 = 100 ≥ 100
        assert!(p.validate().is_ok());
    }

    #[test]
    fn duplicate_venue_rejected() {
        let mut p = good_policy();
        p.protocols.whitelist.push(VenueRef {
            chain: ChainId::Arc,
            protocol: ProtocolId::Idle,
        });
        assert_eq!(
            p.validate(),
            Err(ValidationError::DuplicateVenue {
                chain: ChainId::Arc,
                protocol: ProtocolId::Idle,
            })
        );
    }

    #[test]
    fn venue_chain_not_whitelisted() {
        let mut p = good_policy();
        p.chains.whitelist = vec![ChainId::Arc];
        // arbitrum_sepolia venue still in whitelist but chain not allowed
        assert_eq!(
            p.validate(),
            Err(ValidationError::VenueChainNotWhitelisted {
                chain: ChainId::ArbitrumSepolia,
                protocol: ProtocolId::AaveV3,
            })
        );
    }

    #[test]
    fn apr_delta_too_small() {
        let mut p = good_policy();
        p.triggers.apr_delta_bps = 5;
        assert_eq!(p.validate(), Err(ValidationError::AprDeltaTooSmall(5)));
    }

    #[test]
    fn zero_lookback_and_idle_rejected() {
        let mut p = good_policy();
        p.triggers.apr_lookback_minutes = 0;
        assert_eq!(p.validate(), Err(ValidationError::ZeroLookback));
        p = good_policy();
        p.triggers.min_idle_minutes = 0;
        assert_eq!(p.validate(), Err(ValidationError::ZeroMinIdle));
    }

    #[test]
    fn caps_validation() {
        let mut p = good_policy();
        p.caps.max_move_pct_per_action = 0;
        assert_eq!(p.validate(), Err(ValidationError::InvalidMoveCapPct(0)));
        p = good_policy();
        p.caps.max_actions_per_day = 0;
        assert_eq!(p.validate(), Err(ValidationError::ZeroMaxActions));
        p = good_policy();
        p.caps.min_net_profit_usd = -1.0;
        assert_eq!(p.validate(), Err(ValidationError::InvalidMinProfit(-1.0)));
        p = good_policy();
        p.caps.min_net_profit_usd = f64::NAN;
        match p.validate() {
            Err(ValidationError::InvalidMinProfit(v)) => assert!(v.is_nan()),
            other => panic!("expected InvalidMinProfit(NaN), got {other:?}"),
        }
    }

    #[test]
    fn gas_validation() {
        let mut p = good_policy();
        p.gas.estimated_hold_days = 0;
        assert_eq!(p.validate(), Err(ValidationError::ZeroHoldDays));
        p = good_policy();
        p.gas.max_gas_usd_per_action = 0.0;
        assert_eq!(p.validate(), Err(ValidationError::InvalidMaxGas(0.0)));
        p = good_policy();
        p.gas.max_gas_usd_per_action = f64::INFINITY;
        assert_eq!(
            p.validate(),
            Err(ValidationError::InvalidMaxGas(f64::INFINITY))
        );
    }

    #[test]
    fn circuit_breaker_bounds() {
        let mut p = good_policy();
        p.circuit_breakers.usdc_peg_min = 1.5;
        assert_eq!(p.validate(), Err(ValidationError::InvalidPegMin(1.5)));
        p = good_policy();
        p.circuit_breakers.utilization_max = 0.0;
        assert_eq!(
            p.validate(),
            Err(ValidationError::InvalidUtilizationMax(0.0))
        );
        p = good_policy();
        p.circuit_breakers.tvl_drop_pct_1h = 150.0;
        assert_eq!(
            p.validate(),
            Err(ValidationError::InvalidTvlDropPct(150.0))
        );
    }

    #[test]
    fn json_round_trip() {
        let p = good_policy();
        let s = serde_json::to_string(&p).unwrap();
        let back: Policy = serde_json::from_str(&s).unwrap();
        assert!(back.validate().is_ok());
        assert_eq!(back.protocols.whitelist.len(), 2);
        assert_eq!(back.status, PolicyStatus::Active);
    }

    #[test]
    fn status_defaults_to_active_when_missing() {
        let p = good_policy();
        let mut v = serde_json::to_value(&p).unwrap();
        v.as_object_mut().unwrap().remove("status");
        let back: Policy = serde_json::from_value(v).unwrap();
        assert_eq!(back.status, PolicyStatus::Active);
    }
}
