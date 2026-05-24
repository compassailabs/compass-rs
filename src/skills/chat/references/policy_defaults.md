# Per-risk-profile defaults

When the user picks a risk label (conservative / balanced / growth) but
doesn't specify individual parameters, fill in from this table. Keep
`apr_lookback_minutes = 60`, `min_idle_minutes = 30`,
`estimated_hold_days = 7`, `max_move_pct_per_action = 100`, and the
default circuit_breakers (`usdc_peg_min = 0.98`, `utilization_max = 0.95`,
`tvl_drop_pct_1h = 30.0`, `protocol_blacklist_on_event = true`) unless
the user explicitly asks for something else.

| Risk         | per_protocol_cap_pct | apr_delta_bps | max_actions_per_day | min_net_profit_usd | max_gas_usd_per_action |
|--------------|---------------------|---------------|---------------------|--------------------|-----------------------|
| Conservative | 50                  | 20            | 3                   | 0                  | 3                     |
| Balanced     | 70                  | 10            | 6                   | 0                  | 5                     |
| Growth       | 100                 | 5             | 12                  | 0                  | 10                    |

> **Demo / testnet note:** Both `min_net_profit_usd` and `apr_delta_bps`
> are dialled WAY down from production values so the engine actually
> fires on the testnet's tiny AAVE APR (≈ 29 bps). Production would
> use ~150 / 100 / 50 bps triggers and >$0 min profit so dust moves
> can't accumulate gas waste. For the demo we want every commit to
> result in a visible on-chain action.

## Notes on the three rows

- **Conservative**: 20bps trigger (still 9bps headroom under AAVE's
  ~29bps so the engine moves). Hard cap of 3 actions/day, 50% per
  venue, $3 gas ceiling.
- **Balanced**: 10bps trigger, 6 actions/day, 70% concentration. The
  recommended default if user doesn't specify.
- **Growth**: 5bps trigger, 12 actions/day, full concentration allowed,
  $10 gas tolerance.

## Pre-commit sanity check (REQUIRED)

Before calling `commit_policy`, **always** verify that the
`apr_delta_bps` you've picked is **below** the current best APR's
basis points. The rule:

```
best_venue_apr_bps = round(best_venue_apr * 10_000)
if apr_delta_bps >= best_venue_apr_bps:
    => engine will Noop forever with apr_delta_below_threshold
```

Workflow:
1. Read `snapshot.venues[*].apr` (already in your `read_market` result).
   Pick the highest non-idle APR. Convert to bps: `apr * 10000`.
2. If the table value above exceeds that bps, **lower** `apr_delta_bps`
   to at most `(best_apr_bps - 5)` so the move clearly fires.
3. Tell the Keeper plainly: "AAVE is at 29 bps; I'm using a 10 bps
   trigger so the engine moves immediately."

Never commit a Policy whose trigger you know would Noop. The user
will see "nothing happened" and lose trust in the system.

## `created_at`

Use the current time in ISO-8601 UTC (e.g. `"2026-05-21T12:34:56Z"`).
The server will accept other ISO-8601 forms; this is the canonical one.
