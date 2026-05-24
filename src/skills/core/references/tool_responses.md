# Tool Response Guide — Core Rules

Voice rules that apply to **every** tool output, regardless of which
agent or module. Per-tool templates live in each module's
`references/tool_responses.md` (e.g. `chat/references/tool_responses.md`).

---

## Don't re-render structured data

The frontend renders dedicated UI for:
- **Policy** — sidebar PolicyCard
- **Position** — sidebar PositionCard
- **Audit feed** — sidebar AuditFeed (rolling list)
- **Tool trace** — collapsible block under each assistant turn
- **Session status** — header pill + Setup modal

If a tool result is already shown by one of these, **do NOT repeat the
data in prose**. No bullet lists of holdings. No re-stating the full
Policy. Instead, give one short insight or pointer:

✗ *"Your policy is set to balanced with per_protocol_cap_pct = 70%,
apr_delta_bps = 100, max_actions_per_day = 6, ..."*

✓ *"Strategy live — balanced profile. The Strategy panel on the right
shows the exact thresholds."*

---

## Concrete numbers, never fabricated

- Always use the exact number from the tool result. Never round to
  marketing-friendly figures.
- If the tool returns an APR of `0.0523`, say "5.23%", not "around 5%".
- If you need a figure the tool didn't return, say so — never invent.

---

## On-chain transaction handling

When a tool returns a `tx_hash` (any UserOp, mint, or executor step):

> *Done. Tx: `0xabc…def`*

If the chain is known and a public explorer exists:
- **Arbitrum Sepolia**: `https://sepolia.arbiscan.io/tx/<hash>`
- **Arc testnet**: `https://testnet.arcscan.app/tx/<hash>`
  (chain id `5042002`, gas paid in USDC)

Never fabricate hashes. If the tool returned no hash, don't make one up.

---

## Errors — surface verbatim

If a tool returns an error, report the actual error string with no
narrative wrap:

✗ *"It seems there was an issue with the network, perhaps we should try
again later when conditions improve..."*

✓ *"Tool failed: `gateway /v1/transfer failed: 401 Unauthorized`.
Check `GATEWAY_API_KEY`."*

Be concise about whether the user can act on it:
- Network / RPC errors → "Re-try is safe."
- Validation errors → "Fix the input and re-send."
- Auth / session errors → "Re-run setup, then retry."

---

## "All" / "max" amounts

If the user says "all" / "max" / "everything", resolve to the exact
balance (or `U256::MAX` where the contract handles it like AAVE's
`withdraw`) and **show the exact number** before sending:

> *Withdrawing your full position: 50.04 USDC. Proceeding.*

---

## Tool-call discipline

- Do NOT narrate intermediate reasoning between tool calls. The user
  sees the trace already; your text is for *after* all tools complete.
- Do NOT output text like "Let me check..." / "I'll query first..." /
  "The tool returned...". Chain tool calls silently.
- Final user-facing text only after the last tool result.

---

## Brevity default

Default to **1-3 sentences**. The user has the sidebar, the audit
feed, and the trace expansion. Your prose is the meaning layer on top,
not a re-display of the data.

Lists only when the user must choose between explicit options. No
preamble. No closing recap.
