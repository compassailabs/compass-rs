use alloy::primitives::{Address, B256, Bytes, U256};
use alloy::signers::Signer;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use alloy::sol_types::{SolStruct, eip712_domain};
use anyhow::Result;
use rand::RngCore;

sol! {
    #[derive(Debug)]
    struct TransferSpec {
        uint32 version;
        uint32 sourceDomain;
        uint32 destinationDomain;
        bytes32 sourceContract;
        bytes32 destinationContract;
        bytes32 sourceToken;
        bytes32 destinationToken;
        bytes32 sourceDepositor;
        bytes32 destinationRecipient;
        bytes32 sourceSigner;
        bytes32 destinationCaller;
        uint256 value;
        bytes32 salt;
        bytes hookData;
    }

    #[derive(Debug)]
    struct BurnIntent {
        uint256 maxBlockHeight;
        uint256 maxFee;
        TransferSpec spec;
    }
}

pub struct SignedBurnIntent {
    pub intent: BurnIntent,
    pub signature: Vec<u8>,
    pub digest: B256,
}

pub async fn sign_burn_intent(
    signer: &PrivateKeySigner,
    intent: BurnIntent,
) -> Result<SignedBurnIntent> {
    let domain = eip712_domain! {
        name: "GatewayWallet",
        version: "1",
    };

    let digest = intent.eip712_signing_hash(&domain);
    let sig = signer.sign_hash(&digest).await?;
    Ok(SignedBurnIntent {
        intent,
        signature: sig.as_bytes().to_vec(),
        digest,
    })
}

pub fn addr_to_bytes32(a: Address) -> B256 {
    let mut out = [0u8; 32];
    out[12..].copy_from_slice(a.as_slice());
    B256::from(out)
}

pub struct IntentArgs {
    pub source_depositor: Address,
    pub destination_recipient: Address,
    pub source_token: Address,
    pub destination_token: Address,
    pub source_contract: Address,
    pub destination_contract: Address,
    pub source_signer: Address,
    pub destination_caller: Address,
    pub source_domain: u32,
    pub destination_domain: u32,
    pub amount: U256,
}

const DEFAULT_MAX_FEE_RAW_USDC: u64 = 1_000_000;

pub fn build_intent(args: IntentArgs) -> BurnIntent {
    let mut salt_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut salt_bytes);

    let spec = TransferSpec {
        version: 1,
        sourceDomain: args.source_domain,
        destinationDomain: args.destination_domain,
        sourceContract: addr_to_bytes32(args.source_contract),
        destinationContract: addr_to_bytes32(args.destination_contract),
        sourceToken: addr_to_bytes32(args.source_token),
        destinationToken: addr_to_bytes32(args.destination_token),
        sourceDepositor: addr_to_bytes32(args.source_depositor),
        destinationRecipient: addr_to_bytes32(args.destination_recipient),
        sourceSigner: addr_to_bytes32(args.source_signer),
        destinationCaller: addr_to_bytes32(args.destination_caller),
        value: args.amount,
        salt: B256::from(salt_bytes),
        hookData: Bytes::new(),
    };

    BurnIntent {
        maxBlockHeight: U256::MAX,
        maxFee: U256::from(DEFAULT_MAX_FEE_RAW_USDC),
        spec,
    }
}
