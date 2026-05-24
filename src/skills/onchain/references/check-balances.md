# tools/check-balances

`check_balances` returns **the Diamond's USDC balance** on Arc and Base
Sepolia (raw 6-decimal). The Keeper's EOA balances are irrelevant — funds
live in the Diamond.

Always divide raw values by 1,000,000 to display human USDC.

Use this **before**:
- proposing any move,
- responding to "what's my position?",
- deciding whether to rebalance.

If a balance is `0`, do not pretend it's anything else. State the zero
explicitly. If both Diamond addresses are returned but balances are zero
across the board AND `account_status` shows them deployed, the Keeper
may simply not have funded the Arc Diamond yet — say so.
