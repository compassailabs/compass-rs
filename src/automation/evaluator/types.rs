use std::collections::HashMap;

use alloy::primitives::U256;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::automation::policy::{ChainId, VenueRef};

mod venue_keyed_u256 {
    use super::*;

    #[derive(Serialize, Deserialize)]
    struct Entry {
        venue: VenueRef,
        amount: U256,
    }

    pub fn serialize<S: Serializer>(
        map: &HashMap<VenueRef, U256>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let mut entries: Vec<Entry> = map
            .iter()
            .map(|(k, v)| Entry {
                venue: k.clone(),
                amount: *v,
            })
            .collect();
        entries.sort_by(|a, b| {
            format!("{:?}", a.venue).cmp(&format!("{:?}", b.venue))
        });
        entries.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<HashMap<VenueRef, U256>, D::Error> {
        let entries: Vec<Entry> = Vec::deserialize(d)?;
        Ok(entries.into_iter().map(|e| (e.venue, e.amount)).collect())
    }
}

mod venue_keyed_datetime {
    use super::*;

    #[derive(Serialize, Deserialize)]
    struct Entry {
        venue: VenueRef,
        at: DateTime<Utc>,
    }

    pub fn serialize<S: Serializer>(
        map: &HashMap<VenueRef, DateTime<Utc>>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let mut entries: Vec<Entry> = map
            .iter()
            .map(|(k, v)| Entry {
                venue: k.clone(),
                at: *v,
            })
            .collect();
        entries.sort_by(|a, b| {
            format!("{:?}", a.venue).cmp(&format!("{:?}", b.venue))
        });
        entries.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<HashMap<VenueRef, DateTime<Utc>>, D::Error> {
        let entries: Vec<Entry> = Vec::deserialize(d)?;
        Ok(entries.into_iter().map(|e| (e.venue, e.at)).collect())
    }
}

mod venue_keyed_state {
    use super::*;

    #[derive(Serialize, Deserialize)]
    struct Entry {
        venue: VenueRef,
        state: VenueState,
    }

    pub fn serialize<S: Serializer>(
        map: &HashMap<VenueRef, VenueState>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let mut entries: Vec<Entry> = map
            .iter()
            .map(|(k, v)| Entry {
                venue: k.clone(),
                state: v.clone(),
            })
            .collect();
        entries.sort_by(|a, b| {
            format!("{:?}", a.venue).cmp(&format!("{:?}", b.venue))
        });
        entries.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<HashMap<VenueRef, VenueState>, D::Error> {
        let entries: Vec<Entry> = Vec::deserialize(d)?;
        Ok(entries.into_iter().map(|e| (e.venue, e.state)).collect())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub built_at: DateTime<Utc>,
    pub usdc_usd: f64,
    pub gateway_health: GatewayHealth,
    #[serde(with = "venue_keyed_state")]
    pub venues: HashMap<VenueRef, VenueState>,
    pub gas_usd_per_userop: HashMap<ChainId, f64>,
    pub gateway_fee_usd: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayHealth {
    Ok,
    Degraded,
    Down,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenueState {
    pub apr: f64,
    pub apr_smoothed_1h: f64,
    pub utilization: f64,
    pub tvl_usd: f64,
    pub tvl_drop_pct_1h: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Position {
    #[serde(with = "venue_keyed_u256")]
    pub holdings: HashMap<VenueRef, U256>,
    #[serde(with = "venue_keyed_datetime")]
    pub last_action_at: HashMap<VenueRef, DateTime<Utc>>,
    pub actions_today: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Decision {
    Noop {
        reason: NoopReason,
    },
    Act {
        plan: ActionPlan,
    },
    Escalate {
        reason: EscalateReason,
    },
    CircuitBreak {
        reason: BreakReason,
        drain_to: Option<VenueRef>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluatorThought {
    pub label: String,
}

impl EvaluatorThought {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EvaluationOutcome {
    pub thoughts: Vec<EvaluatorThought>,
    pub decision: Decision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoopReason {
    NoCapital,
    AlreadyAtBestVenue,
    AprDeltaBelowThreshold,
    DestinationInCooldown,
    BestVenueAtCap,
    GasExceedsCap,
    EvBelowThreshold,
    DailyQuotaReached,
    GatewayDown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalateReason {
    NewListingNoData { venue: VenueRef },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BreakReason {
    SnapshotStale,
    UsdcDepeg { observed: f64 },
    VenueUnhealthy { venue: VenueRef, signal: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionPlan {
    pub actions: Vec<Action>,
    pub expected_profit_usd: f64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub from: VenueRef,
    pub to: VenueRef,
    pub amount: U256,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::policy::{ChainId, ProtocolId};
    use chrono::Utc;
    use std::collections::HashMap;

    #[test]
    fn position_serializes_to_json() {
        let venue = VenueRef {
            chain: ChainId::Arc,
            protocol: ProtocolId::Idle,
        };
        let mut h = HashMap::new();
        h.insert(venue.clone(), U256::from(1_000u128));
        let mut last = HashMap::new();
        last.insert(venue, Utc::now());
        let pos = Position {
            holdings: h,
            last_action_at: last,
            actions_today: 1,
        };
        serde_json::to_string(&pos).expect("Position should JSON-serialize");
    }

    #[test]
    fn snapshot_serializes_to_json() {
        let venue = VenueRef {
            chain: ChainId::ArbitrumSepolia,
            protocol: ProtocolId::AaveV3,
        };
        let mut venues = HashMap::new();
        venues.insert(
            venue,
            VenueState {
                apr: 0.05,
                apr_smoothed_1h: 0.05,
                utilization: 0.5,
                tvl_usd: 1_000_000.0,
                tvl_drop_pct_1h: 0.0,
            },
        );
        let mut gas = HashMap::new();
        gas.insert(ChainId::Arc, 0.5);
        let snap = Snapshot {
            built_at: Utc::now(),
            usdc_usd: 1.0,
            gateway_health: GatewayHealth::Ok,
            venues,
            gas_usd_per_userop: gas,
            gateway_fee_usd: 0.1,
        };
        serde_json::to_string(&snap).expect("Snapshot should JSON-serialize");
    }
}
