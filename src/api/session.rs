use std::str::FromStr;

use alloy::primitives::{Address, Bytes, FixedBytes, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::TransactionRequest;
use alloy::sol_types::SolCall;
use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::account::{
    ACCOUNT_SALT, arc_agent_selectors, arbitrum_agent_selectors, deploy, ensure_session,
    is_deployed, predict_address, validate_init,
};
use crate::api::error::ApiError;
use crate::contracts::{IAaveFacet, IAccount4337Facet, IGatewayFacet, ISecurityFacet, InitArgs};
use crate::gateway::contracts::IGatewayWallet;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/session/{user}", get(get_status))
        .route("/session/{user}/setup", post(setup_session))
}

#[derive(Serialize, Deserialize)]
pub struct DiamondStatus {
    pub chain: String,
    pub address: String,
    pub deployed: bool,
    pub session_valid: bool,
    pub session_expires_at: u64,
}

#[derive(Serialize, Deserialize)]
pub struct SessionStatus {
    pub user: String,
    pub agent: String,
    pub salt: u64,
    pub arc: DiamondStatus,
    pub arbitrum_sepolia: DiamondStatus,
    pub addresses_match: bool,
    pub ready: bool,
}

async fn get_status(
    State(state): State<AppState>,
    Path(user): Path<Address>,
) -> Result<Json<SessionStatus>, ApiError> {
    let now = chrono::Utc::now();
    if let Some(cached) = state.session_cache.get_fresh(user, now).await? {
        if let Ok(status) = serde_json::from_value::<SessionStatus>(cached) {
            return Ok(Json(status));
        }
    }
    let status = compute_status(&state, user).await?;
    let _ = state
        .session_cache
        .put(user, serde_json::to_value(&status)?)
        .await;
    Ok(Json(status))
}

#[derive(Serialize)]
pub struct SetupResult {
    pub status: SessionStatus,
    pub deploy: SetupTxs,
    pub session: SetupTxs,
}

#[derive(Serialize)]
pub struct SetupTxs {
    pub arc_tx: Option<String>,
    pub arb_tx: Option<String>,
}

async fn setup_session(
    State(state): State<AppState>,
    Path(user): Path<Address>,
) -> Result<Json<SetupResult>, ApiError> {
    let cfg = &state.cfg;
    let arc_factory = Address::from_str(&cfg.arc_factory)?;
    let arb_factory = Address::from_str(&cfg.arbitrum_sepolia_factory)?;

    let upgrade_authority = Address::from_str(&cfg.compass_upgrade_authority)?;

    let arc_paymaster = cfg
        .arc_paymaster
        .as_deref()
        .map(Address::from_str)
        .transpose()?
        .unwrap_or(Address::ZERO);
    let arb_paymaster = cfg
        .arbitrum_sepolia_paymaster
        .as_deref()
        .map(Address::from_str)
        .transpose()?
        .unwrap_or(Address::ZERO);

    let arc_init = InitArgs {
        entryPoint: Address::from_str(&cfg.arc_entry_point)?,
        usdc: Address::from_str(&cfg.arc_usdc)?,
        gatewayWallet: Address::from_str(&cfg.gateway_wallet)?,
        gatewayMinter: Address::from_str(&cfg.gateway_minter)?,
        aavePool: Address::ZERO,
        upgradeAuthority: upgrade_authority,
        paymaster: arc_paymaster,
    };
    validate_init(&arc_init)?;

    let arb_init = InitArgs {
        entryPoint: Address::from_str(&cfg.arbitrum_sepolia_entry_point)?,
        usdc: Address::from_str(&cfg.arbitrum_sepolia_aave_usdc)?,
        gatewayWallet: Address::from_str(&cfg.gateway_wallet)?,
        gatewayMinter: Address::from_str(&cfg.gateway_minter)?,
        aavePool: Address::from_str(&cfg.arbitrum_sepolia_aave_pool)?,
        upgradeAuthority: upgrade_authority,
        paymaster: arb_paymaster,
    };
    validate_init(&arb_init)?;

    let (arc_diamond, arc_deploy_tx) = deploy(
        &cfg.arc_rpc_url,
        state.agent_signer.as_ref().clone(),
        arc_factory,
        user,
        arc_init,
    )
    .await?;
    let (arb_diamond, arb_deploy_tx) = deploy(
        &cfg.arbitrum_sepolia_rpc_url,
        state.agent_signer.as_ref().clone(),
        arb_factory,
        user,
        arb_init,
    )
    .await?;

    let expires_at = (chrono::Utc::now().timestamp() as u64) + 86_400;

    let arc_session_tx = ensure_session(
        &cfg.arc_rpc_url,
        state.user_signer.as_ref().clone(),
        arc_diamond,
        state.agent_address,
        expires_at,
        &arc_agent_selectors(),
    )
    .await?;
    let arb_session_tx = ensure_session(
        &cfg.arbitrum_sepolia_rpc_url,
        state.user_signer.as_ref().clone(),
        arb_diamond,
        state.agent_address,
        expires_at,
        &arbitrum_agent_selectors(),
    )
    .await?;

    authorize_arc_gateway_delegate(&state, arc_diamond).await?;

    let _ = state.session_cache.invalidate(user).await;

    let status = compute_status(&state, user).await?;
    let _ = state
        .session_cache
        .put(user, serde_json::to_value(&status)?)
        .await;

    Ok(Json(SetupResult {
        status,
        deploy: SetupTxs {
            arc_tx: arc_deploy_tx,
            arb_tx: arb_deploy_tx,
        },
        session: SetupTxs {
            arc_tx: arc_session_tx,
            arb_tx: arb_session_tx,
        },
    }))
}

