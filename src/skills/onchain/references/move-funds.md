# tools/move-funds

Moving USDC from Arc → destination chain is a **3-tool sequence** in the
4337 + Diamond architecture:

1. **`deposit_to_gateway { amount_usdc }`** — UserOp on the Arc Diamond
   calling `GatewayFacet.depositToGateway(amount)`. The facet does
   `approve` + `IGatewayWallet.deposit` in one tx. Returns the UserOp tx
   hash. The Diamond's deposit is now sitting in the Keeper's Circle
   unified balance.

2. **`burn_intent_to_attestation { amount_usdc, destination_domain }`** —
   Off-chain. Builds an EIP-712 BurnIntent with `depositor = Arc Diamond`,
   `recipient = destination Diamond`, signs with the agent's session key,
   POSTs to Circle's `/v1/transfer`. Returns `{attestation, signature}`.

3. **`mint_on_destination { destination_domain, attestation, signature }`** —
   EOA relay tx to `GatewayMinter.gatewayMint(attestation, signature)` on
   the destination chain. The attestation embeds the recipient (destination
   Diamond), so the minted USDC lands there even though the agent's EOA
   relays the tx. Returns the mint tx hash.

Steps 2 and 3 must follow step 1 in order. Don't reorder, don't skip.
If step 1's UserOp fails (e.g. SessionExpired), call `ensure_account` and
retry from step 1.

After step 3, the destination Diamond's USDC balance reflects the mint;
call `supply_aave` to put it to work.
