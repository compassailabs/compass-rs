use std::collections::HashMap;
use std::str::FromStr;

use alloy::primitives::Address;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::RwLock;

pub const CACHE_TTL_SECS: i64 = 30;

#[async_trait]
pub trait SessionCacheStore: Send + Sync {
    async fn get_fresh(&self, user: Address, now: DateTime<Utc>) -> Result<Option<Value>>;
    async fn put(&self, user: Address, payload: Value) -> Result<()>;
    async fn invalidate(&self, user: Address) -> Result<()>;
}

struct CachedEntry {
    payload: Value,
    cached_at: DateTime<Utc>,
}

#[derive(Default)]
pub struct InMemorySessionCacheStore {
    inner: RwLock<HashMap<Address, CachedEntry>>,
}

impl InMemorySessionCacheStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SessionCacheStore for InMemorySessionCacheStore {
    async fn get_fresh(&self, user: Address, now: DateTime<Utc>) -> Result<Option<Value>> {
        let guard = self.inner.read().await;
        let Some(entry) = guard.get(&user) else {
            return Ok(None);
        };
        let age = now.signed_duration_since(entry.cached_at);
        if age.num_seconds() > CACHE_TTL_SECS {
            Ok(None)
        } else {
            Ok(Some(entry.payload.clone()))
        }
    }

    async fn put(&self, user: Address, payload: Value) -> Result<()> {
        self.inner.write().await.insert(
            user,
            CachedEntry {
                payload,
                cached_at: Utc::now(),
            },
        );
        Ok(())
    }

    async fn invalidate(&self, user: Address) -> Result<()> {
        self.inner.write().await.remove(&user);
        Ok(())
    }
}

pub struct PostgresSessionCacheStore {
    pool: PgPool,
}

impl PostgresSessionCacheStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn addr_key(addr: Address) -> String {
    format!("{addr:#x}")
}

#[async_trait]
impl SessionCacheStore for PostgresSessionCacheStore {
    async fn get_fresh(&self, user: Address, now: DateTime<Utc>) -> Result<Option<Value>> {
        let key = addr_key(user);
        let row: Option<(Value, DateTime<Utc>)> =
            sqlx::query_as("SELECT payload, cached_at FROM session_cache WHERE user_addr = $1")
                .bind(&key)
                .fetch_optional(&self.pool)
                .await?;
        let Some((payload, cached_at)) = row else {
            return Ok(None);
        };
        if now.signed_duration_since(cached_at).num_seconds() > CACHE_TTL_SECS {
            Ok(None)
        } else {
            Ok(Some(payload))
        }
    }

    async fn put(&self, user: Address, payload: Value) -> Result<()> {
        let key = addr_key(user);
        sqlx::query(
            "INSERT INTO session_cache (user_addr, payload, cached_at)
             VALUES ($1, $2, NOW())
             ON CONFLICT (user_addr) DO UPDATE
             SET payload = EXCLUDED.payload, cached_at = NOW()",
        )
        .bind(&key)
        .bind(&payload)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn invalidate(&self, user: Address) -> Result<()> {
        let key = addr_key(user);
        sqlx::query("DELETE FROM session_cache WHERE user_addr = $1")
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
