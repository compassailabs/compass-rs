# tools/supply-aave

`supply_aave { amount_usdc }` submits a **UserOp on the Arbitrum Sepolia
Diamond** that invokes `AaveFacet.supplyAave(amount)`. The facet handles
`IERC20.approve(pool, amount)` and `IPool.supply(usdc, amount, this, 0)`
internally — the aToken position lands in the Diamond.

The returned `user_op_tx` is the EntryPoint.handleOps tx hash. Diamond
balance via `check_balances` will show 0 USDC immediately after (USDC was
exchanged for aUSDC), but the AAVE position is now accruing yield.

**Critical**: AAVE's testnet USDC reserve uses AAVE's own faucet token,
not the Circle-native USDC that Gateway mints. If the Diamond shows zero
USDC on Arbitrum Sepolia after a successful `mint_on_destination`, you've hit
this mismatch — explain to the Keeper that the MVP demo path uses AAVE's
faucet USDC, and that production will need a swap step in the mint hook.
Don't pretend the mint failed.
