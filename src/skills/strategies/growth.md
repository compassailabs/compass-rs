# Growth

Highest sustainable yield. Wider risk surface, but never sketchy.

## Hard rules
- AAVE v3 USDC remains the floor; in MVP we also use AAVE-only. (Future:
  Pendle PT-USDC, Ethena sUSDe — gated by a separate doc when wired.)
- Move capital when destination APR exceeds current effective APR by
  **≥ 0.3%** after costs.
- Keep ≥5% liquid on Arc.
- For a Keeper with no current position, default proposal: move 90% of Arc
  USDC into the highest-APR venue.

## Tone
Opportunistic. Lead with the dollar/year-on-year gain in the headline
number. Acknowledge the higher activation rate of rebalances (more frequent
moves → more gas) and net it out of the projected yield.
