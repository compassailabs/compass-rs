use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use alloy::primitives::Address;
use anyhow::Result;
use chrono::Utc;
use tokio::task::JoinHandle;

use crate::aave::helpers::current_supply_apr;
use crate::automation::evaluator::{GatewayHealth, Snapshot, VenueState};
use crate::automation::policy::{ChainId, ProtocolId, VenueRef};
use crate::automation::snapshot::SnapshotStore;
use crate::chain::arbitrum_sepolia::ArbitrumSepoliaClient;
use crate::config::AppConfig;

pub async fn build_snapshot(cfg: &AppConfig, arb: &ArbitrumSepoliaClient) -> Result<Snapshot> {
    let aave_usdc = Address::from_str(&cfg.arbitrum_sepolia_aave_usdc)?;
    let aave_apr = current_supply_apr(arb, aave_usdc).await?;

    let mut venues = HashMap::new();

    for chain in [ChainId::Arc, ChainId::ArbitrumSepolia] {
        venues.insert(
            VenueRef {
                chain,
                protocol: ProtocolId::Idle,
            },
            VenueState {
                apr: 0.0,
                apr_smoothed_1h: 0.0,
                utilization: 0.0,
                tvl_usd: f64::INFINITY,
                tvl_drop_pct_1h: 0.0,
            },
        );
    }

    venues.insert(
        VenueRef {
            chain: ChainId::ArbitrumSepolia,
            protocol: ProtocolId::AaveV3,
        },
        VenueState {
            apr: aave_apr,
            apr_smoothed_1h: aave_apr, // TODO: maintain a 1h rolling window
            utilization: 0.5,          // TODO: derive from AAVE reserveData
            tvl_usd: 0.0,              // TODO: read aToken totalSupply × price
            tvl_drop_pct_1h: 0.0,      // TODO: needs TVL history
        },
    );

    let mut gas = HashMap::new();
    gas.insert(ChainId::Arc, 0.50); // TODO: derive from eth_gasPrice × USDC/ETH
    gas.insert(ChainId::ArbitrumSepolia, 0.50);

    Ok(Snapshot {
        built_at: Utc::now(),
        usdc_usd: 1.0,                     // TODO: Chainlink USDC/USD feed
        gateway_health: GatewayHealth::Ok, // TODO: probe `GET /v1/info`
        venues,
        gas_usd_per_userop: gas,
        gateway_fee_usd: 0.10, // TODO: read from Circle quote endpoint
    })
}

pub fn spawn_snapshot_worker(
    cfg: Arc<AppConfig>,
    arb: Arc<ArbitrumSepoliaClient>,
    store: Arc<dyn SnapshotStore>,
    interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            match build_snapshot(&cfg, &arb).await {
                Ok(snap) => {
                    if let Err(e) = store.put(snap).await {
                        tracing::error!(error = %e, "snapshot store put failed");
                    } else {
                        tracing::debug!("snapshot refreshed");
                    }
                }
                Err(e) => tracing::error!(error = %e, "snapshot build failed"),
            }
        }
    })
}
