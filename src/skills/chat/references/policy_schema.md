# Policy JSON schema

This is the exact shape you pass to `commit_policy.policy`. The server
validates against this and will reject anything that violates the rules
below.

```jsonc
{
  "version": 1,                                  // server overrides
  "user": "0x...",                              // server overrides with auth'd user
  "risk_label": "conservative" | "balanced" | "growth",
  "created_at": "<ISO-8601 UTC>",
  "compiled_from": "<the user's original intent text>",
  "status": "active",
  "protocols": {
    "whitelist": [
      { "chain": "arc",          "protocol": "idle"    },
      { "chain": "arbitrum_sepolia", "protocol": "idle"    },
      { "chain": "arbitrum_sepolia", "protocol": "aave_v3" }
    ],
    "per_protocol_cap_pct": 1-100
  },
  "chains": { "whitelist": ["arc", "arbitrum_sepolia"] },
  "triggers": {
    "apr_delta_bps":         >= 10,
    "apr_lookback_minutes":  > 0,
    "min_idle_minutes":      > 0
  },
  "caps": {
    "max_move_pct_per_action": 1-100,
    "max_actions_per_day":     > 0,
    "min_net_profit_usd":      >= 0
  },
  "gas": {
    "estimated_hold_days":     > 0,
    "max_gas_usd_per_action":  > 0
  },
  "circuit_breakers": {
    "usdc_peg_min":                0.98,
    "utilization_max":             0.95,
    "tvl_drop_pct_1h":             30.0,
    "protocol_blacklist_on_event": true
  },
  "notifications": null
}
```

## Validation rules — must satisfy

- `protocols.whitelist` non-empty; **always include BOTH idle venues**
  (`arc/idle` + `arbitrum_sepolia/idle`) so the engine has somewhere to
  drain to.
- Every venue's `chain` must appear in `chains.whitelist`.
- `per_protocol_cap_pct × whitelist.length ≥ 100` (otherwise capital
  can't be fully placed under the cap).
- `triggers.apr_delta_bps ≥ 10` (10 bps minimum — anything finer is
  noise).
- `caps.min_net_profit_usd ≥ 0`, finite.
- `gas.estimated_hold_days > 0`.
- `gas.max_gas_usd_per_action > 0`.
- `circuit_breakers.usdc_peg_min ∈ (0, 1]`.
- `circuit_breakers.utilization_max ∈ (0, 1]`.
- `circuit_breakers.tvl_drop_pct_1h ∈ (0, 100]`.

## Field semantics

- **`risk_label`** — display + skill-routing only. Real behaviour is
  governed by the numeric fields below.
- **`triggers.apr_delta_bps`** — the minimum APR difference between
  current best venue and a candidate before the evaluator considers a
  rebalance.
- **`triggers.apr_lookback_minutes`** — smoothing window. Use 60 unless
  the user wants something else.
- **`triggers.min_idle_minutes`** — cooldown after a venue receives
  capital before it can receive again (anti-oscillation).
- **`caps.min_net_profit_usd`** — expected profit (ΔAPR × principal ×
  hold_days / 365) minus gas + bridge cost must clear this number.
- **`gas.estimated_hold_days`** — assumed holding period; used only in
  the EV calc above.
- **`circuit_breakers.usdc_peg_min`** — if Chainlink shows USDC below
  this, the evaluator emits CircuitBreak and the engine drains to idle.
- **`circuit_breakers.utilization_max`** — pool utilisation ceiling on
  any venue the user currently holds.
- **`circuit_breakers.tvl_drop_pct_1h`** — sudden TVL drop on any
  currently-held venue triggers drain.
