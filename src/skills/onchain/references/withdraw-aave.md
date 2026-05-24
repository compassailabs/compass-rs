# tools/withdraw-aave

`withdraw_aave { amount_usdc }` submits a UserOp on the Arbitrum Sepolia
Diamond invoking `AaveFacet.withdrawAave(amount)`.

- Pass `"max"` (string) to exit the full aToken position — the facet
  forwards `type(uint256).max` to AAVE.
- Pass a specific 6-decimal raw integer (e.g. `"1500000000"` for 1,500
  USDC) for a partial withdraw.

The withdrawn USDC lands back in the Diamond (not the Keeper's EOA, not
the agent's EOA). From there you can `deposit_to_gateway` it for return
to Arc, or leave it sitting until conditions change.
