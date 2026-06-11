---
name: superclaw-predict
description: Forecast Polymarket events AND place real bets through a managed, non-custodial wallet. The user only sees four dashboard screens and replies with numbers; all wallet/chain/contract machinery is hidden. Trigger when the user asks about odds, predictions, "what are the chances", or wants to bet on an event.
---

# SuperClaw Predict

## INSTALL (run once on skill install)
- `pip install --quiet web3 py-clob-client-v2` (runtime already has python3).
- Ensure binaries are executable: `chmod +x ./bin/order ./bin/sign-batch`.
- Config / env the skill reads: relay is hardcoded to `https://superclaw-predict.onrender.com` (already live), `POLYGON_RPC` (defaults to a public Polygon RPC), wallet stored at `~/.superclaw-predict/wallet.json` (generated on first use, never committed).
- The Rust binaries are self-contained (only need standard linux glibc) — no Rust toolchain required at runtime.


Forecasts Polymarket markets and places real bets. The user talks in plain language and taps numbers. NEVER expose: commands, token ids, contract addresses, tx hashes (beyond a short receipt), "deposit wallet", pUSD, sig types, or any chain detail.

## OUTPUT DISCIPLINE (CRITICAL — overrides default agent behavior)
- Your ENTIRE reply is the screen markdown — nothing else. NO preamble, NO "let me…", NO reasoning shown, NO narration, NO description of what you're doing, NO commentary before or after the screen.
- Run all commands SILENTLY in the background. Never print a command, a file path, or "running…".
- If the user says "run superclaw-predict", "start", "open it", or gives no specific market → render **SCREEN 0 (Markets board)**. Do not explain — just show it.
- Reformat any script output into the exact locked screen. NEVER paste raw script output. NEVER collapse the list into a paragraph — every market is its own bullet line.

## RUNTIME (run under the hood; never shown to the user)
- `PY` = python3 in the skill dir. Binaries are prebuilt at `./bin/order` and `./bin/sign-batch` (linux x86_64, shipped in the repo).
- Forecast data: `PY polymarket.py events|search|market <...>` (keyless Gamma API).
- Wallet + funding (client-side, user key): `PY polymarket_trade.py wallet|balance|swap|wrap|withdraw`.
- Orders (client-side, user key, posts to CLOB): `./bin/order buy|sell <token_id> <amount>`.
- Deploy/approvals/redeem (gasless, NON-CUSTODIAL): client `./bin/sign-batch > req.json` (user key, NO creds) → `POST https://superclaw-predict.onrender.com/relay` (relay sidecar has builder creds, NO key).
- Positions: GET `https://data-api.polymarket.com/positions?user=<deposit_wallet>`.
- Relay backend (already deployed): `https://superclaw-predict.onrender.com` — the skill POSTs signed batches to `https://superclaw-predict.onrender.com/relay`.

## FORECASTING (works everywhere, no wallet needed)
1. User asks about a topic → `polymarket.py search "<topic>"` (or `events` for a general board) → numbered list.
2. User picks one → `polymarket.py market <slug>` → read resolution criteria + the YES/NO token ids.
3. Web-research the event, form YOUR OWN probability, compute edge vs market → render SCREEN 1.

## ONE-TIME SETUP (only if no funded deposit wallet yet)
1. `wallet` → "Send USDC on Polygon (+ ~$1 POL for gas) to `<address>`."
2. Poll `balance` until USDC lands. If native USDC: `swap <amt> --yes`. Then `wrap <amt> --yes`.
3. Deploy + approvals (gasless, automatic, user does nothing): `sign-batch > req.json` → `POST /relay`.
4. "✅ Wallet ready — $<pusd> to bet with." Continue.

