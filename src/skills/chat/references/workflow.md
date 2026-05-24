# Chat agent workflows

Match the user's intent to one of these recipes. Never improvise a new
recipe that combines tools in unfamiliar ways without re-reading
`chat/tools_index.md`.

## New user wants to invest
```
read_market → read_position → translate intent → commit_policy → confirm in human language
```
The reply should mention the chosen venue (currently AAVE v3 on Arbitrum
Sepolia given current APR), the rebalance trigger (apr_delta in bps),
and one protection clause (peg floor or daily cap).

## User asks for status
```
read_position → read_audit → summarise in plain English
```
Pick out the most informative 1-3 audit events (latest action, most
recent circuit-break if any, otherwise the latest noop reason).
Don't dump the raw audit array.

## User wants to change strategy
```
read_policy → modify based on intent → commit_policy with the patched policy
```
Keep `compiled_from` set to the new intent text. The server bumps
`version` automatically — don't try to guess it.

## User says stop
```
pause_policy
```
One sentence: "Paused. Engine won't tick for you until you ask me to
resume."

## User says resume
```
resume_policy
```
One sentence: "Active again. Next cron tick re-evaluates."

## User asks how Compass works
No tool call needed for a brief explanation. Keep it under 4 sentences,
mention: (1) you compile intent into a Policy, (2) a deterministic
engine ticks every 15min and acts within Policy bounds, (3) funds stay
in the user's Diamond — agent only has a scoped session key, (4) they
can pause / change strategy any time.
