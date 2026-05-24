use std::str::FromStr;

use alloy::primitives::Address;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::automation::policy::ChainId;

use super::schema::{AuditEvent, EventType, NewAuditEvent};
use super::store::AuditStore;

pub struct PostgresAuditStore {
    pool: PgPool,
}

impl PostgresAuditStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn addr_key(addr: Address) -> String {
    format!("{addr:#x}")
}

fn event_type_to_str(et: EventType) -> &'static str {
    match et {
        EventType::TriggerFired => "trigger_fired",
        EventType::EvaluatorThought => "evaluator_thought",
        EventType::EvaluatorDecision => "evaluator_decision",
        EventType::RiskGateDecision => "risk_gate_decision",
        EventType::LlmEscalateIn => "llm_escalate_in",
        EventType::LlmEscalateOut => "llm_escalate_out",
        EventType::ExecutorActionStart => "executor_action_start",
        EventType::ExecutorSubstep => "executor_substep",
        EventType::ExecutorActionDone => "executor_action_done",
        EventType::CircuitBreak => "circuit_break",
        EventType::PolicyChange => "policy_change",
        EventType::SessionRevoke => "session_revoke",
    }
}

fn event_type_from_str(s: &str) -> Result<EventType> {
    Ok(match s {
        "trigger_fired" => EventType::TriggerFired,
        "evaluator_thought" => EventType::EvaluatorThought,
        "evaluator_decision" => EventType::EvaluatorDecision,
        "risk_gate_decision" => EventType::RiskGateDecision,
        "llm_escalate_in" => EventType::LlmEscalateIn,
        "llm_escalate_out" => EventType::LlmEscalateOut,
        "executor_action_start" => EventType::ExecutorActionStart,
        "executor_substep" => EventType::ExecutorSubstep,
        "executor_action_done" => EventType::ExecutorActionDone,
        "circuit_break" => EventType::CircuitBreak,
        "policy_change" => EventType::PolicyChange,
        "session_revoke" => EventType::SessionRevoke,
        other => return Err(anyhow!("unknown event_type in db: {other}")),
    })
}

fn chain_to_str(c: Option<ChainId>) -> Option<&'static str> {
    c.map(|c| match c {
        ChainId::Arc => "arc",
        ChainId::ArbitrumSepolia => "arbitrum_sepolia",
    })
}

fn chain_from_str(s: Option<String>) -> Option<ChainId> {
    s.and_then(|v| match v.as_str() {
        "arc" => Some(ChainId::Arc),
        "arbitrum_sepolia" => Some(ChainId::ArbitrumSepolia),
        _ => None,
    })
}

type AuditRow = (
    i64,                  // id
    DateTime<Utc>,        // ts
    String,               // user_addr
    String,               // event_type
    Option<i32>,          // policy_version
    serde_json::Value,    // payload
    Option<String>,       // tx_hash
    Option<String>,       // chain
    Option<f64>,          // cost_usd
);

fn row_to_event(row: AuditRow) -> Result<AuditEvent> {
    let (id, ts, user_addr, et, policy_version, payload, tx_hash, chain, cost_usd) = row;
    Ok(AuditEvent {
        id: id as u64,
        ts,
        user: Address::from_str(&user_addr)
            .map_err(|e| anyhow!("bad user_addr in db ({user_addr}): {e}"))?,
        event_type: event_type_from_str(&et)?,
        policy_version: policy_version.map(|v| v as u32),
        payload,
        tx_hash,
        chain: chain_from_str(chain),
        cost_usd,
    })
}

#[async_trait]
impl AuditStore for PostgresAuditStore {
    async fn append(&self, event: NewAuditEvent) -> Result<u64> {
        let key = addr_key(event.user);
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO audit_event (
                ts, user_addr, event_type, policy_version, payload, tx_hash, chain, cost_usd
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id",
        )
        .bind(event.ts)
        .bind(&key)
        .bind(event_type_to_str(event.event_type))
        .bind(event.policy_version.map(|v| v as i32))
        .bind(&event.payload)
        .bind(event.tx_hash.as_deref())
        .bind(chain_to_str(event.chain))
        .bind(event.cost_usd)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0 as u64)
    }

    async fn get(&self, id: u64) -> Result<Option<AuditEvent>> {
        let row: Option<AuditRow> = sqlx::query_as(
            "SELECT id, ts, user_addr, event_type, policy_version, payload, tx_hash, chain, cost_usd
             FROM audit_event WHERE id = $1",
        )
        .bind(id as i64)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_event).transpose()
    }

    async fn list_for_user(
        &self,
        user: Address,
        since: Option<DateTime<Utc>>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>> {
        let key = addr_key(user);
        let rows: Vec<AuditRow> = match since {
            Some(s) => sqlx::query_as(
                "SELECT id, ts, user_addr, event_type, policy_version, payload, tx_hash, chain, cost_usd
                 FROM audit_event
                 WHERE user_addr = $1 AND ts > $2
                 ORDER BY id DESC
                 LIMIT $3",
            )
            .bind(&key)
            .bind(s)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?,
            None => sqlx::query_as(
                "SELECT id, ts, user_addr, event_type, policy_version, payload, tx_hash, chain, cost_usd
                 FROM audit_event
                 WHERE user_addr = $1
                 ORDER BY id DESC
                 LIMIT $2",
            )
            .bind(&key)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?,
        };
        rows.into_iter().map(row_to_event).collect()
    }
}
