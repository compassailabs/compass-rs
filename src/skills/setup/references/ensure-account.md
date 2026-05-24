# ensure_account Reference

Idempotent setup. Deploys whichever Diamond is missing and registers
the agent's session key on each. Safe to re-call — only writes what's
genuinely absent.

---

## Tool

```
ensure_account() → JSON summary
```

No args. Uses `ctx.user` as Diamond owner, `ctx.state.agent_address`
as session-key subject.

---

## What It Does

For each chain (Arc + Arbitrum Sepolia):

1. **Predict address** via `factory.getAccountAddress(owner, salt=0)`.
2. **If not deployed**, call `factory.createAccount(owner, salt=0,
   initArgs)`. The relayer (agent's EOA) pays gas; ownership transfers
   to `owner` inside the factory.
3. **Check session validity** via `SecurityFacet.isSessionValid` for
   the chain's expected selectors.
4. **If invalid / missing**, the owner (`user_signer`) calls
   `SecurityFacet.registerSession(agent, expires_at, selectors)`.
   Expiry is **24 hours** from now.

Selectors per chain (pulled from the `sol!` bindings — guaranteed in
sync with the Solidity signatures):

- **Arc**: `depositToGateway`, `withdrawFromGateway`
- **Arbitrum Sepolia**: `supplyAave`, `withdrawAave`

---

## Returns

```jsonc
{
  "user":  "0x...",
  "agent": "0x...",
  "arc_diamond":  "0x...",
  "arb_diamond": "0x...",
  "salt": 0,
  "deploy":  { "arc_tx": "0x..." | null, "arb_tx": "0x..." | null },
  "session": { "arc_tx": "0x..." | null, "arb_tx": "0x..." | null, "expires_at": <unix-secs> }
}
```

`null` tx hashes mean "already done, no write needed."

---

## Cost

Worst case (cold Keeper): 2 deployments + 2 `registerSession` txs = 4
chain writes, paid by the relayer EOA. Best case (everything fresh
already): 0 writes, just RPC reads.

---

## Failure Modes

| Error | Meaning | Resolution |
|---|---|---|
| `InitArgs.entryPoint is zero` | `.env` not configured for that chain | Set the chain's `_ENTRY_POINT` env var |
| `InitArgs.usdc is zero` | Likewise | Set the chain's `_USDC_ADDRESS` |
| `registerSession reverts: NotOwner` | The `user_signer` doesn't actually own the Diamond | Verify `COMPASS_USER_PK` is the same EOA that owns it |
| RPC timeout | Testnet flake | Re-call; idempotency means nothing was half-committed |

---

## Side-Effect Surface

Up to 4 on-chain writes per Keeper. Surface the resulting addresses +
session expiry to the Keeper **once**, then carry on. Do not re-call
this tool on every turn — `account_status` is the cheap read.
