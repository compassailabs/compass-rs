pub mod aave_tools;
pub mod account_tools;
pub mod gateway_tools;
pub mod market_tools;
pub mod skill_tools;

use serde_json::{Value, json};

use super::types::ToolSchema;

pub fn registry() -> Vec<ToolSchema> {
    vec![
        ToolSchema {
            name: "load_skill".into(),
            description:
                "Read the body of an on-demand skill doc by namespace key (e.g. 'tools/supply-aave'). \
                 Call this before invoking a write-side tool you haven't used yet so you can verify the steps."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Skill key, e.g. tools/supply-aave" }
                },
                "required": ["name"]
            }),
        },
        ToolSchema {
            name: "account_status".into(),
            description:
                "Predict + report the Keeper's diamond addresses on Arc and Arbitrum Sepolia, and whether \
                 each is deployed. Use first on any new Keeper to decide if setup is required."
                    .into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSchema {
            name: "ensure_account".into(),
            description:
                "Idempotent setup: deploy the Arc and Arbitrum diamonds if missing, and register an active \
                 session-key for the agent on each so subsequent UserOps validate. Safe to call repeatedly; \
                 only writes when something is actually missing."
                    .into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSchema {
            name: "check_balances".into(),
            description:
                "Read the Keeper's diamond USDC balances on Arc and Arbitrum Sepolia. \
                 Funds live in the diamonds, NOT in the Keeper's EOA."
                    .into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSchema {
            name: "get_aave_apr".into(),
            description: "Read AAVE v3 current supply APR for the USDC reserve on Arbitrum Sepolia.".into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSchema {
            name: "deposit_to_gateway".into(),
            description:
                "UserOp on the Arc diamond → GatewayFacet.depositToGateway(amount). Moves USDC from the \
                 diamond into the Keeper's Circle Gateway unified balance. First step of a cross-chain move."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "amount_usdc": { "type": "string", "description": "6-decimal raw USDC units (string)." }
                },
                "required": ["amount_usdc"]
            }),
        },
        ToolSchema {
            name: "burn_intent_to_attestation".into(),
            description:
                "Off-chain: sign EIP-712 BurnIntent (depositor = Arc diamond, recipient = destination \
                 diamond) and POST to Circle Gateway. Returns the attestation pair to feed into \
                 mint_on_destination."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "amount_usdc": { "type": "string" },
                    "destination_domain": { "type": "integer", "description": "Circle CCTP domain (Arbitrum Sepolia = 3)." }
                },
                "required": ["amount_usdc", "destination_domain"]
            }),
        },
        ToolSchema {
            name: "mint_on_destination".into(),
            description:
                "Direct EOA call to GatewayMinter on the destination chain. The attestation embeds the \
                 recipient address so the minted USDC lands in the destination diamond regardless of relayer."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "destination_domain": { "type": "integer" },
                    "attestation": { "type": "string" },
                    "signature": { "type": "string" }
                },
                "required": ["destination_domain", "attestation", "signature"]
            }),
        },
        ToolSchema {
            name: "supply_aave".into(),
            description:
                "UserOp on the Arbitrum diamond → AaveFacet.supplyAave(amount). Diamond handles approve+supply \
                 internally and receives the aToken position."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "amount_usdc": { "type": "string" }
                },
                "required": ["amount_usdc"]
            }),
        },
        ToolSchema {
            name: "withdraw_aave".into(),
            description:
                "UserOp on the Arbitrum diamond → AaveFacet.withdrawAave(amount). Pass `\"max\"` to exit fully."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "amount_usdc": { "type": "string" }
                },
                "required": ["amount_usdc"]
            }),
        },
    ]
}

pub(crate) fn parse_u256_usdc(v: Option<&Value>) -> anyhow::Result<alloy::primitives::U256> {
    let s = v
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing amount_usdc"))?;
    alloy::primitives::U256::from_str_radix(s, 10)
        .map_err(|e| anyhow::anyhow!("amount_usdc not a decimal integer: {e}"))
}
