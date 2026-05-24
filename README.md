# Compass AI Backend

Rust backend for **Compass AI**, a multi-chain USDC yield aggregator
managed by an AI agent across Arc.

The service serves the streaming chat API, manages user sessions and
policies, drives Circle Gateway bridging, and runs the cron loop that
rebalances yield across venues like AAVE via ERC-4337 UserOperations.

## Repository Structure

```
src/
├── main.rs               // axum entrypoint, worker spawn
├── config.rs             // env-driven AppConfig
├── state.rs              // shared AppState
├── api/                  // HTTP routes (chat, session, policy, balance, ...)
├── automation/           // cron, executor, evaluator, snapshot, audit
├── chain/                // Arc + Arbitrum Sepolia providers
├── contracts/            // alloy sol! bindings (Diamond, EntryPoint, AAVE)
├── core/llm/             // Anthropic streaming client + tool loop
├── gateway/              // Circle Gateway intent + API client
├── skills/               // chat tools (onchain, setup, strategies)
├── userop/               // ERC-4337 builder, paymaster, submit
└── aave/                 // AAVE pool helpers
```

## Local Development

```bash
cargo run
```

The service binds to `0.0.0.0:8787` by default. Configure via `.env`:

```
ARC_RPC_URL=...
ARC_USDC_ADDRESS=0x...
ARC_FACTORY_ADDRESS=0x...
ARC_ENTRY_POINT=0x...

ARBITRUM_SEPOLIA_RPC_URL=...
ARBITRUM_SEPOLIA_AAVE_POOL=0x...
ARBITRUM_SEPOLIA_AAVE_USDC=0x...
ARBITRUM_SEPOLIA_FACTORY_ADDRESS=0x...
ARBITRUM_SEPOLIA_ENTRY_POINT=0x...
ARBITRUM_SEPOLIA_PAYMASTER=0x...

COMPASS_USER_PK=0x...
COMPASS_AGENT_PK=0x...
COMPASS_UPGRADE_AUTHORITY=0x...

GATEWAY_API_URL=https://gateway-api-testnet.circle.com
GATEWAY_WALLET_ADDRESS=0x...
GATEWAY_MINTER_ADDRESS=0x...

ANTHROPIC_API_KEY=...
DATABASE_URL=postgres://...
```

## License

MIT — see [LICENSE](./LICENSE).
