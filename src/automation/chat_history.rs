use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicI64, Ordering};

use alloy::primitives::Address;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurnRow {
    pub id: i64,
    pub ts: DateTime<Utc>,
    pub role: ChatRole,
    pub text: String,
    pub trace: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
}

impl ChatRole {
    fn as_str(self) -> &'static str {
        match self {
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
        }
    }

    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "user" => ChatRole::User,
            "assistant" => ChatRole::Assistant,
            other => return Err(anyhow!("unknown chat role: {other}")),
        })
    }
}

pub struct NewChatTurn {
    pub user: Address,
    pub role: ChatRole,
    pub text: String,
    pub trace: Option<Value>,
}

#[async_trait]
pub trait ChatHistoryStore: Send + Sync {
    async fn append(&self, turn: NewChatTurn) -> Result<i64>;
    async fn list_for_user(&self, user: Address, limit: usize) -> Result<Vec<ChatTurnRow>>;
    async fn clear_for_user(&self, user: Address) -> Result<()>;
}

pub struct InMemoryChatHistoryStore {
    inner: RwLock<HashMap<Address, Vec<ChatTurnRow>>>,
    next_id: AtomicI64,
}

impl InMemoryChatHistoryStore {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            next_id: AtomicI64::new(1),
        }
    }
}

impl Default for InMemoryChatHistoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatHistoryStore for InMemoryChatHistoryStore {
    async fn append(&self, turn: NewChatTurn) -> Result<i64> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let row = ChatTurnRow {
            id,
            ts: Utc::now(),
            role: turn.role,
            text: turn.text,
            trace: turn.trace,
        };
        self.inner.write().await.entry(turn.user).or_default().push(row);
        Ok(id)
    }

    async fn list_for_user(&self, user: Address, limit: usize) -> Result<Vec<ChatTurnRow>> {
        let guard = self.inner.read().await;
        let Some(rows) = guard.get(&user) else {
            return Ok(Vec::new());
        };
        let start = rows.len().saturating_sub(limit);
        Ok(rows[start..].to_vec())
    }

    async fn clear_for_user(&self, user: Address) -> Result<()> {
        self.inner.write().await.remove(&user);
        Ok(())
    }
}

pub struct PostgresChatHistoryStore {
    pool: PgPool,
}

impl PostgresChatHistoryStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn addr_key(addr: Address) -> String {
    format!("{addr:#x}")
}

type ChatRow = (
    i64,                  // id
    DateTime<Utc>,        // ts
    String,               // role
    String,               // text
    Option<Value>,        // trace
);

fn row_to_turn(row: ChatRow) -> Result<ChatTurnRow> {
    let (id, ts, role, text, trace) = row;
    Ok(ChatTurnRow {
        id,
        ts,
        role: ChatRole::from_str(&role)?,
        text,
        trace,
    })
}

#[async_trait]
impl ChatHistoryStore for PostgresChatHistoryStore {
    async fn append(&self, turn: NewChatTurn) -> Result<i64> {
        let key = addr_key(turn.user);
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO chat_message (user_addr, role, text, trace)
             VALUES ($1, $2, $3, $4)
             RETURNING id",
        )
        .bind(&key)
        .bind(turn.role.as_str())
        .bind(&turn.text)
        .bind(&turn.trace)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    async fn list_for_user(&self, user: Address, limit: usize) -> Result<Vec<ChatTurnRow>> {
        let key = addr_key(user);
        let mut rows: Vec<ChatRow> = sqlx::query_as(
            "SELECT id, ts, role, text, trace
             FROM chat_message
             WHERE user_addr = $1
             ORDER BY id DESC
             LIMIT $2",
        )
        .bind(&key)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        rows.reverse();
        rows.into_iter().map(row_to_turn).collect()
    }

    async fn clear_for_user(&self, user: Address) -> Result<()> {
        let key = addr_key(user);
        sqlx::query("DELETE FROM chat_message WHERE user_addr = $1")
            .bind(&key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[allow(dead_code)]
fn _parse_addr(s: &str) -> Result<Address> {
    Address::from_str(s).map_err(|e| anyhow!("bad address: {e}"))
}
