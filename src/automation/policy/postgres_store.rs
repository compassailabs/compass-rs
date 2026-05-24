use std::str::FromStr;

use alloy::primitives::Address;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use sqlx::PgPool;

use super::schema::{Policy, PolicyStatus};
use super::store::PolicyStore;

pub struct PostgresPolicyStore {
    pool: PgPool,
}

impl PostgresPolicyStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn addr_key(addr: Address) -> String {
    format!("{addr:#x}")
}

fn status_to_str(s: PolicyStatus) -> &'static str {
    match s {
        PolicyStatus::Active => "active",
        PolicyStatus::Paused => "paused",
    }
}

#[async_trait]
impl PolicyStore for PostgresPolicyStore {
    async fn get(&self, user: Address) -> Result<Option<Policy>> {
        let key = addr_key(user);
        let row: Option<(serde_json::Value,)> =
            sqlx::query_as("SELECT data FROM policy WHERE user_addr = $1")
                .bind(&key)
                .fetch_optional(&self.pool)
                .await?;
        match row {
            Some((data,)) => Ok(Some(serde_json::from_value(data)?)),
            None => Ok(None),
        }
    }

    async fn put(&self, mut policy: Policy) -> Result<u32> {
        policy.validate()?;
        let key = addr_key(policy.user);

        let mut tx = self.pool.begin().await?;

        let current: Option<(i32,)> =
            sqlx::query_as("SELECT version FROM policy WHERE user_addr = $1 FOR UPDATE")
                .bind(&key)
                .fetch_optional(&mut *tx)
                .await?;

        let next_version: i32 = current.map(|(v,)| v).unwrap_or(0) + 1;
        policy.version = next_version as u32;
        let data = serde_json::to_value(&policy)?;
        let status = status_to_str(policy.status);

        sqlx::query(
            "INSERT INTO policy (user_addr, version, status, data, updated_at)
             VALUES ($1, $2, $3, $4, NOW())
             ON CONFLICT (user_addr) DO UPDATE
             SET version = EXCLUDED.version,
                 status  = EXCLUDED.status,
                 data    = EXCLUDED.data,
                 updated_at = NOW()",
        )
        .bind(&key)
        .bind(next_version)
        .bind(status)
        .bind(&data)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(next_version as u32)
    }

    async fn set_status(&self, user: Address, status: PolicyStatus) -> Result<()> {
        let key = addr_key(user);
        let status_str = status_to_str(status);

        let mut tx = self.pool.begin().await?;
        let row: Option<(serde_json::Value,)> =
            sqlx::query_as("SELECT data FROM policy WHERE user_addr = $1 FOR UPDATE")
                .bind(&key)
                .fetch_optional(&mut *tx)
                .await?;
        let Some((mut data,)) = row else {
            return Err(anyhow!("no policy for user {user}"));
        };

        if let serde_json::Value::Object(ref mut m) = data {
            m.insert(
                "status".into(),
                serde_json::Value::String(status_str.into()),
            );
        }

        sqlx::query(
            "UPDATE policy SET status = $1, data = $2, updated_at = NOW() WHERE user_addr = $3",
        )
        .bind(status_str)
        .bind(&data)
        .bind(&key)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn list_active_users(&self) -> Result<Vec<Address>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT user_addr FROM policy WHERE status = 'active'")
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|(s,)| {
                Address::from_str(&s)
                    .map_err(|e| anyhow!("bad address in db ({s}): {e}"))
            })
            .collect()
    }
}
