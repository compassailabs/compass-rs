use alloy::primitives::{Address, FixedBytes, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use alloy::sol_types::SolCall;
use anyhow::{Result, anyhow};

use crate::contracts::{
    IAaveFacet, ICompassAccountFactory, IGatewayFacet, ISecurityFacet, InitArgs,
};

pub const ACCOUNT_SALT: u64 = 0;

pub async fn predict_address(
    rpc_url: &str,
    factory: Address,
    owner: Address,
) -> Result<Address> {
    let provider = ProviderBuilder::new().connect(rpc_url).await?;
    let f = ICompassAccountFactory::new(factory, provider);
    Ok(f.getAccountAddress(owner, U256::from(ACCOUNT_SALT)).call().await?)
}

pub async fn is_deployed(rpc_url: &str, addr: Address) -> Result<bool> {
    let provider = ProviderBuilder::new().connect(rpc_url).await?;
    let code = provider.get_code_at(addr).await?;
    Ok(!code.is_empty())
}

pub async fn deploy(
    rpc_url: &str,
    relayer: PrivateKeySigner,
    factory: Address,
    owner: Address,
    init: InitArgs,
) -> Result<(Address, Option<String>)> {
    let provider = ProviderBuilder::new()
        .wallet(relayer)
        .connect(rpc_url)
        .await?;
    let f = ICompassAccountFactory::new(factory, provider.clone());

    let predicted = f
        .getAccountAddress(owner, U256::from(ACCOUNT_SALT))
        .call()
        .await?;

    let code = provider.get_code_at(predicted).await?;
    if !code.is_empty() {
        return Ok((predicted, None));
    }

    let pending = f
        .createAccount(owner, U256::from(ACCOUNT_SALT), init)
        .send()
        .await?;
    let tx = format!("{:?}", pending.tx_hash());
    Ok((predicted, Some(tx)))
}

pub async fn ensure_session(
    rpc_url: &str,
    owner_signer: PrivateKeySigner,
    diamond: Address,
    agent: Address,
    expires_at: u64,
    selectors: &[FixedBytes<4>],
) -> Result<Option<String>> {
    let provider = ProviderBuilder::new()
        .wallet(owner_signer)
        .connect(rpc_url)
        .await?;
    let sec = ISecurityFacet::new(diamond, provider);

    let mut all_valid = true;
    for s in selectors {
        if !sec.isSessionValid(agent, *s).call().await? {
            all_valid = false;
            break;
        }
    }
    if all_valid {
        return Ok(None);
    }

    let sel_vec: Vec<FixedBytes<4>> = selectors.to_vec();
    let pending = sec
        .registerSession(agent, expires_at, sel_vec)
        .send()
        .await?;
    Ok(Some(format!("{:?}", pending.tx_hash())))
}

pub fn arc_agent_selectors() -> Vec<FixedBytes<4>> {
    vec![
        FixedBytes::<4>::from(IGatewayFacet::depositToGatewayCall::SELECTOR),
        FixedBytes::<4>::from(IGatewayFacet::withdrawFromGatewayCall::SELECTOR),
    ]
}

pub fn arbitrum_agent_selectors() -> Vec<FixedBytes<4>> {
    vec![
        FixedBytes::<4>::from(IAaveFacet::supplyAaveCall::SELECTOR),
        FixedBytes::<4>::from(IAaveFacet::withdrawAaveCall::SELECTOR),
    ]
}

pub fn validate_init(init: &InitArgs) -> Result<()> {
    if init.entryPoint == Address::ZERO {
        return Err(anyhow!("InitArgs.entryPoint is zero"));
    }
    if init.usdc == Address::ZERO {
        return Err(anyhow!("InitArgs.usdc is zero"));
    }
    Ok(())
}