## THE LOOP (every choice is a number; always end on a screen)
- **A.** Market question → forecast → SCREEN 1.
- **B.** SCREEN 1: 1=Bet Yes / 2=Bet No → AMOUNT PICKER. 3=Skip → ask what's next.
- **C.** AMOUNT PICKER (render exactly):
  `How much on <SIDE>?` newline `**1** $5　**2** $10　**3** $25　**4** Custom`
  → set stake; shares = stake/ask; payout = shares; multiple = 1/ask → SCREEN 2.
- **D.** SCREEN 2: 1=Place bet → `order buy <token_id> <stake>` → success=SCREEN 3, fail=FAILURE. 2=Change→PICKER. 3=Cancel.
- **E.** SCREEN 3: 1=Positions→SCREEN 4. 2=Bet again→A. 3=Done.
- **F.** SCREEN 4: 1=Claim (if redeemable) → `sign-batch`(redeem) → `/relay`. Sell→ask which→`order sell`. Withdraw→ask amt+addr→`withdraw`. Done.

## OUTPUT SCREENS — render EXACTLY, fill <placeholders> only, never reword/reorder/add emoji. Numbered row always last. Gauge = `▓`×round(pct/10) + `░` to 10. Edge 🟢under/🔴over/⚪fair. Conf 🟢hi/🟡med/🔴lo. PnL 🟢≥0/🔴<0. Money $X.XX, odds whole %, shares whole.

### SCREEN 0 — Markets board (default, or when a category 6–10 is picked)
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
Board rules:
- Header word: default board = "Live"; after a category is picked = that category (e.g. "## 🔮 Crypto markets — tap a number to forecast").
- **1–5** = forecast the market currently shown on this board → SCREEN 1.
- **6–10** = reload THIS board filtered to that category, then keep showing it (the category row stays). Run `polymarket.py events --tag=<tag>`: 6→politics, 7→sports, 8→crypto, 9→economics, 10→geopolitics. 1–5 re-map to the new (filtered) markets.
- If a category returns no live markets, render the default board plus a single line: "No live <Category> markets right now." Never show an error.
- Always bulleted (lines never collapse). Drop dead longshots (Yes <3% or >97%), max 5 markets.

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

### SCREEN 2 — Confirm
```
## 🧾 Confirm bet
<question>

- Side **<YES|NO>**
- Stake **$<amount>**
- Price ~$<price> → **~<shares> shares**
- Wins **~$<payout>** (×<multiple>) if <side>
- Loses **$<amount>** if <other side>

**1** Place bet　**2** Change amount　**3** Cancel
```

### SCREEN 3 — Receipt
```
## ✅ Bet placed
$<amount> on **<side>** — <question>

- Filled **<shares> shares** @ $<price>
- Pays **$<payout>** if <side>
- Receipt `<short tx hash>`

**1** My positions　**2** Bet again　**3** Done
```

### SCREEN 4 — Positions
```
## 📊 Your predictions
- **<market>** · <side> · <shares> sh · $<value> · <🟢|🔴> <±pnl>
- **<market>** · <side> · <shares> sh · $<value> · <🟢|🔴> <±pnl>

Total **$<total>**
P&L <🟢|🔴> <±total>

✅ Ready to claim — **<market> (<outcome>) → $<amount>**

**1** Claim $<amount>　**2** Sell　**3** Withdraw　**4** Done
```
(Nothing redeemable → drop the claim line + action, renumber: `**1** Sell　**2** Withdraw　**3** Done`.)

### FAILURE — render instead of ANY raw error
```
## ⚠️ <one plain sentence, no jargon>

**1** <retry>　**2** <alternative>　**3** Cancel
```

## HARD RULES
- Confirm gate mandatory: never buy/sell/withdraw/redeem without the user tapping confirm.
- Translate every backend/chain error → FAILURE screen. Never show a 400/403/allowance string, token id, or contract.
- First bet must be ~$1 (validate the flow). This is real-money speculation; not financial advice; user bears all risk.
- Private key lives client-side only; builder creds live on the relay backend only; never log/echo/commit either.
