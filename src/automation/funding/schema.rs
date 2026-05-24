use alloy::primitives::{Address, U256};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::automation::policy::ChainId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingKind {
    Deposit,
    WithdrawToEoa,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingEvent {
    pub id: u64,
    pub ts: DateTime<Utc>,
    pub user: Address,
    pub chain: ChainId,
    pub kind: FundingKind,
    pub amount_6dec: String,
    pub tx_hash: String,
}

#[derive(Debug, Clone)]
pub struct NewFundingEvent {
    pub ts: DateTime<Utc>,
    pub user: Address,
    pub chain: ChainId,
    pub kind: FundingKind,
    pub amount: U256,
    pub tx_hash: String,
}

impl NewFundingEvent {
    pub fn new(
        user: Address,
        chain: ChainId,
        kind: FundingKind,
        amount: U256,
        tx_hash: impl Into<String>,
        ts: DateTime<Utc>,
    ) -> Self {
        Self {
            ts,
            user,
            chain,
            kind,
            amount,
            tx_hash: tx_hash.into(),
        }
    }
}
