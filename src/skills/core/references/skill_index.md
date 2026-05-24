# Skill Index

## Tools you call directly

### Setup
- `account_status` — Predict Diamond addresses on Arc + Arbitrum Sepolia and
  report deployment status. Use first on any new Keeper.
- `ensure_account` — Idempotent: deploys missing Diamonds and registers
  the agent's session key on each. Call when `account_status` shows
  anything missing.

### Observation
- `check_balances` — USDC in each Diamond on Arc + Arbitrum Sepolia.
- `get_aave_apr` — Current AAVE v3 USDC supply APR on Arbitrum Sepolia.

### Cross-chain move (3 steps, in order)
- `deposit_to_gateway` — UserOp on Arc Diamond → GatewayFacet. Moves
  USDC from the Diamond into the Keeper's Circle unified balance. Args:
  `amount_usdc`.
- `burn_intent_to_attestation` — Off-chain. Signs EIP-712 BurnIntent
  with the agent's session key, POSTs to Circle, returns
  `{attestation, signature}`. Args: `amount_usdc`, `destination_domain`.
- `mint_on_destination` — EOA relay tx → GatewayMinter on the
  destination. Mints USDC into the destination Diamond. Args:
  `destination_domain`, `attestation`, `signature`.

### Allocation
- `supply_aave` — UserOp on Arbitrum Diamond → AaveFacet. Args: `amount_usdc`.
- `withdraw_aave` — UserOp on Arbitrum Diamond → AaveFacet. Args:
  `amount_usdc` (or `"max"`).

## On-demand skill docs (read via `load_skill`)
- `tools/check-balances`   — interpretation of the balance report.
- `tools/move-funds`       — the 3-step Gateway flow in detail.
- `tools/supply-aave`      — supply procedure + AAVE-USDC reserve gotcha.
- `tools/withdraw-aave`    — withdraw procedure, including `"max"` semantics.

## Domain IDs (Circle CCTP / Gateway)
- Arc testnet:      26
- Arbitrum Sepolia: 3
- Ethereum Sepolia:  0
- Arbitrum Sepolia:  3
- Avalanche Fuji:    1

MVP only wires Arc (26) ↔ Arbitrum Sepolia (3).
