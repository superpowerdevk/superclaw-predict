---
name: superclaw-predict
description: Forecast Polymarket prediction-market events. Fetches live, keyless market-implied odds (sports, politics, crypto, business, tech, geopolitics), then researches the event, forms its OWN probability, and flags the EDGE versus the market. Use when the user asks "what are the odds of X", "predict <event>", "is the market right about Y", "should I bet on Z", or wants to browse prediction markets. Forecasts AND places real bets on Polymarket through a managed wallet — the user just deposits USDC; SuperClaw handles the wallet, allowance, signing, positions, redeem, and withdraw. Dry-run + confirmation gates on every order.
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

## Placing a bet (REAL MONEY — managed wallet)

SuperClaw trades through a **dedicated Polygon wallet it generates and manages for the user** (`polymarket_trade.py`, via `py-clob-client-v2`). The user never handles keys, never signs up on Polymarket, never sets env vars — they only **deposit USDC**. Non-custodial: the key lives locally in a git-ignored file; the user can `export` it or `withdraw` anytime.

One-time dependency: `pip install py-clob-client-v2 web3`.

### Wallet setup (first time — just deposit)
1. **Show the wallet:** `python3 polymarket_trade.py wallet` → prints the user's **deposit address**.
2. Tell the user to send, **on the Polygon network**, to that address: **USDC** (their betting balance) + **a little POL/MATIC** (one-time gas, ~$1). No Polymarket account needed.
3. **Confirm funds:** `python3 polymarket_trade.py balance` (shows USDC + POL).
4. **Approve allowance (one-time):** `python3 polymarket_trade.py setup`.

### Placing the bet (natural language — never show token IDs to the user)
1. The user expresses it plainly: *"bet $10 on YES"* / *"put $5 on No."* You map the side to its token ID **internally** from `python3 polymarket.py market <slug>` (Yes = first token, No = second). **Do NOT show raw token IDs to the user.**
2. **DRY RUN first** (no `--yes`): `python3 polymarket_trade.py buy <token_id> <usdc_amount>` — shows side, amount, price, slippage cap. Summarize it in plain language ("$10 on YES at ~$0.18 → ~55 shares; pays ~$55 if it resolves YES").
3. **Get explicit confirmation** of side + amount, then execute: `python3 polymarket_trade.py buy <token_id> <usdc_amount> --yes`. Report the fill in plain language.

### Managing (all natural language)
- **Positions:** "show my positions" → `python3 polymarket_trade.py positions`.
- **Sell / exit early:** `python3 polymarket_trade.py sell <token_id> <shares> --yes`.
- **Redeem winnings** (after a market resolves): "claim my winnings" → from `positions`, take the `conditionId` of a ✅ REDEEMABLE row → dry-run `python3 polymarket_trade.py redeem <condition_id>` → confirm → `--yes`.
- **Withdraw:** "withdraw $X to <address>" → `python3 polymarket_trade.py withdraw <address> <usdc> --yes`.
- **Take the wallet to polymarket.com:** "export my wallet" → `python3 polymarket_trade.py export` (reveals the key once, with a warning) so the user can import it into MetaMask and see full history on polymarket.com.

### Non-negotiable safety rules
- **Tiny first.** The user's FIRST bet must be **~$1** to validate the full flow before any larger size.
- **Always dry-run → confirm → `--yes`.** Never pass `--yes` without the user explicitly approving that exact side + amount.
- **🔒 The private key is managed locally and is NEVER shown except on an explicit `export`.** Never echo, log, or commit it. If a user pastes ANY private key into chat, tell them not to and that it is exposed. (The user does not need to handle keys at all in the normal flow.)
- This is speculative real-money betting: the user can lose the entire stake, markets can be illiquid or resolve unexpectedly. Not financial or betting advice — the user bears all risk.

## Notes

- `polymarket.py` uses Polymarket's public Gamma API — no key, no wallet, read-only. Generous rate limit (~1,000/hr); don't spam it.
- Price = probability (0.65 = 65% chance). Volume = total traded; liquidity = order-book depth. Higher volume/liquidity = more trustworthy odds.