async fn compute_status(state: &AppState, user: Address) -> Result<SessionStatus> {
    let cfg = &state.cfg;
    let arc_factory = Address::from_str(&cfg.arc_factory)?;
    let arb_factory = Address::from_str(&cfg.arbitrum_sepolia_factory)?;

    let arc_selector = FixedBytes::<4>::from(IGatewayFacet::depositToGatewayCall::SELECTOR);
    let arb_selector = FixedBytes::<4>::from(IAaveFacet::supplyAaveCall::SELECTOR);

    let (arc_result, arb_result) = tokio::join!(
        probe_chain(
            &cfg.arc_rpc_url,
            arc_factory,
            user,
            state.agent_address,
            arc_selector,
        ),
        probe_chain(
            &cfg.arbitrum_sepolia_rpc_url,
            arb_factory,
            user,
            state.agent_address,
            arb_selector,
        ),
    );
    let (arc_diamond, arc_live, arc_session_valid, arc_expires) = arc_result?;
    let (arb_diamond, arb_live, arb_session_valid, arb_expires) = arb_result?;

    let addresses_match = arc_diamond == arb_diamond;
    let ready = arc_live && arb_live && arc_session_valid && arb_session_valid;

    Ok(SessionStatus {
        user: format!("{user:?}"),
        agent: format!("{:?}", state.agent_address),
        salt: ACCOUNT_SALT,
        arc: DiamondStatus {
            chain: "arc".into(),
            address: format!("{arc_diamond:?}"),
            deployed: arc_live,
            session_valid: arc_session_valid,
            session_expires_at: arc_expires,
        },
        arbitrum_sepolia: DiamondStatus {
            chain: "arbitrum_sepolia".into(),
            address: format!("{arb_diamond:?}"),
            deployed: arb_live,
            session_valid: arb_session_valid,
            session_expires_at: arb_expires,
        },
        addresses_match,
        ready,
    })
}

pub async fn authorize_arc_gateway_delegate(
    state: &AppState,
    arc_diamond: Address,
) -> Result<()> {
    let cfg = &state.cfg;
    let gateway_wallet = Address::from_str(&cfg.gateway_wallet)?;
    let usdc = Address::from_str(&cfg.arc_usdc)?;

    let inner: Bytes = IGatewayWallet::addDelegateCall {
        token: usdc,
        delegate: state.agent_address,
    }
    .abi_encode()
    .into();
    let outer: Bytes = IAccount4337Facet::executeCall {
        target: gateway_wallet,
        value: U256::ZERO,
        data: inner,
    }
    .abi_encode()
    .into();

    let provider = ProviderBuilder::new()
        .wallet(state.user_signer.as_ref().clone())
        .connect(&cfg.arc_rpc_url)
        .await?;
    let tx = TransactionRequest::default()
        .to(arc_diamond)
        .input(outer.into());
    let pending = provider.send_transaction(tx).await?;
    let _receipt = pending.get_receipt().await?;
    Ok(())
}

async fn probe_session(
    rpc: &str,
    diamond: Address,
    agent: Address,
    selector: FixedBytes<4>,
) -> Result<(bool, u64)> {
    let provider = ProviderBuilder::new().connect(rpc).await?;
    let sec = ISecurityFacet::new(diamond, provider);
    let valid = sec.isSessionValid(agent, selector).call().await?;
    let expiry: u64 = sec
        .sessionExpiry(agent)
        .call()
        .await
        .map(|v| v.try_into().unwrap_or(0))
        .unwrap_or(0);
    Ok((valid, expiry))
}

async fn probe_chain(
    rpc: &str,
    factory: Address,
    user: Address,
    agent: Address,
    selector: FixedBytes<4>,
) -> Result<(Address, bool, bool, u64)> {
    let diamond = predict_address(rpc, factory, user).await?;
    let live = is_deployed(rpc, diamond).await?;
    let (session_valid, expires) = if live {
        probe_session(rpc, diamond, agent, selector)
            .await
            .unwrap_or((false, 0))
    } else {
        (false, 0)
    };
    Ok((diamond, live, session_valid, expires))
}
