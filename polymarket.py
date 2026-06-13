#!/usr/bin/env python3
"""SuperClaw Predict — keyless Polymarket odds fetcher.

Reads live market-implied probabilities from Polymarket's public Gamma API
(no key, no wallet). Provides the DATA half of prediction; the agent adds the
forecast/edge on top (see SKILL.md).

Usage:
    python3 polymarket.py events [--tag=crypto] [--limit=8] [--wallet=0x..]   # board + portfolio strip
    python3 polymarket.py worldcup [--limit=8] [--wallet=0x..]                # FIFA World Cup (mandatory)
    python3 polymarket.py search "<query>"                                    # search markets by keyword
    python3 polymarket.py market <slug>                                       # one market's detail
    python3 polymarket.py profile <0x-address>                               # profile URL (also saves wallet)
    python3 polymarket.py setwallet <0x-address>                             # save wallet + show dashboard
    python3 polymarket.py portfolio                                          # show saved wallet's positions/PnL

Wallet resolution order: --wallet arg > POLYMARKET_WALLET env > ~/.superclaw-predict/wallet.json
"""

from __future__ import annotations

import json
import os
import re
import sys
import urllib.parse
import urllib.request

GAMMA = "https://gamma-api.polymarket.com"
DATA = "https://data-api.polymarket.com"

_ADDR_RE = re.compile(r"^0x[a-fA-F0-9]{40}$")
WALLET_FILE = os.path.expanduser("~/.superclaw-predict/wallet.json")


def _get(path: str) -> object:
    req = urllib.request.Request(GAMMA + path, headers={"User-Agent": "superclaw-predict", "Accept": "application/json"})
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.loads(r.read().decode())


def _get_data(path: str) -> object:
    """Fetch from Polymarket's public (keyless) data API."""
    req = urllib.request.Request(DATA + path, headers={"User-Agent": "superclaw-predict", "Accept": "application/json"})
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.loads(r.read().decode())


def _num(v) -> float:
    try:
        return float(v)
    except Exception:
        return 0.0


def _load_wallet(explicit: str | None = None) -> str | None:
    """Resolve the user's wallet from, in order: explicit --wallet arg,
    POLYMARKET_WALLET env, then the saved dotfile. Returns a valid 0x addr or None."""
    for cand in (explicit, os.environ.get("POLYMARKET_WALLET")):
        if cand and _ADDR_RE.match(cand.strip()):
            return cand.strip()
    try:
        with open(WALLET_FILE) as f:
            saved = (json.load(f) or {}).get("wallet", "")
        if _ADDR_RE.match(saved):
            return saved
    except Exception:
        pass
    return None


def _save_wallet(addr: str) -> bool:
    """Best-effort persist. Never raises if the runtime is read-only/ephemeral."""
    try:
        os.makedirs(os.path.dirname(WALLET_FILE), exist_ok=True)
        with open(WALLET_FILE, "w") as f:
            json.dump({"wallet": addr}, f)
        return True
    except Exception:
        return False


def _parse_list(s):
    """outcomes / outcomePrices arrive as JSON strings like '["Yes","No"]'."""
    if isinstance(s, list):
        return s
    if isinstance(s, str) and s.strip():
        try:
            return json.loads(s)
        except Exception:
            return []
    return []


def _odds(market: dict):
    """Return (label, pct) for the leading outcome, e.g. ('Yes', 65)."""
    outs = _parse_list(market.get("outcomes"))
    prices = _parse_list(market.get("outcomePrices"))
    pairs = []
    for o, p in zip(outs, prices):
        try:
            pairs.append((o, float(p)))
        except Exception:
            continue
    if not pairs:
        return None, None
    # For Yes/No markets, always report the YES side; else the leading outcome.
    yes = [pp for pp in pairs if str(pp[0]).lower() == "yes"]
    label, pr = (yes[0] if yes else max(pairs, key=lambda x: x[1]))
    return label, round(pr * 100)


