use alloy::primitives::{Address, Bytes, FixedBytes};
use alloy::providers::ProviderBuilder;
use alloy::rpc::types::Log;
use alloy::signers::Signer;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol_types::SolEvent;
use anyhow::{Result, anyhow};

use crate::contracts::{IEntryPoint, PackedUserOperation};

pub async fn sign_and_submit(
    rpc_url: &str,
    relayer: PrivateKeySigner,
    agent_signer: &PrivateKeySigner,
    entry_point: Address,
    mut userop: PackedUserOperation,
    beneficiary: Address,
) -> Result<String> {
    let provider = ProviderBuilder::new()
        .wallet(relayer)
        .connect(rpc_url)
        .await?;
    let ep = IEntryPoint::new(entry_point, provider);

    let userop_hash = ep.getUserOpHash(userop.clone()).call().await?;
    let sig = agent_signer.sign_message(userop_hash.as_slice()).await?;
    userop.signature = Bytes::from(sig.as_bytes().to_vec());

    let pending = ep.handleOps(vec![userop], beneficiary).send().await?;
    let tx_hash = format!("{:?}", pending.tx_hash());
    let receipt = pending.get_receipt().await?;
    if !receipt.status() {
        return Err(anyhow!(
            "handleOps tx reverted (tx={tx_hash}) — likely validation \
             failure (AA-prefixed error). Check `cast tx {tx_hash}` for \
             FailedOp / FailedOpWithRevert calldata.",
        ));
    }

    verify_userop_succeeded(receipt.inner.logs(), userop_hash, &tx_hash)?;
    Ok(tx_hash)
}

fn verify_userop_succeeded(
    logs: &[Log],
    expected_hash: FixedBytes<32>,
    tx_hash: &str,
) -> Result<()> {
    let event_sig = IEntryPoint::UserOperationEvent::SIGNATURE_HASH;
    let revert_sig = IEntryPoint::UserOperationRevertReason::SIGNATURE_HASH;

    for log in logs {
        let topics = log.topics();
        if topics.first() != Some(&event_sig) {
            continue;
        }
        if topics.get(1) != Some(&expected_hash) {
            continue;
        }
        let decoded = IEntryPoint::UserOperationEvent::decode_log(&log.inner)
            .map_err(|e| anyhow!("decode UserOperationEvent: {e}"))?;
        if decoded.success {
            return Ok(());
        }

        let reason = logs
            .iter()
            .find(|l| {
                l.topics().first() == Some(&revert_sig)
                    && l.topics().get(1) == Some(&expected_hash)
            })
            .and_then(|l| {
                IEntryPoint::UserOperationRevertReason::decode_log(&l.inner).ok()
            })
            .map(|d| decode_revert_string(&d.revertReason))
            .unwrap_or_else(|| "<no revert reason emitted>".into());
        return Err(anyhow!(
            "userOp reverted (tx={tx_hash}, hash={expected_hash:?}): {reason}"
        ));
    }
    Err(anyhow!(
        "no UserOperationEvent for hash {expected_hash:?} in tx {tx_hash} \
         receipt — EntryPoint may have skipped this op or address mismatch"
    ))
}

fn decode_revert_string(raw: &Bytes) -> String {
    if raw.len() >= 4 && &raw[..4] == [0x08, 0xc3, 0x79, 0xa0] {
        if let Ok(s) = <(String,) as alloy::sol_types::SolValue>::abi_decode(&raw[4..]) {
            return s.0;
        }
    }
    let preview = &raw[..raw.len().min(64)];
    format!("0x{}", hex::encode(preview))
}
