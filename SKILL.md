---
name: superclaw-predict
description: Forecast Polymarket prediction-market events. Fetches live, keyless market-implied odds (sports, politics, crypto, business, tech, geopolitics), then researches the event, forms its OWN probability, and flags the EDGE versus the market. Use when the user asks "what are the odds of X", "predict <event>", "is the market right about Y", "should I bet on Z", or wants to browse prediction markets. Forecasts AND can place real bets on Polymarket (with dry-run confirmation + safety gates).
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
6. **Close** with: market odds are one signal and this is an estimate, not advice; events can resolve unexpectedly; the user decides. Then offer (numbered): "**1) Place a bet on this**  2) Forecast another  3) See more markets  4) Done." If they pick 1, go to **Placing a bet** below.

## Hard rules

- Betting is **real money** (see **Placing a bet** below). Always dry-run + confirm before executing; never bet without the user's explicit go-ahead on the exact side and amount.
- Read the **resolution criteria** before forecasting — many markets resolve on a technicality; forecast what actually resolves the market, not the loose headline.
- Calibrate honestly: if the evidence is thin, say Low confidence and keep your probability near the market's. Don't manufacture a big edge to seem useful.
- Markets older than their `resolves` date or with tiny volume are unreliable — flag thin/low-liquidity markets.
- Not financial or betting advice. The user bears all risk.

## Placing a bet (REAL MONEY — Polymarket CLOB)

Bets execute on Polymarket's on-chain order book via `polymarket_trade.py` (uses `py-clob-client-v2`). This is real money on Polygon. Hard safety flow — never skip:

### One-time setup (first bet only)
1. **Install the client:** `pip install py-clob-client-v2`
2. **Set credentials as environment variables** (NEVER commit or paste a private key in chat — if the user pastes one, tell them to stop and set it as an env var instead):
   - `POLYMARKET_PK` — wallet private key
   - `POLYMARKET_FUNDER` — wallet address holding the USDC (optional; defaults to signer)
   - `POLYMARKET_CHAIN` — `80002` for **Amoy testnet** (recommended first), `137` for **mainnet** (real money)
3. **Fund the wallet:** USDC on Polygon + a little MATIC for gas.
4. **Approve allowance (one-time):** `python3 polymarket_trade.py setup`
5. **Sanity check:** `python3 polymarket_trade.py balance`

### Placing the bet
1. Get the **token ID** for the side from `polymarket.py market <slug>` (Yes = first token, No = second).
2. Ask the user the **amount in USDC** and confirm the **side** (YES/NO).
3. **DRY RUN first** — run WITHOUT `--yes`:
   `python3 polymarket_trade.py buy <token_id> <usdc_amount>`
   This prints the plan (side, amount, current price, slippage cap, chain). Show it to the user.
4. **Get explicit confirmation** of side + amount + chain. Only then execute:
   `python3 polymarket_trade.py buy <token_id> <usdc_amount> --yes`
5. Report the fill. To exit a position later: `python3 polymarket_trade.py sell <token_id> <shares> --yes`.

### Non-negotiable safety rules
- **Testnet/tiny first.** The user's FIRST bet must be on **Amoy testnet** (`POLYMARKET_CHAIN=80002`) OR a **$1 mainnet bet on a throwaway wallet** to validate the full flow. Never let a first-ever action be a large mainnet bet.
- **Always dry-run, then confirm, then `--yes`.** Never pass `--yes` without the user explicitly approving that exact side + amount + chain.
- **Never echo, log, store, or commit the private key.** It lives only in the env var. Config files with keys must be git-ignored.
- State the **chain** (mainnet = real money, testnet = practice) in every confirmation.
- This is speculative real-money betting: the user can lose the entire stake, markets can be illiquid or resolve unexpectedly, and SL/TP-style exits are not automatic. Not financial or betting advice — the user bears all risk.

## Notes

- `polymarket.py` uses Polymarket's public Gamma API — no key, no wallet, read-only. Generous rate limit (~1,000/hr); don't spam it.
- Price = probability (0.65 = 65% chance). Volume = total traded; liquidity = order-book depth. Higher volume/liquidity = more trustworthy odds.