def _fmt_money(v) -> str:
    try:
        v = float(v)
    except Exception:
        return "?"
    if v >= 1e6:
        return f"${v/1e6:.1f}M"
    if v >= 1e3:
        return f"${v/1e3:.0f}K"
    return f"${v:.0f}"


def _date(s) -> str:
    if not s:
        return "?"
    return str(s)[:10]


def _market_block(m: dict, n: int) -> str:
    label, pct = _odds(m)
    odds = f"{label} **{pct}%**" if pct is not None else "odds n/a"
    vol = _fmt_money(m.get("volume") or m.get("volumeNum"))
    q = m.get("question") or m.get("groupItemTitle") or "(market)"
    return f"**{n} · {q}**\nMarket: {odds}  ·  💰 {vol}  ·  📅 {_date(m.get('endDate'))}"


def _vol(m: dict) -> float:
    try:
        return float(m.get("volume") or m.get("volumeNum") or 0)
    except Exception:
        return 0.0


def _is_live(m: dict) -> bool:
    """Keep genuinely uncertain markets; drop dead longshots / near-settled (no forecast value)."""
    if m.get("closed") or m.get("archived"):
        return False
    _, pct = _odds(m)
    return pct is not None and 3 <= pct <= 97


def _collect_markets(events: list, limit: int) -> list:
    """One representative (highest-volume, live) market per event, so a single
    multi-outcome mega-event (World Cup, election) can't flood the list with longshots."""
    picks = []
    for ev in events:
        cands = [m for m in (ev.get("markets", []) or []) if _is_live(m)]
        if not cands:
            continue
        cands.sort(key=_vol, reverse=True)
        picks.append(cands[0])
    picks.sort(key=_vol, reverse=True)
    return picks[:limit]


def cmd_events(tag: str | None, limit: int, wallet: str | None = None) -> None:
    qs = {"active": "true", "closed": "false", "order": "volume24hr", "ascending": "false", "limit": "60"}
    if tag:
        # resolve tag slug -> id is optional; Gamma also accepts tag slug via tag filter on events
        qs["tag_slug" if False else "tag"] = tag  # Gamma accepts ?tag=<slug> on /events
    events = _get("/events?" + urllib.parse.urlencode(qs))
    if not isinstance(events, list):
        print("No events returned.")
        return
    rows = _collect_markets(events, limit)
    sub = f" · {tag}" if tag else ""
    out = [f"## 🔮 SUPERCLAW PREDICT — 🔥 Trending Markets{sub}", "_⚡ Pick a number to forecast_", ""]
    slugs = []
    for i, m in enumerate(rows, 1):
        out.append(_market_block(m, i)); out.append("")
        slugs.append(f"{i}={m.get('slug','')}")
    out += _portfolio_footer(wallet)
    out.append(f"\n[AGENT: present the list above; ask 'Which market should I forecast? (number)'. "
               f"ALWAYS keep the 👛 portfolio dashboard block at the bottom. "
               f"Slugs for `market <slug>`: {' '.join(slugs)}]")
    print("\n".join(out))


def cmd_search(query: str, limit: int = 12) -> None:
    data = _get("/public-search?q=" + urllib.parse.quote(query) + "&limit=50")
    markets = []
    if isinstance(data, dict):
        for ev in data.get("events", []) or []:
            markets.extend(ev.get("markets", []) or [])
        markets.extend(data.get("markets", []) or [])
    elif isinstance(data, list):
        markets = data
    markets = [m for m in markets if not (m.get("closed") or m.get("archived"))][:limit]
    if not markets:
        print(f"## 🔮 SUPERCLAW PREDICT\n_No active markets found for_ **\"{query}\"** — try a different keyword.")
        return
    out = [f"## 🔮 SUPERCLAW PREDICT — Search: \"{query}\"", "_Pick a number to forecast_", ""]
    slugs = []
    for i, m in enumerate(markets, 1):
        out.append(_market_block(m, i)); out.append("")
        slugs.append(f"{i}={m.get('slug','')}")
    out.append(f"[AGENT: present the list above; ask which market (number) to forecast. "
               f"Slugs for `market <slug>`: {' '.join(slugs)}]")
    print("\n".join(out))


