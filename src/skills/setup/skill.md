---
name: compass-setup
description: One-time on-chain bootstrap — deploys per-chain Diamonds (CREATE2, same salt → same address) and registers the agent's session key. Idempotent.
metadata:
  pattern: tool-wrapper
  domain: account-lifecycle
---

# Setup Skill — Tool Wrapper

Before any onchain action, the Keeper must have:
1. A Diamond deployed on every chain Compass operates on (Arc + Base
   Sepolia).
2. A valid session key registered on each Diamond so the agent's
   UserOps validate.

This skill exposes the read + write tools that bring those two
invariants up. Both tools are **idempotent** — safe to re-call on every
cold start of the strategy agent.

---

## When to Load References

| When you want to... | Load this reference |
|---|---|
| Check whether a Keeper is already set up (read-only) | `references/account-status.md` |
| Deploy missing Diamonds + register the session (write) | `references/ensure-account.md` |

---

## Tools You Call Directly

| Tool | Purpose |
|---|---|
| `account_status` | Predict Arc + Arbitrum Diamond addresses, report `deployed: bool` per chain. Always safe. |
| `ensure_account` | Deploys whichever Diamond is missing, then registers the agent's session key (1-day expiry) on each. Owner (the Keeper's EOA) signs `registerSession`. |

---

## Standard Setup Flow

1. Call `account_status`. If both chains show `deployed: true`, the
   only thing that might be stale is the session key — but if a later
   UserOp succeeds, you know it's still valid.
2. If either chain shows `deployed: false`, or a previous UserOp
   failed with `SessionExpired` / `SelectorNotAllowed`, call
   `ensure_account`. It only writes what's missing.
3. On success, surface the Diamond addresses **once** to the Keeper:
   > *Compass account live on Arc and Arbitrum Sepolia: `0x…`. Session valid
   > for 24 hours.*

---

## Determinism Invariant

`CompassAccountFactory.createAccount(owner, salt=0)` uses CREATE2 with
the same factory bytecode + same deployer + same salt on every chain.
Result: **the Keeper's Diamond has the same address on Arc and Base
Sepolia** (assuming the factory itself was deployed to identical
addresses across chains, which is the deployer's responsibility).

If `account_status` ever reports different addresses across chains,
that's a contract deployment issue — surface it, do not proceed with
cross-chain flows.

---

## What This Skill Does NOT Do

- Does not transfer ownership, perform diamond cuts, or register new
  session keys for OTHER agents. Those are owner-only operations the
  Keeper performs from their own wallet.
- Does not extend an expired session. If `ensure_account` reports the
  session was re-registered, that's because the previous one had
  expired — the Keeper had to sign again.
