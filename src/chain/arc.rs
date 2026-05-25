use std::str::FromStr;

use alloy::primitives::{Address, U256};
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;

use crate::chain::usdc::IERC20;

pub struct ArcClient {
    pub rpc_url: String,
    pub signer: PrivateKeySigner,
    pub usdc: Address,
    read_provider: DynProvider,
}

impl ArcClient {
    pub async fn connect(rpc_url: &str, signer: PrivateKeySigner) -> Result<Self> {
        let usdc = Address::from_str(
            &std::env::var("ARC_USDC_ADDRESS")
                .unwrap_or_else(|_| "0x3600000000000000000000000000000000000000".into()),
        )?;
        let read_provider = ProviderBuilder::new().connect(rpc_url).await?.erased();
        Ok(Self {
            rpc_url: rpc_url.to_string(),
            signer,
            usdc,
            read_provider,
        })
    }

    pub fn read_provider(&self) -> &DynProvider {
        &self.read_provider
    }

    pub async fn usdc_balance(&self, who: Address) -> Result<U256> {
        let token = IERC20::new(self.usdc, &self.read_provider);
        let bal = token.balanceOf(who).call().await?;
        Ok(bal)
    }

    pub async fn usdc_approve(&self, spender: Address, amount: U256) -> Result<String> {
        let provider = ProviderBuilder::new()
            .wallet(self.signer.clone())
            .connect(&self.rpc_url)
            .await?;
        let token = IERC20::new(self.usdc, provider);
        let pending = token.approve(spender, amount).send().await?;
        Ok(format!("{:?}", pending.tx_hash()))
    }
}
