use std::str::FromStr;

use alloy::primitives::{Address, U256};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::automation::policy::ChainId;

use super::schema::{FundingEvent, FundingKind, NewFundingEvent};
use super::store::FundingStore;

pub struct PostgresFundingStore {
    pool: PgPool,
}

impl PostgresFundingStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn addr_key(a: Address) -> String {
    format!("{a:#x}")
}

fn chain_to_str(c: ChainId) -> &'static str {
    match c {
        ChainId::Arc => "arc",
        ChainId::ArbitrumSepolia => "arbitrum_sepolia",
    }
}

fn chain_from_str(s: &str) -> Result<ChainId> {
    Ok(match s {
        "arc" => ChainId::Arc,
        "arbitrum_sepolia" => ChainId::ArbitrumSepolia,
        other => return Err(anyhow!("unknown chain in db: {other}")),
    })
}

fn kind_to_str(k: FundingKind) -> &'static str {
    match k {
        FundingKind::Deposit => "deposit",
        FundingKind::WithdrawToEoa => "withdraw_to_eoa",
    }
}

fn kind_from_str(s: &str) -> Result<FundingKind> {
    Ok(match s {
        "deposit" => FundingKind::Deposit,
        "withdraw_to_eoa" => FundingKind::WithdrawToEoa,
        other => return Err(anyhow!("unknown funding kind in db: {other}")),
    })
}

type Row = (
    i64,           // id
    DateTime<Utc>, // ts
    String,        // user_addr
    String,        // chain
    String,        // kind
    String,        // amount_6dec
    String,        // tx_hash
);

fn row_to_event(row: Row) -> Result<FundingEvent> {
    let (id, ts, user_addr, chain, kind, amount_6dec, tx_hash) = row;
    Ok(FundingEvent {
        id: id as u64,
        ts,
        user: Address::from_str(&user_addr)
            .map_err(|e| anyhow!("bad user_addr {user_addr}: {e}"))?,
        chain: chain_from_str(&chain)?,
        kind: kind_from_str(&kind)?,
        amount_6dec,
        tx_hash,
    })
}

#[async_trait]
impl FundingStore for PostgresFundingStore {
    async fn append(&self, event: NewFundingEvent) -> Result<u64> {
        let user = addr_key(event.user);
        let inserted: Option<(i64,)> = sqlx::query_as(
            "INSERT INTO funding_event (ts, user_addr, chain, kind, amount_6dec, tx_hash)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (user_addr, chain, kind, tx_hash) DO NOTHING
             RETURNING id",
        )
        .bind(event.ts)
        .bind(&user)
        .bind(chain_to_str(event.chain))
        .bind(kind_to_str(event.kind))
        .bind(event.amount.to_string())
        .bind(&event.tx_hash)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((id,)) = inserted {
            return Ok(id as u64);
        }
        let (id,): (i64,) = sqlx::query_as(
            "SELECT id FROM funding_event
             WHERE user_addr = $1 AND chain = $2 AND kind = $3 AND tx_hash = $4",
        )
        .bind(&user)
        .bind(chain_to_str(event.chain))
        .bind(kind_to_str(event.kind))
        .bind(&event.tx_hash)
        .fetch_one(&self.pool)
        .await?;
        Ok(id as u64)
    }

    async fn sum(&self, user: Address, kind: FundingKind) -> Result<U256> {
        let user_key = addr_key(user);
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT amount_6dec FROM funding_event WHERE user_addr = $1 AND kind = $2",
        )
        .bind(&user_key)
        .bind(kind_to_str(kind))
        .fetch_all(&self.pool)
        .await?;
        let mut total = U256::ZERO;
        for (raw,) in rows {
            total += U256::from_str(&raw).unwrap_or(U256::ZERO);
        }
        Ok(total)
    }

    async fn list_for_user(&self, user: Address, limit: usize) -> Result<Vec<FundingEvent>> {
        let user_key = addr_key(user);
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT id, ts, user_addr, chain, kind, amount_6dec, tx_hash
             FROM funding_event
             WHERE user_addr = $1
             ORDER BY id DESC
             LIMIT $2",
        )
        .bind(&user_key)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_event).collect()
    }
}
