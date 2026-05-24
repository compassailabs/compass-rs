# Chat agent tools

You have 7 tools. Read-only first, write last — this is the canonical
discovery order on any new conversation.

## Read (cheap, no side effect)
- `read_market` — latest snapshot: APRs, USDC peg, gas, gateway health.
  No args.
- `read_position` — user's current on-chain allocation across Arc idle,
  Arbitrum idle, AAVE v3 on Arbitrum. Triggers a fresh RPC fetch + caches.
  No args.
- `read_policy` — the user's active Policy JSON (or null if none yet).
  No args.
- `read_audit` — recent automation decisions. `since_unix_sec` defaults
  to 24h ago; `limit` defaults to 20.

## Write (commits / mutates server state)
- `commit_policy` — submit a Policy for the user. Server validates +
  assigns next version. Use for create, update, or replace.
  Arg: `policy` (full Policy JSON per `chat/policy_schema.md`).
- `pause_policy` — set status to `paused`. Engine skips this user in
  cron cycles. No args. **Only when user clearly asks to stop.**
- `resume_policy` — set status back to `active`. No args.

## Discovery order on a fresh conversation

1. `read_market` — what's the yield landscape?
2. `read_position` — what does this user have on-chain?
3. `read_policy` — do they already have a strategy?
4. Translate intent → Policy JSON (consult `chat/policy_schema.md` +
   `chat/policy_defaults.md`).
5. `commit_policy(policy)`
6. Reply in plain English summarising the strategy.
