use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use anyhow::Result;
use async_trait::async_trait;

use crate::aave::pool::IPool;
use crate::account::predict_address;
use crate::automation::evaluator::Position;
use crate::automation::policy::{ChainId, ProtocolId, VenueRef};
use crate::automation::position::PositionFetcher;
use crate::chain::arc::ArcClient;
use crate::chain::arbitrum_sepolia::ArbitrumSepoliaClient;
use crate::chain::usdc::IERC20;
use crate::config::AppConfig;

pub struct RpcPositionFetcher {
    pub cfg: Arc<AppConfig>,
    pub arc: Arc<ArcClient>,
    pub arb: Arc<ArbitrumSepoliaClient>,
}

#[async_trait]
impl PositionFetcher for RpcPositionFetcher {
    async fn fetch(&self, user: Address) -> Result<Position> {
        let arc_factory = Address::from_str(&self.cfg.arc_factory)?;
        let arb_factory = Address::from_str(&self.cfg.arbitrum_sepolia_factory)?;
        let aave_usdc = Address::from_str(&self.cfg.arbitrum_sepolia_aave_usdc)?;

        let arc_diamond =
            predict_address(&self.cfg.arc_rpc_url, arc_factory, user).await?;
        let arb_diamond =
            predict_address(&self.cfg.arbitrum_sepolia_rpc_url, arb_factory, user).await?;

        let arc_idle = self.arc.usdc_balance(arc_diamond).await?;
        let arb_idle = self.arb.usdc_balance(arb_diamond).await?;

        let aave_position =
            match read_aave_position(&self.arb, aave_usdc, arb_diamond).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(
                        user = %user,
                        error = %e,
                        "could not read AAVE position, treating as zero"
                    );
                    U256::ZERO
                }
            };

        let mut holdings = HashMap::new();
        if !arc_idle.is_zero() {
            holdings.insert(
                VenueRef {
                    chain: ChainId::Arc,
                    protocol: ProtocolId::Idle,
                },
                arc_idle,
            );
        }
        if !arb_idle.is_zero() {
            holdings.insert(
                VenueRef {
                    chain: ChainId::ArbitrumSepolia,
                    protocol: ProtocolId::Idle,
                },
                arb_idle,
            );
        }
        if !aave_position.is_zero() {
            holdings.insert(
                VenueRef {
                    chain: ChainId::ArbitrumSepolia,
                    protocol: ProtocolId::AaveV3,
                },
                aave_position,
            );
        }

        Ok(Position {
            holdings,
            last_action_at: HashMap::new(),
            actions_today: 0,
        })
    }
}

async fn read_aave_position(
    arb: &ArbitrumSepoliaClient,
    aave_usdc: Address,
    owner: Address,
) -> Result<U256> {
    let provider = ProviderBuilder::new()
        .wallet(arb.signer.clone())
        .connect(&arb.rpc_url)
        .await?;
    let pool = IPool::new(arb.aave_pool, provider.clone());
    let data = pool.getReserveData(aave_usdc).call().await?;
    let token = IERC20::new(data.aTokenAddress, provider);
    Ok(token.balanceOf(owner).call().await?)
}
