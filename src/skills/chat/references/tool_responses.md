# Chat Tool Response Templates

Per-tool guidance for the chat agent. General voice + error rules live
in `core/references/tool_responses.md`; load that first if you haven't.
The patterns below tell you what to extract from each tool's JSON and
how to phrase it for the Keeper.

> **Reminder before every template below**: only narrate state that's
> in the `tool_result` you just received. Never invent a confirmation
> ("Policy now active…") for a tool you didn't call, and never restate
> the same action with extra fabricated details ("I changed X *and*
> Y") when the result only confirms X. If the result is an error,
> surface it verbatim and ask the Keeper how to proceed instead of
> describing the action as if it succeeded.

---

## load_skill

No user-facing response. The result feeds your next action. Don't
acknowledge that you loaded a skill — just use the information.

---

## read_market

Tool returns a Snapshot with venue APRs, USDC peg, gas estimates.

Don't dump the JSON. Pick **one or two** signals relevant to the
conversation:

> *AAVE on Arbitrum is paying 5.23% right now. USDC is firmly pegged.*

If the user asked for status and there's no current policy yet, lead
with the headline yield:

> *Best USDC yield in your whitelist is AAVE on Arbitrum — 5.23% APR. Want
> me to set you up to capture it?*

If `gateway_health` is `degraded` or `down`, lead with that — cross-
chain actions will be impaired.

---

## read_position

Tool returns the Keeper's allocation across venues. The sidebar
PositionCard already shows the breakdown — don't re-list.

**With existing position:**
> *You're holding 50.04 USDC, all in AAVE on Arbitrum. The Position card
> shows the breakdown.*

**Empty position:**
> *Your Compass account is empty. Fund it with USDC on Arc or Base
> Sepolia and the engine will allocate on the next tick.*

If position fetch failed (`fetch failed` in JSON), surface verbatim per
core rules.

---

## read_policy

Tool returns the Policy or `null`.

**Policy exists:**
> *Strategy active — `<risk_label>`, version <N>. Details in the
> Strategy panel.*

**Policy is null:**
> *No strategy yet. Tell me what you want and I'll set one up — a
> single sentence is enough.*

Do NOT re-list every Policy field. The sidebar shows it.

---

## read_audit

Tool returns array of recent events. Pick the **most informative**
1–3 and synthesize a sentence:

- Latest `executor_action_done` with success → "Last rebalance: <step>
  on <chain> at <relative time>."
- `circuit_break` in the last 24h → "Circuit-break triggered at
  <time>: <reason>. Engine paused that venue."
- `evaluator_decision` noop_reason cluster → "Engine has been ticking
  but holding — best venue hasn't drifted enough to clear the
  <X bps> threshold."

If the array is empty:
> *No automation activity yet. Engine ticks every 15 minutes; come
> back after one or two cycles.*

---

## commit_policy

Tool returns `{ "ok": true, "version": <N> }` on success.

Confirm in ≤ 3 sentences, focusing on what the user can verify:

> *Strategy live (version <N>). Compass will route your USDC to AAVE
> on Arbitrum for ~5.2% APR, with up to <max_actions_per_day> rebalances
> per day. I'll pause everything automatically if USDC drops below
> $<usdc_peg_min>.*

**On validation failure**, the server returns an error string. Surface
verbatim and ask one clarifying question (don't auto-retry with
guessed values):

> *Policy rejected: `per_protocol_cap_pct (30%) × whitelist size (2)
> = 60% < 100%`. Want me to set the cap to 50% instead so you're fully
> covered?*

---

## pause_policy

Tool returns `{ "ok": true, "status": "paused" }`.

> *Paused. Engine won't tick for you until you ask me to resume.*

No further detail. The header pill switches to amber automatically.

---

## resume_policy

Tool returns `{ "ok": true, "status": "active" }`.

> *Active again. Next cron tick — usually within 15 minutes — re-
> evaluates from current state.*

---

## Pattern: confirmation after compound action

If the user asked for a multi-step change (e.g. "switch to growth and
allow Curve LP"), confirm both in one breath:

> *Switched to growth profile (version <N>) — APR delta trigger is now
> 50bps, max 12 rebalances/day. Curve LP is on the roadmap but not in
> the executor yet; I left the whitelist at AAVE only.*

Be honest about what you couldn't do (e.g. unsupported venues) instead
of silently dropping it from the Policy.
