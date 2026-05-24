use std::str::FromStr;

use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;

use crate::chain::usdc::IERC20;

pub struct ArbitrumSepoliaClient {
    pub rpc_url: String,
    pub signer: PrivateKeySigner,
    pub aave_pool: Address,
    pub aave_usdc: Address,
}

impl ArbitrumSepoliaClient {
    pub async fn connect(rpc_url: &str, signer: PrivateKeySigner) -> Result<Self> {
        Ok(Self {
            rpc_url: rpc_url.to_string(),
            signer,
            aave_pool: Address::from_str(&std::env::var("ARBITRUM_SEPOLIA_AAVE_POOL")?)?,
            aave_usdc: Address::from_str(&std::env::var("ARBITRUM_SEPOLIA_AAVE_USDC")?)?,
        })
    }

    pub async fn usdc_balance(&self, who: Address) -> Result<U256> {
        let provider = ProviderBuilder::new()
            .wallet(self.signer.clone())
            .connect(&self.rpc_url)
            .await?;
        let token = IERC20::new(self.aave_usdc, provider);
        Ok(token.balanceOf(who).call().await?)
    }

    pub async fn usdc_approve(&self, spender: Address, amount: U256) -> Result<String> {
        let provider = ProviderBuilder::new()
            .wallet(self.signer.clone())
            .connect(&self.rpc_url)
            .await?;
        let token = IERC20::new(self.aave_usdc, provider);
        let pending = token.approve(spender, amount).send().await?;
        Ok(format!("{:?}", pending.tx_hash()))
    }
}
