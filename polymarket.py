#!/usr/bin/env python3
"""SuperClaw Predict — keyless Polymarket odds fetcher.

Reads live market-implied probabilities from Polymarket's public Gamma API
(no key, no wallet). Provides the DATA half of prediction; the agent adds the
forecast/edge on top (see SKILL.md).

Usage:
    python3 polymarket.py events [--tag=crypto] [--limit=8]   # top active events by 24h volume
    python3 polymarket.py search "<query>"                    # search markets by keyword
    python3 polymarket.py market <slug>                       # one market's detail
"""

from __future__ import annotations

import json
import sys
import urllib.parse
import urllib.request

GAMMA = "https://gamma-api.polymarket.com"


def _get(path: str) -> object:
    req = urllib.request.Request(GAMMA + path, headers={"User-Agent": "superclaw-predict", "Accept": "application/json"})
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.loads(r.read().decode())


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


def cmd_events(tag: str | None, limit: int) -> None:
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
    out = [f"## 🔮 SUPERCLAW PREDICT — Live Polymarket Odds{sub}", "_Pick a number to forecast_", ""]
    slugs = []
    for i, m in enumerate(rows, 1):
        out.append(_market_block(m, i)); out.append("")
        slugs.append(f"{i}={m.get('slug','')}")
    out.append(f"[AGENT: present the list above; ask 'Which market should I forecast? (number)'. "
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
    out.append("\n[AGENT: these are the MARKET's odds. Research the event, form YOUR own probability, "
               "then present the forecast card (Market % / My read % / Edge / Confidence). One signal, not advice.]")
    print("\n".join(out))


def main() -> None:
    argv = sys.argv[1:]
    if not argv:
        cmd_events(None, 8); return
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
            cmd_events(tag, limit)
        elif cmd in ("search", "s"):
            cmd_search(" ".join(pos) if pos else "", limit)
        elif cmd in ("market", "m"):
            cmd_market(pos[0] if pos else "")
        else:
            print("Usage: events [--tag=] [--limit=] | search <query> | market <slug>")
    except Exception as e:  # noqa: BLE001
        print(f"ERROR: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
