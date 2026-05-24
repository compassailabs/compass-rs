use alloy::primitives::{Address, B256, Bytes, U256, Uint};
use alloy::providers::ProviderBuilder;
use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;

use crate::contracts::{IEntryPoint, PackedUserOperation};
use crate::userop::paymaster::PaymasterConfig;

pub const DEFAULT_VERIFICATION_GAS: u128 = 150_000;
pub const DEFAULT_CALL_GAS: u128 = 300_000;
pub const DEFAULT_PRE_VERIFICATION_GAS: u128 = 80_000;
pub const DEFAULT_MAX_PRIORITY_FEE: u128 = 100_000_000;   // 0.1 gwei
pub const DEFAULT_MAX_FEE: u128 = 500_000_000;            // 0.5 gwei (typical Arbitrum L2)

pub fn pack_gas_uint128_pair(hi: u128, lo: u128) -> B256 {
    let mut out = [0u8; 32];
    out[..16].copy_from_slice(&hi.to_be_bytes());
    out[16..].copy_from_slice(&lo.to_be_bytes());
    B256::from(out)
}

pub async fn build_userop(
    rpc_url: &str,
    signer: PrivateKeySigner,
    entry_point: Address,
    sender_account: Address,
    call_data: Bytes,
    paymaster: Option<PaymasterConfig>,
) -> Result<PackedUserOperation> {
    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect(rpc_url)
        .await?;
    let ep = IEntryPoint::new(entry_point, provider);

    let nonce = ep
        .getNonce(sender_account, Uint::<192, 3>::ZERO)
        .call()
        .await?;

    let paymaster_and_data = match paymaster {
        Some(pm) => pm.pack(),
        None => Bytes::new(),
    };

    Ok(PackedUserOperation {
        sender: sender_account,
        nonce,
        initCode: Bytes::new(),
        callData: call_data,
        accountGasLimits: pack_gas_uint128_pair(
            DEFAULT_VERIFICATION_GAS,
            DEFAULT_CALL_GAS,
        ),
        preVerificationGas: U256::from(DEFAULT_PRE_VERIFICATION_GAS),
        gasFees: pack_gas_uint128_pair(DEFAULT_MAX_PRIORITY_FEE, DEFAULT_MAX_FEE),
        paymasterAndData: paymaster_and_data,
        signature: Bytes::new(),
    })
}
