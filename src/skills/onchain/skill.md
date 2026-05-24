---
name: compass-onchain
description: Strategy agent's execution module — UserOp construction for AAVE supply/withdraw plus the 3-step Circle Gateway cross-chain flow. Enforces a strict 5-step pipeline with safety gate.
metadata:
  pattern: pipeline
  steps: "5"
  domain: onchain-operations
---

# Onchain Skill — Pipeline

This module covers every state-changing onchain operation the strategy
agent can take. It does NOT include account setup (see `setup/skill.md`)
or read-only state queries (those return JSON directly without going
through this pipeline).

---

## When to Load References

| When the Keeper wants to... | Load this reference |
|---|---|
| See balances on each Diamond | `references/check-balances.md` |
| Move USDC from Arc to Arbitrum (the 3-step Gateway flow in detail) | `references/move-funds.md` |
| Supply USDC into AAVE on Arbitrum | `references/supply-aave.md` |
| Withdraw USDC from AAVE on Arbitrum (incl. `"max"` semantics) | `references/withdraw-aave.md` |

Load the matching reference **before** building any UserOp. The
references contain the args, the post-execution checks, and the gotchas
specific to that operation.

---

## Tools You Call Directly

| Tool | Where it runs | Args |
|---|---|---|
| `check_balances` | read-only RPC | none |
| `get_aave_apr` | read-only RPC | none |
| `deposit_to_gateway` | UserOp on Arc Diamond | `amount_usdc` |
| `burn_intent_to_attestation` | off-chain Circle POST | `amount_usdc`, `destination_domain` |
| `mint_on_destination` | direct EOA tx on destination | `destination_domain`, `attestation`, `signature` |
| `supply_aave` | UserOp on Arbitrum Diamond | `amount_usdc` |
| `withdraw_aave` | UserOp on Arbitrum Diamond | `amount_usdc` (or `"max"`) |

---

## Execution Pipeline — 5 Strict Steps

State-changing operations MUST follow these steps in order. Do not skip.
Do not advance past a failed step.

### Step 1 — Intent Clarification

Parse the request: verb + asset + amount + (optional) source/destination
chain. If anything is ambiguous, ask one focused question. Don't guess
on amount.

### Step 2 — Read Before Write

Call `check_balances` + `get_aave_apr` so the proposal is grounded in
current data. Reject (with a clear explanation) any move that exceeds
the Diamond's source-chain balance or would deposit < 1 USDC (testnet
dust isn't worth the gas).

### Step 3 — Safety Gate

Load `core/references/safety.md`. Verify every constraint applies.
Surface to the Keeper:
- exact action (verb + asset + amount)
- destination Diamond + chain
- estimated gas + bridge fee in USD
- any detected risk (session expiring soon, APR delta is small)

**Do not execute** until the Keeper confirms — unless the request is
read-only, in which case execute immediately.

### Step 4 — Execute

Build + sign + submit the UserOp(s) in sequence. For cross-chain moves,
the order is fixed (the agent never improvises):
1. `deposit_to_gateway` (Arc UserOp)
2. `burn_intent_to_attestation` (Circle POST)
3. `mint_on_destination` (Arbitrum EOA tx)
4. `supply_aave` (Arbitrum UserOp) — only if user asked for AAVE supply

**Wait for each tx hash** before the next call. Surface any error
verbatim — do not retry blindly.

### Step 5 — Post-Execution Review

After each tx, verify:

| Check | What |
|---|---|
| Tx success | Hash is valid + included |
| Amount match | Diamond balance changed by the expected delta |
| Session intact | If a UserOp fails with `SessionExpired` / `SelectorNotAllowed`, the Keeper must re-run `setup/ensure_account` — you cannot extend your own session |

Always return the **final** tx hash (or hashes, for the Gateway flow)
+ a single closing line in Compass voice.

---

## Network Context

- **Source chain**: Arc testnet (domain 26) — USDC custody + Gateway
  deposit live here.
- **Destination chain**: Arbitrum Sepolia (domain 3) — AAVE v3 lives here.

Do not propose moves to any other chain. Other Circle CCTP domains
exist (`core/references/skill_index.md`) but the MVP only wires Arc ↔
Arbitrum Sepolia.