def cmd_worldcup(limit: int = 8, wallet: str | None = None) -> None:
    """Mandatory ⚽ FIFA World Cup category. Pulls live markets for Polymarket's
    `world-cup` league: tries the tag filter first (precise), then keyword search as a
    fallback, so it works regardless of how Gamma indexes the league. Footer links to
    the full Polymarket World Cup board."""
    BOARD = "https://polymarket.com/sports/world-cup/games"
    seen: dict = {}

    def _ingest(data):
        markets = []
        if isinstance(data, dict):
            for ev in data.get("events", []) or []:
                markets.extend(ev.get("markets", []) or [])
            markets.extend(data.get("markets", []) or [])
        elif isinstance(data, list):
            for item in data:
                if isinstance(item, dict) and item.get("markets"):
                    markets.extend(item.get("markets", []) or [])
                else:
                    markets.append(item)
        for m in markets:
            slug = m.get("slug", "")
            if slug and slug not in seen and _is_live(m):
                seen[slug] = m

    # 1) precise: events filtered by the world-cup tag
    try:
        qs = {"active": "true", "closed": "false", "order": "volume24hr",
              "ascending": "false", "limit": "60", "tag": "world-cup"}
        _ingest(_get("/events?" + urllib.parse.urlencode(qs)))
    except Exception:
        pass
    # 2) fallback / widen: keyword search across phrasings
    for q in ("FIFA World Cup", "World Cup 2026", "World Cup"):
        try:
            _ingest(_get("/public-search?q=" + urllib.parse.quote(q) + "&limit=50"))
        except Exception:
            continue

    rows = sorted(seen.values(), key=_vol, reverse=True)[:limit]
    if not rows:
        print("## ⚽ SUPERCLAW PREDICT — 🏆 FIFA World Cup\n"
              "_No live World Cup markets right now — check back closer to match days._\n"
              f"🔗 Full board: {BOARD}")
        return
    out = ["## ⚽ SUPERCLAW PREDICT — 🏆 FIFA World Cup", "_⚡ Pick a number to forecast_", ""]
    slugs = []
    for i, m in enumerate(rows, 1):
        out.append(_market_block(m, i)); out.append("")
        slugs.append(f"{i}={m.get('slug','')}")
    out.append(f"🔗 Full World Cup board: {BOARD}")
    out += _portfolio_footer(wallet)
    out.append(f"\n[AGENT: present the list above; ask which market (number) to forecast. "
               f"Keep the 'Full World Cup board' link AND the 👛 portfolio dashboard in the output. "
               f"Slugs for `market <slug>`: {' '.join(slugs)}]")
    print("\n".join(out))


