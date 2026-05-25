use std::env;
use std::str::FromStr;

use alloy::primitives::Address;
use anyhow::{Context, Result, anyhow};

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub bind: String,

    pub arc_rpc_url: String,
    pub arc_usdc: String,
    pub arc_factory: String,
    pub arc_entry_point: String,

    pub arbitrum_sepolia_rpc_url: String,
    pub arbitrum_sepolia_aave_pool: String,
    pub arbitrum_sepolia_aave_usdc: String,
    pub arbitrum_sepolia_usdc: String,
    pub arbitrum_sepolia_factory: String,
    pub arbitrum_sepolia_entry_point: String,
    pub arbitrum_sepolia_paymaster: Option<String>,
    
    pub arc_paymaster: Option<String>,

    pub user_pk: String,
    pub agent_pk: String,

    pub gateway_api_url: String,
    pub gateway_api_key: String,
    pub gateway_wallet: String,
    pub gateway_minter: String,

    pub compass_upgrade_authority: String,

    pub anthropic_api_key: String,
    pub anthropic_model: String,

    pub automation_cron_interval_secs: u64,
    pub automation_snapshot_interval_secs: u64,

    pub database_url: Option<String>,
    pub db_schema: String,

    pub enable_debug_api: bool,

    pub disable_automation: bool,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let cfg = Self {
            bind: env::var("COMPASS_BIND").unwrap_or_else(|_| "0.0.0.0:8787".into()),

            arc_rpc_url: req("ARC_RPC_URL")?,
            arc_usdc: req("ARC_USDC_ADDRESS")?,
            arc_factory: req("ARC_FACTORY_ADDRESS")?,
            arc_entry_point: req("ARC_ENTRY_POINT")?,

            arbitrum_sepolia_rpc_url: req("ARBITRUM_SEPOLIA_RPC_URL")?,
            arbitrum_sepolia_aave_pool: req("ARBITRUM_SEPOLIA_AAVE_POOL")?,
            arbitrum_sepolia_aave_usdc: req("ARBITRUM_SEPOLIA_AAVE_USDC")?,
            arbitrum_sepolia_usdc: env::var("ARBITRUM_SEPOLIA_USDC")
                .unwrap_or_else(|_| "0x75faf114eafb1BDbe2F0316DF893fd58CE46AA4d".into()),
            arbitrum_sepolia_factory: req("ARBITRUM_SEPOLIA_FACTORY_ADDRESS")?,
            arbitrum_sepolia_entry_point: req("ARBITRUM_SEPOLIA_ENTRY_POINT")?,
            arbitrum_sepolia_paymaster: env::var("ARBITRUM_SEPOLIA_PAYMASTER")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            arc_paymaster: env::var("ARC_PAYMASTER")
                .ok()
                .filter(|s| !s.trim().is_empty()),

            user_pk: req("COMPASS_USER_PK")?,
            agent_pk: req("COMPASS_AGENT_PK")?,

            gateway_api_url: req("GATEWAY_API_URL")?,
            gateway_api_key: env::var("GATEWAY_API_KEY").unwrap_or_default(),
            gateway_wallet: req("GATEWAY_WALLET_ADDRESS")?,
            gateway_minter: req("GATEWAY_MINTER_ADDRESS")?,

            compass_upgrade_authority: req("COMPASS_UPGRADE_AUTHORITY")?,

            anthropic_api_key: env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            anthropic_model: env::var("ANTHROPIC_MODEL")
                .unwrap_or_else(|_| "claude-opus-4-7".into()),

            automation_cron_interval_secs: env::var("COMPASS_AUTOMATION_CRON_INTERVAL_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(900),
            automation_snapshot_interval_secs: env::var(
                "COMPASS_AUTOMATION_SNAPSHOT_INTERVAL_SECS",
            )
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60),

            database_url: env::var("DATABASE_URL").ok().filter(|s| !s.is_empty()),
            db_schema: {
                let schema = env::var("COMPASS_DB_SCHEMA")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "public".into());
                if !schema
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
                    || schema.chars().next().is_some_and(|c| c.is_ascii_digit())
                {
                    return Err(anyhow!(
                        "COMPASS_DB_SCHEMA={schema:?} must match [A-Za-z_][A-Za-z0-9_]*"
                    ));
                }
                schema
            },

            enable_debug_api: env::var("ENABLE_DEBUG_API")
                .ok()
                .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes" | "on"))
                .unwrap_or(false),

            disable_automation: env::var("COMPASS_DISABLE_AUTOMATION")
                .ok()
                .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes" | "on"))
                .unwrap_or(false),
        };

        cfg.validate_addresses()?;
        Ok(cfg)
    }

    fn validate_addresses(&self) -> Result<()> {
        for (label, value) in [
            ("ARC_USDC_ADDRESS", &self.arc_usdc),
            ("ARC_FACTORY_ADDRESS", &self.arc_factory),
            ("ARC_ENTRY_POINT", &self.arc_entry_point),
            ("ARBITRUM_SEPOLIA_AAVE_POOL", &self.arbitrum_sepolia_aave_pool),
            ("ARBITRUM_SEPOLIA_AAVE_USDC", &self.arbitrum_sepolia_aave_usdc),
            ("ARBITRUM_SEPOLIA_USDC", &self.arbitrum_sepolia_usdc),
            ("ARBITRUM_SEPOLIA_FACTORY_ADDRESS", &self.arbitrum_sepolia_factory),
            ("ARBITRUM_SEPOLIA_ENTRY_POINT", &self.arbitrum_sepolia_entry_point),
            ("GATEWAY_WALLET_ADDRESS", &self.gateway_wallet),
            ("GATEWAY_MINTER_ADDRESS", &self.gateway_minter),
            ("COMPASS_UPGRADE_AUTHORITY", &self.compass_upgrade_authority),
        ] {
            if value.trim().is_empty() {
                return Err(anyhow!("env var {label} is empty — set it in .env"));
            }
            Address::from_str(value).with_context(|| {
                format!(
                    "env var {label}={value:?} is not a valid 0x… address \
                     (expected 0x + 40 hex chars)"
                )
            })?;
        }
        Ok(())
    }
}

fn req(key: &str) -> Result<String> {
    env::var(key).with_context(|| format!("env var {key} required"))
}
