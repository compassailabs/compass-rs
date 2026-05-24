use alloy::primitives::Address;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::automation::policy::ChainId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: u64,
    pub ts: DateTime<Utc>,
    pub user: Address,
    pub event_type: EventType,
    pub policy_version: Option<u32>,
    pub payload: Value,
    pub tx_hash: Option<String>,
    pub chain: Option<ChainId>,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct NewAuditEvent {
    pub ts: DateTime<Utc>,
    pub user: Address,
    pub event_type: EventType,
    pub policy_version: Option<u32>,
    pub payload: Value,
    pub tx_hash: Option<String>,
    pub chain: Option<ChainId>,
    pub cost_usd: Option<f64>,
}

impl NewAuditEvent {
    pub fn new(user: Address, event_type: EventType, payload: Value, ts: DateTime<Utc>) -> Self {
        Self {
            ts,
            user,
            event_type,
            policy_version: None,
            payload,
            tx_hash: None,
            chain: None,
            cost_usd: None,
        }
    }

    pub fn with_policy_version(mut self, v: u32) -> Self {
        self.policy_version = Some(v);
        self
    }

    pub fn with_tx_hash(mut self, hash: impl Into<String>) -> Self {
        self.tx_hash = Some(hash.into());
        self
    }

    pub fn with_chain(mut self, chain: ChainId) -> Self {
        self.chain = Some(chain);
        self
    }

    pub fn with_cost_usd(mut self, cost: f64) -> Self {
        self.cost_usd = Some(cost);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    TriggerFired,
    EvaluatorThought,
    EvaluatorDecision,
    RiskGateDecision,
    LlmEscalateIn,
    LlmEscalateOut,
    ExecutorActionStart,
    ExecutorSubstep,
    ExecutorActionDone,
    CircuitBreak,
    PolicyChange,
    SessionRevoke,
}
