# account_status Reference

Read-only predicate over the Keeper's Diamond deployment on both
chains. Use this first on any new Keeper before deciding whether to
call `ensure_account`.

---

## Tool

```
account_status() → JSON
```

No args. Uses `ctx.user` as the owner address.

---

## Returns

```jsonc
{
  "user":  "0x...",                       // Keeper EOA
  "agent": "0x...",                       // session-key address
  "arc_diamond":  { "address": "0x...", "deployed": true|false },
  "arb_diamond": { "address": "0x...", "deployed": true|false }
}
```

The two `address` fields **should be identical** when the factory has
been deployed correctly across chains (same deployer + same bytecode +
salt = 0). If they differ, surface that as a deployment issue before
attempting any cross-chain flow.

---

## When to Call

- First turn of any new Keeper conversation.
- After a UserOp fails with `SessionExpired` / `SelectorNotAllowed` —
  the diamond is still deployed, but check `ensure_account` to refresh
  the session key.
- Whenever the Keeper asks "is my Compass account set up?"

---

## Cost

One factory `getAccountAddress` call per chain (cheap view) + one
`eth_getCode` per Diamond. No gas, no on-chain writes.
