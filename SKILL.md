---
name: superclaw-predict
description: Forecast Polymarket prediction markets — real odds plus SuperClaw's own independent read and edge — in chat, then place bets through the OKX OnchainOS Polymarket route. Trigger when the user asks about odds, predictions, "what are the chances", or wants to bet on an event.
---

# SuperClaw Predict

Two layers, clean separation:
1. **SuperClaw = the forecasting brain** (in chat): market discovery, an independent probability, the edge vs the market. This is the value-add and it works everywhere.
2. **Placement = delegated to the OKX OnchainOS `polymarket-plugin`** (sanctioned route): OKX owns funding/deposit, the order, the confirmation gates, risk limits, positions, and redeem. SuperClaw NEVER signs or submits anything itself.

## OUTPUT DISCIPLINE (critical)
- Your reply for a forecasting step is ONLY the screen markdown — no narration, no "let me…", no commands shown.
- For placement, you HAND OFF to the OKX plugin and let its flow run; you do not fabricate fills or place orders yourself.

## PREREQUISITES (one-time, user side)
- OKX OnchainOS installed: `npx skills add okx/onchainos-skills`
- Polymarket plugin installed: `npx skills add okx/plugin-store --skill polymarket-plugin`
- OKX API creds present server-side: `OKX_API_KEY`, `OKX_SECRET_KEY`, `OKX_PASSPHRASE`. NEVER echo these in output, logs, or screenshots.

## FORECASTING (SuperClaw's job)
1. User asks about a topic → `python3 polymarket.py search "<topic>"` (or `events` for the board) → numbered list.
2. User picks one → `python3 polymarket.py market <slug>` → resolution criteria + YES/NO odds.
3. Research the event, form YOUR OWN probability, compute the edge vs the market → render SCREEN 1.
4. Render the board / forecast card. These are markdown screens (below).

### SCREEN 0 — Markets board
```
## 🔮 <Category> markets — tap a number to forecast
- **1** <question> · Yes <x>%
- **2** <question> · Yes <x>%
- **3** <question> · Yes <x>%
- **4** <question> · Yes <x>%
- **5** <question> · Yes <x>%

Or browse a category:
**6** Politics　**7** Sports　**8** Crypto　**9** Economy　**10** World

Reply with a number.
```
(Default header "Live"; after a category pick, that category, e.g. "Crypto markets". 1–5 forecast the shown market; 6–10 reload filtered via `polymarket.py events --tag=<politics|sports|crypto|economics|geopolitics>`. Bulleted so lines never collapse, max 5, drop dead longshots.)

### SCREEN 1 — Forecast
```
## 🔮 <question>
<volume> traded · resolves <date>

- **Market** `<bar>` <X>%
- **My read** `<bar>` <Y>%
- **Edge** <🟢|🔴|⚪> <underpriced|overpriced|fair> by <Z> pts
- **Confidence** <🟢|🟡|🔴> <high|medium|low>

Why I'm <higher|lower>:
- <reason 1>
- <reason 2>
- <reason 3>

**1** Bet Yes　**2** Bet No　**3** Skip
```
(Gauge `<bar>` = `▓`×round(pct/10) + `░` to 10. Money $X.XX, odds whole %.)

## PLACEMENT — hand off to OKX (SuperClaw never places orders itself)
When the user picks a side (Bet Yes / Bet No) and an amount, DELEGATE to the OKX `polymarket-plugin` by emitting its trigger phrasing, e.g.:

> **buy polymarket shares — market: "<question or slug>", outcome: <YES|NO>, amount: $<usd>**

`okx-dapp-discovery` routes "place a bet on Polymarket …" to the plugin automatically. The OKX plugin then owns the whole write path:
- **Funding/deposit** (it handles USDC deposit on Polygon — 充值/deposit), so SuperClaw does NOT manage wallets, swaps, relays, or keys.
- **Preview** of the resolved trade (from/to, amount, slippage, price impact).
- **Mandatory typed live-mode confirmation** before any real write ("confirm live mode" / "确认开启实盘"). A casual "yes" does NOT satisfy it.
- **Risk limits** (max position, max trades, daily loss cap) — if a limit fires, it stops for a fresh typed confirmation.
- **Positions, redeem, cancel** — also via the plugin.

SuperClaw's role at this point is to pass the resolved intent and let the OKX plugin drive its preview + confirmation. Surface the plugin's preview/result to the user; do not wrap, fake, or bypass it.

## HARD RULES
- SuperClaw = forecasting + handoff ONLY. It never holds keys, signs, funds, or submits orders — the OKX plugin does all writes.
- NEVER attempt to bypass the OKX plugin's paper-mode default, typed live-mode confirmation, preview, or risk limits. If a gate fails, surface why; do not "try anyway."
- Forecasts are SuperClaw's own analysis, not guarantees. Real money; not financial advice; the user bears all risk.
- OKX credentials stay server-side; never print `OKX_*` in any output.
- Eligibility/region/compliance for placement is governed by OKX's route. SuperClaw does not attempt to route around it.