def _dashboard(address: str) -> str:
    """Compact portfolio strip from Polymarket's public positions API (keyless)."""
    try:
        qs = {"user": address, "sortBy": "CURRENT", "sortDirection": "DESC",
              "sizeThreshold": "1", "limit": "50"}
        data = _get_data("/positions?" + urllib.parse.urlencode(qs))
    except Exception:
        return "👛 **Your portfolio** · _couldn't load positions right now — try again in a moment._"
    if not isinstance(data, list) or not data:
        return ("👛 **Your portfolio** · **no active positions yet** — pick a market above "
                "and place your first bet on Polymarket.")
    total_val = sum(_num(p.get("currentValue")) for p in data)
    total_pnl = sum(_num(p.get("cashPnl")) for p in data)
    total_cost = sum(_num(p.get("initialValue")) for p in data)
    pct = (total_pnl / total_cost * 100) if total_cost else 0.0
    dot = "🟢" if total_pnl >= 0 else "🔴"
    sign = "+" if total_pnl >= 0 else "−"
    lines = [f"👛 **Your portfolio** · {len(data)} open · 💰 {_fmt_money(total_val)} · "
             f"{dot} {sign}{_fmt_money(abs(total_pnl))} ({sign}{abs(pct):.0f}%)"]
    for p in data[:3]:
        pv = _num(p.get("currentValue")); pp = _num(p.get("cashPnl"))
        d = "🟢" if pp >= 0 else "🔴"; s = "+" if pp >= 0 else "−"
        title = (p.get("title") or "market").strip()[:40]
        outcome = p.get("outcome") or "?"
        lines.append(f"- {title} · {outcome} · {_fmt_money(pv)} · {d} {s}{_fmt_money(abs(pp))}")
    if len(data) > 3:
        lines.append(f"- _+{len(data) - 3} more_")
    return "\n".join(lines)


def _portfolio_footer(wallet: str | None) -> list:
    """Bottom-of-board dashboard, or a prompt to add a wallet if none is set."""
    if wallet:
        return ["", "─────────────", _dashboard(wallet)]
    return ["", "─────────────",
            "👤 _Drop your Polymarket wallet (public `0x…`) to see your live positions & PnL here._"]


def cmd_profile(address: str) -> None:
    """Build the user's Polymarket profile URL from their PUBLIC wallet address (0x…),
    so they can view their own positions / P&L. Public address only — NEVER a private key."""
    address = (address or "").strip()
    if not _ADDR_RE.match(address):
        print("## 👤 SUPERCLAW PREDICT — Your Polymarket profile\n"
              "_That doesn't look like a Polymarket wallet address._ It's the **public** address "
              "starting with `0x` (40 hex chars) — find it on Polymarket under **Settings → "
              "Wallet Address**. Never share a private key or seed phrase. 🔒")
        return
    url = f"https://polymarket.com/profile/{address}"
    _save_wallet(address)
    print("## 👤 SUPERCLAW PREDICT — Your Polymarket profile\n"
          "📊 View your live positions, P&L, and trade history here 👇\n"
          f"🔗 {url}")


def cmd_portfolio(wallet: str | None) -> None:
    """Standalone portfolio dashboard. Prompts for a wallet if none is set."""
    if not wallet:
        print("## 👛 SUPERCLAW PREDICT — Your portfolio\n"
              "_No wallet saved yet._ Send your **public** Polymarket address (`0x…`, from "
              "Settings → Wallet Address) and I'll track your positions & PnL. 🔒")
        return
    print("## 👛 SUPERCLAW PREDICT — Your portfolio\n" + _dashboard(wallet))


def cmd_setwallet(address: str) -> None:
    """Validate + persist the user's public wallet, then show their dashboard."""
    address = (address or "").strip()
    if not _ADDR_RE.match(address):
        print("## 👛 SUPERCLAW PREDICT — Your portfolio\n"
              "_That's not a valid Polymarket address._ Use the **public** `0x…` (40 hex) one "
              "from Settings → Wallet Address. Never a private key or seed phrase. 🔒")
        return
    saved = _save_wallet(address)
    note = ("✅ Wallet saved — your positions & PnL will show on every board from now on."
            if saved else
            "✅ Wallet noted for this session (couldn't write to disk — I'll carry it in context).")
    print(f"## 👛 SUPERCLAW PREDICT — {note}\n" + _dashboard(address))


