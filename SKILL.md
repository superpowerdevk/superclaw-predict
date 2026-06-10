---
name: superclaw-predict
description: Forecast Polymarket prediction-market events. Fetches live, keyless market-implied odds (sports, politics, crypto, business, tech, geopolitics), then researches the event, forms its OWN probability, and flags the EDGE versus the market. Use when the user asks "what are the odds of X", "predict <event>", "is the market right about Y", "should I bet on Z", or wants to browse prediction markets. Forecast and analysis only — it does NOT place bets.
---

# SuperClaw Predict — Polymarket Event Forecasting

Turn Polymarket's live odds into a real read: the market's number, the agent's own number, and the **edge** between them. Data comes from `polymarket.py` (keyless Polymarket Gamma API); the forecast is the agent's job.

> The agent does the analysis. The user decides. Market odds are one signal — this is not financial or betting advice.

## Interaction rules (apply everywhere)
- Present every choice as a **numbered menu** (`1) … 2) …`) and accept the number (or the label).
- Never fabricate odds — they come from `polymarket.py`. Never fabricate facts — research via web search; if you can't verify something, say so and lower your confidence.
- Always show three things: **market odds · your read · the edge** — and a confidence level. Never claim certainty.

## FIRST ACTION (on invoke)

1. **Find the market(s).**
   - If the user named a topic/event/team → `python3 polymarket.py search "<their query>"`.
   - If they just want to browse → `python3 polymarket.py events` (optionally `--tag=crypto|politics|sports|business|tech`).
   - You may run `python3 polymarket.py events --limit=8` for a top list.
2. **Show the numbered list** the script returns (question · market odds · volume · resolves) and ask which one to forecast: "Which market should I forecast? Reply with a number."

## Forecast a selected market

1. **Pull detail:** `python3 polymarket.py market <slug>` (the slug is the backtick value in the list). This gives the market's odds for each outcome + resolution criteria.
2. **Research the event:** use web search for the latest relevant facts (news, data, base rates, recent developments). Read the resolution criteria carefully — forecast the *exact* thing the market resolves on.
3. **Form YOUR OWN probability** with a short rationale (3–5 bullets of concrete evidence). Anchor on base rates where possible; adjust for current specifics.
4. **Compute the edge:** `edge = your_prob − market_prob`. Classify:
   - **Underpriced** — your probability is meaningfully higher than the market's (market too low).
   - **Overpriced** — your probability is meaningfully lower (market too high).
   - **Fair** — within ~3–5 points; no clear edge.
5. **Present the forecast** as a clean dashboard card, in this exact Markdown structure (render as Markdown — do NOT put it in a code block):

   ## 🔮 \<market question>
   **📊 Market:** Yes **\<X>%**  ·  💰 \<volume> traded  ·  📅 resolves \<date>

   **🧠 My read:** Yes **\<Y>%**
   - \<evidence bullet>
   - \<evidence bullet>
   - \<evidence bullet>

   **🎯 Edge:** \<🟢 Underpriced / 🔴 Overpriced / ⚪ Fair> by **\<Z> pts** _(market too low / too high / fair)_
   **Confidence:** 🟢 High / 🟡 Medium / 🔴 Low
6. **Close** with: market odds are one signal and this is an estimate, not advice; events can resolve unexpectedly; the user decides. Offer (numbered): "1) Forecast another  2) See more markets  3) Done."

## Hard rules

- **Forecast only — this skill does NOT place bets.** If the user wants to actually bet, tell them execution isn't wired into this skill yet (Polymarket trading runs on its own Polygon CLOB) and stop there.
- Read the **resolution criteria** before forecasting — many markets resolve on a technicality; forecast what actually resolves the market, not the loose headline.
- Calibrate honestly: if the evidence is thin, say Low confidence and keep your probability near the market's. Don't manufacture a big edge to seem useful.
- Markets older than their `resolves` date or with tiny volume are unreliable — flag thin/low-liquidity markets.
- Not financial or betting advice. The user bears all risk.

## Notes

- `polymarket.py` uses Polymarket's public Gamma API — no key, no wallet, read-only. Generous rate limit (~1,000/hr); don't spam it.
- Price = probability (0.65 = 65% chance). Volume = total traded; liquidity = order-book depth. Higher volume/liquidity = more trustworthy odds.
