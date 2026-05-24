use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use anyhow::Result;

use crate::aave::pool::IPool;
use crate::chain::arbitrum_sepolia::ArbitrumSepoliaClient;

pub fn ray_to_apr(ray_rate: U256) -> f64 {
    let ray = 1e27_f64;
    let r = u128::try_from(ray_rate).unwrap_or(0) as f64;
    r / ray
}

pub async fn current_supply_apr(client: &ArbitrumSepoliaClient, asset: Address) -> Result<f64> {
    let provider = ProviderBuilder::new()
        .wallet(client.signer.clone())
        .connect(&client.rpc_url)
        .await?;
    let pool = IPool::new(client.aave_pool, provider);
    let data = pool.getReserveData(asset).call().await?;
    Ok(ray_to_apr(U256::from(data.currentLiquidityRate)))
}

pub async fn supply(client: &ArbitrumSepoliaClient, asset: Address, amount: U256) -> Result<String> {
    let provider = ProviderBuilder::new()
        .wallet(client.signer.clone())
        .connect(&client.rpc_url)
        .await?;
    let pool = IPool::new(client.aave_pool, provider);
    let on_behalf_of = client.signer.address();
    let pending = pool.supply(asset, amount, on_behalf_of, 0).send().await?;
    Ok(format!("{:?}", pending.tx_hash()))
}

pub async fn withdraw(client: &ArbitrumSepoliaClient, asset: Address, amount: U256) -> Result<String> {
    let provider = ProviderBuilder::new()
        .wallet(client.signer.clone())
        .connect(&client.rpc_url)
        .await?;
    let pool = IPool::new(client.aave_pool, provider);
    let to = client.signer.address();
    let pending = pool.withdraw(asset, amount, to).send().await?;
    Ok(format!("{:?}", pending.tx_hash()))
}