def cmd_market(slug: str) -> None:
    data = _get("/markets?slug=" + urllib.parse.quote(slug))
    m = (data[0] if isinstance(data, list) and data else (data if isinstance(data, dict) else None))
    if not m:
        print(f"Market '{slug}' not found.")
        return
    outs = _parse_list(m.get("outcomes")); prices = _parse_list(m.get("outcomePrices"))
    out = [f"## 🔮 {m.get('question','(market)')}",
           f"📅 resolves {_date(m.get('endDate'))}  ·  💰 {_fmt_money(m.get('volume') or m.get('volumeNum'))} traded  ·  💧 {_fmt_money(m.get('liquidity') or m.get('liquidityNum'))} liquidity",
           "", "**📊 Market-implied odds**"]
    for o, p in zip(outs, prices):
        try:
            out.append(f"- {o} **{round(float(p)*100)}%**")
        except Exception:
            continue
    desc = (m.get("description") or "").strip()
    if desc:
        out += ["", "**Resolution criteria**", desc[:600]]
    # Polymarket event URL for the handoff (prefer the event slug, fall back to market slug)
    ev = m.get("events") or []
    event_slug = (ev[0].get("slug") if ev and isinstance(ev[0], dict) else None) or m.get("slug") or slug
    url = f"https://polymarket.com/event/{event_slug}"
    out.append(
        "\n[AGENT: Output ONLY the forecast card markdown — no narration, no preamble, no 'let me'. "
        "These are the MARKET's odds. Research the event, form YOUR OWN probability, then render SCREEN 1 "
        "EXACTLY per SKILL.md, in this order: title; traded/resolves line; bullets Market %, My read %, "
        "Edge, Confidence; then 'Why I am higher/lower:' with 3 bullets; then the play line "
        "the play line — ALWAYS a side, NEVER 'Skip': '**📈 The play: Bet YES|Bet NO|Lean YES|Lean NO** — for the reasons above, ...' pointing to the mispriced VALUE side (not the favorite); thin edge => 'Lean'; "
        "then EXACTLY this Polymarket link block (verbatim, with this URL):\n"
        "> 🎯 **Place your bets on Polymarket** — url below 👇\n"
        f"> 🔗 {url}\n"
        "One signal, not advice; SuperClaw never trades.]"
    )
    print("\n".join(out))


def main() -> None:
    argv = sys.argv[1:]
    explicit_wallet = None
    if argv:
        for a in argv[1:]:
            if a.startswith("--wallet="):
                explicit_wallet = a.split("=", 1)[1]
    wallet = _load_wallet(explicit_wallet)
    # If a fresh valid --wallet was passed, persist it so later runs auto-load.
    if explicit_wallet and _ADDR_RE.match((explicit_wallet or "").strip()):
        _save_wallet(explicit_wallet.strip())

    if not argv:
        cmd_events(None, 8, wallet); return
    cmd = argv[0]
    rest = argv[1:]
    tag = None; limit = 8
    for a in rest:
        if a.startswith("--tag="):
            tag = a.split("=", 1)[1]
        elif a.startswith("--limit="):
            try:
                limit = int(a.split("=", 1)[1])
            except Exception:
                pass
    pos = [a for a in rest if not a.startswith("--")]
    try:
        if cmd in ("events", "e"):
            cmd_events(tag, limit, wallet)
        elif cmd in ("worldcup", "wc"):
            cmd_worldcup(limit, wallet)
        elif cmd in ("search", "s"):
            cmd_search(" ".join(pos) if pos else "", limit)
        elif cmd in ("market", "m"):
            cmd_market(pos[0] if pos else "")
        elif cmd in ("profile", "me"):
            cmd_profile(pos[0] if pos else "")
        elif cmd in ("portfolio", "positions", "pnl"):
            cmd_portfolio(wallet)
        elif cmd in ("setwallet", "wallet"):
            cmd_setwallet(pos[0] if pos else (explicit_wallet or ""))
        else:
            print("Usage: events [--tag=] [--limit=] [--wallet=0x..] | worldcup | search <query> | "
                  "market <slug> | profile <0x-address> | setwallet <0x-address> | portfolio")
    except Exception as e:  # noqa: BLE001
        print(f"ERROR: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
