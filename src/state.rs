use std::sync::Arc;

use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;

use sqlx::PgPool;

use crate::automation::audit::{AuditStore, InMemoryAuditStore, PostgresAuditStore};
use crate::automation::funding::{FundingStore, InMemoryFundingStore, PostgresFundingStore};
use crate::automation::chat_history::{
    ChatHistoryStore, InMemoryChatHistoryStore, PostgresChatHistoryStore,
};
use crate::automation::policy::{InMemoryPolicyStore, PolicyStore, PostgresPolicyStore};
use crate::automation::position::{InMemoryPositionStore, PositionFetcher, PositionStore};
use crate::automation::session_cache::{
    InMemorySessionCacheStore, PostgresSessionCacheStore, SessionCacheStore,
};
use crate::automation::snapshot::{InMemorySnapshotStore, SnapshotStore};
use crate::automation::workers::position::RpcPositionFetcher;
use crate::chain::{arc::ArcClient, arbitrum_sepolia::ArbitrumSepoliaClient};
use crate::config::AppConfig;
use crate::core::llm::config::build_llm_from_env;
use crate::core::llm::provider::LlmProvider;
use crate::gateway::api::GatewayApi;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<AppConfig>,
    pub user_signer: Arc<PrivateKeySigner>,
    pub user_address: Address,
    pub agent_signer: Arc<PrivateKeySigner>,
    pub agent_address: Address,
    pub arc: Arc<ArcClient>,
    pub arbitrum_sepolia: Arc<ArbitrumSepoliaClient>,
    pub gateway: Arc<GatewayApi>,
    pub llm: Arc<dyn LlmProvider>,
    pub policies: Arc<dyn PolicyStore>,
    pub snapshots: Arc<dyn SnapshotStore>,
    pub positions: Arc<dyn PositionStore>,
    pub position_fetcher: Arc<dyn PositionFetcher>,
    pub audit: Arc<dyn AuditStore>,
    pub funding: Arc<dyn FundingStore>,
    pub session_cache: Arc<dyn SessionCacheStore>,
    pub chat_history: Arc<dyn ChatHistoryStore>,
}

impl AppState {
    pub async fn new(cfg: AppConfig) -> Result<Self> {
        let user_signer: PrivateKeySigner = cfg.user_pk.parse()?;
        let agent_signer: PrivateKeySigner = cfg.agent_pk.parse()?;
        let user_address = user_signer.address();
        let agent_address = agent_signer.address();

        let arc = Arc::new(ArcClient::connect(&cfg.arc_rpc_url, agent_signer.clone()).await?);
        let arb = Arc::new(
            ArbitrumSepoliaClient::connect(&cfg.arbitrum_sepolia_rpc_url, agent_signer.clone()).await?,
        );
        let gateway = GatewayApi::new(&cfg.gateway_api_url, &cfg.gateway_api_key);
        let llm = build_llm_from_env();
        tracing::info!(llm.primary = %llm.name(), llm.model = %llm.model(), "[STATE] llm stack ready");
        let cfg = Arc::new(cfg);
        let position_fetcher: Arc<dyn PositionFetcher> = Arc::new(RpcPositionFetcher {
            cfg: cfg.clone(),
            arc: arc.clone(),
            arb: arb.clone(),
        });

        let (policies, audit, session_cache, chat_history, funding): (
            Arc<dyn PolicyStore>,
            Arc<dyn AuditStore>,
            Arc<dyn SessionCacheStore>,
            Arc<dyn ChatHistoryStore>,
            Arc<dyn FundingStore>,
        ) = match &cfg.database_url {
            Some(url) => {
                let pool = PgPool::connect(url).await?;
                tracing::info!("connected to Postgres");
                (
                    Arc::new(PostgresPolicyStore::new(pool.clone())),
                    Arc::new(PostgresAuditStore::new(pool.clone())),
                    Arc::new(PostgresSessionCacheStore::new(pool.clone())),
                    Arc::new(PostgresChatHistoryStore::new(pool.clone())),
                    Arc::new(PostgresFundingStore::new(pool)),
                )
            }
            None => {
                tracing::warn!(
                    "DATABASE_URL not set — using in-memory stores (state lost on restart)"
                );
                (
                    Arc::new(InMemoryPolicyStore::new()),
                    Arc::new(InMemoryAuditStore::new()),
                    Arc::new(InMemorySessionCacheStore::new()),
                    Arc::new(InMemoryChatHistoryStore::new()),
                    Arc::new(InMemoryFundingStore::new()),
                )
            }
        };

        Ok(Self {
            cfg,
            user_signer: Arc::new(user_signer),
            user_address,
            agent_signer: Arc::new(agent_signer),
            agent_address,
            arc,
            arbitrum_sepolia: arb,
            gateway: Arc::new(gateway),
            llm,
            policies,
            snapshots: Arc::new(InMemorySnapshotStore::new()),
            positions: Arc::new(InMemoryPositionStore::new()),
            position_fetcher,
            audit,
            funding,
            session_cache,
            chat_history,
        })
    }

}
