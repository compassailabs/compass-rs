use alloy::primitives::Address;
use anyhow::{Result, anyhow};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::intent::SignedBurnIntent;

pub struct GatewayApi {
    base: String,
    key: String,
    http: Client,
}

#[derive(Debug, Deserialize)]
pub struct GatewayInfo {
    pub domains: Vec<DomainInfo>,
}

#[derive(Debug, Deserialize)]
pub struct DomainInfo {
    pub domain: u32,
    pub chain: String,
    #[serde(default)]
    pub gateway_wallet: Option<String>,
    #[serde(default)]
    pub gateway_minter: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Balances {
    #[serde(default)]
    pub balances: Vec<BalanceEntry>,
}

#[derive(Debug, Deserialize)]
pub struct BalanceEntry {
    pub domain: u32,
    pub amount: String,
}

#[derive(Debug, Deserialize)]
pub struct TransferAttestation {
    pub attestation: String,
    pub signature: String,
}

#[derive(Debug, Serialize)]
struct SignedBurnIntentBody {
    #[serde(rename = "burnIntent")]
    burn_intent: Value,
    signature: String,
}

impl GatewayApi {
    pub fn new(base: &str, key: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            key: key.to_string(),
            http: Client::new(),
        }
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.key.is_empty() {
            req
        } else {
            req.bearer_auth(&self.key)
        }
    }

    pub async fn info(&self) -> Result<GatewayInfo> {
        let url = format!("{}/v1/info", self.base);
        let resp = self.auth(self.http.get(url)).send().await?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "gateway /v1/info failed: {} {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        Ok(resp.json().await?)
    }

    pub async fn balances(&self, depositor: Address) -> Result<Balances> {
        let url = format!("{}/v1/balances", self.base);
        let body = serde_json::json!({ "depositor": format!("{depositor:?}") });
        let resp = self.auth(self.http.post(url)).json(&body).send().await?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "gateway /v1/balances failed: {} {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        Ok(resp.json().await?)
    }

    pub async fn transfer(&self, signed: &SignedBurnIntent) -> Result<TransferAttestation> {
        let intent = &signed.intent;
        let spec = &intent.spec;
        let burn_intent_value = serde_json::json!({
            "maxBlockHeight": intent.maxBlockHeight.to_string(),
            "maxFee": intent.maxFee.to_string(),
            "spec": {
                "version": spec.version,
                "sourceDomain": spec.sourceDomain,
                "destinationDomain": spec.destinationDomain,
                "sourceContract": format!("{:?}", spec.sourceContract),
                "destinationContract": format!("{:?}", spec.destinationContract),
                "sourceToken": format!("{:?}", spec.sourceToken),
                "destinationToken": format!("{:?}", spec.destinationToken),
                "sourceDepositor": format!("{:?}", spec.sourceDepositor),
                "destinationRecipient": format!("{:?}", spec.destinationRecipient),
                "sourceSigner": format!("{:?}", spec.sourceSigner),
                "destinationCaller": format!("{:?}", spec.destinationCaller),
                "value": spec.value.to_string(),
                "salt": format!("{:?}", spec.salt),
                "hookData": format!("0x{}", hex::encode(spec.hookData.as_ref())),
            }
        });
        let item = SignedBurnIntentBody {
            burn_intent: burn_intent_value,
            signature: format!("0x{}", hex::encode(&signed.signature)),
        };
        let body = vec![item];
        let url = format!("{}/v1/transfer", self.base);
        let resp = self.auth(self.http.post(url)).json(&body).send().await?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "gateway /v1/transfer failed: {} {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let raw: Value = resp.json().await?;
        if let Some(first) = raw.as_array().and_then(|a| a.first()) {
            return Ok(serde_json::from_value(first.clone())?);
        }
        Ok(serde_json::from_value(raw)?)
    }

    pub async fn raw_get(&self, path: &str) -> Result<Value> {
        let url = format!("{}{}", self.base, path);
        Ok(self.auth(self.http.get(url)).send().await?.json().await?)
    }
}
