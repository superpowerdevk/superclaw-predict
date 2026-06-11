#!/usr/bin/env python3
"""SuperClaw Predict — Polymarket trade execution (py-clob-client-v2).

REAL MONEY. Places live bets on Polymarket's CLOB (Polygon). Built with safety
gates: every buy/sell is DRY-RUN unless you pass --yes, every order carries a
slippage price cap, and the chain/real-money banner is always shown.

Credentials (never commit a private key; use env vars or a git-ignored config):
    POLYMARKET_PK        your wallet private key (0x...)         [required to trade]
    POLYMARKET_FUNDER    funder/wallet address that holds USDC   [defaults to signer]
    POLYMARKET_CHAIN     137 = Polygon mainnet, 80002 = Amoy testnet [default 137]
    POLYMARKET_HOST      CLOB host [default https://clob.polymarket.com]
    POLYMARKET_SIGTYPE   0 = EOA (own key, default) / 1 = email / 2 = browser proxy

Commands:
    address                                  show the signer address (sanity check)
    balance                                  USDC balance + allowance
    setup                                    one-time: approve USDC allowance
    price <token_id>                         current buy/sell price
    buy  <token_id> <usdc_amount> [--max-price=0.x] [--yes]
    sell <token_id> <shares>     [--min-price=0.x] [--yes]
"""

from __future__ import annotations

import os
import sys

HOST_DEFAULT = "https://clob.polymarket.com"


def _cfg():
    pk = os.environ.get("POLYMARKET_PK", "").strip()
    funder = os.environ.get("POLYMARKET_FUNDER", "").strip() or None
    chain = int(os.environ.get("POLYMARKET_CHAIN", "137"))
    host = os.environ.get("POLYMARKET_HOST", HOST_DEFAULT).strip()
    sig = int(os.environ.get("POLYMARKET_SIGTYPE", "0"))
    return pk, funder, chain, host, sig


def _banner(chain: int):
    if chain == 137:
        print("⚠️  LIVE — POLYGON MAINNET · REAL MONEY · orders are final\n")
    else:
        print(f"🧪 TEST MODE — chain {chain} (not mainnet)\n")


def _client(require_key: bool = True):
    pk, funder, chain, host, sig = _cfg()
    if require_key and not pk:
        print("ERROR: POLYMARKET_PK not set. Export your wallet private key (never commit it).", file=sys.stderr)
        sys.exit(2)
    from py_clob_client_v2 import ClobClient  # noqa: import here so read-only help works without the dep
    client = ClobClient(host, chain_id=chain, key=pk or None, signature_type=sig, funder=funder)
    if pk:
        client.set_api_creds(client.create_or_derive_api_key())
    return client, chain


def _flag(args, name, cast=str, default=None):
    for a in args:
        if a.startswith(f"--{name}="):
            try:
                return cast(a.split("=", 1)[1])
            except Exception:
                return default
    return default


def cmd_address():
    client, chain = _client()
    _banner(chain)
    print("Signer address:", client.get_address())


def cmd_balance():
    from py_clob_client_v2 import BalanceAllowanceParams, AssetType
    client, chain = _client()
    _banner(chain)
    try:
        ba = client.get_balance_allowance(BalanceAllowanceParams(asset_type=AssetType.COLLATERAL))
        print("USDC balance / allowance:", ba)
    except Exception as e:  # noqa: BLE001
        print("Could not read balance:", e)


def cmd_setup():
    from py_clob_client_v2 import BalanceAllowanceParams, AssetType
    client, chain = _client()
    _banner(chain)
    print("Approving USDC (COLLATERAL) allowance for the exchange — one-time, needs a little gas...")
    try:
        client.update_balance_allowance(BalanceAllowanceParams(asset_type=AssetType.COLLATERAL))
        print("✅ USDC allowance set. You can now buy. (Selling a token also approves that token on first sell.)")
    except Exception as e:  # noqa: BLE001
        print("Allowance setup failed:", e)
        sys.exit(1)


def cmd_price(token_id: str):
    client, chain = _client(require_key=False)
    try:
        buy = client.get_price(token_id, "BUY")
        sell = client.get_price(token_id, "SELL")
        print(f"token {token_id}\n  buy (ask): {buy}\n  sell (bid): {sell}")
    except Exception as e:  # noqa: BLE001
        print("Could not fetch price:", e)


def _exec_market(side_name: str, token_id: str, amount: float, limit_price: float, do_it: bool):
    from py_clob_client_v2 import (ClobClient, MarketOrderArgs, PartialCreateOrderOptions,
                                   OrderType, Side, AssetType, BalanceAllowanceParams)
    client, chain = _client()
    _banner(chain)
    side = Side.BUY if side_name == "buy" else Side.SELL
    # market context for correct tick size + neg-risk flag
    try:
        tick = client.get_tick_size(token_id)
        neg = client.get_neg_risk(token_id)
        ref = client.get_price(token_id, "BUY" if side == Side.BUY else "SELL")
    except Exception as e:  # noqa: BLE001
        print("Could not load market context:", e); sys.exit(1)

    unit = "USDC" if side == Side.BUY else "shares"
    print(f"PLAN: {side_name.upper()} {amount} {unit} of token {token_id}")
    print(f"  current {'ask' if side==Side.BUY else 'bid'}: {ref}  ·  your price cap: {limit_price}  ·  tick {tick} · negRisk {neg}")
    if not do_it:
        print("\n(dry run — nothing placed. Re-run with --yes to execute.)")
        return

    # selling an outcome token needs its CONDITIONAL allowance
    if side == Side.SELL:
        try:
            client.update_balance_allowance(BalanceAllowanceParams(asset_type=AssetType.CONDITIONAL, token_id=token_id))
        except Exception:
            pass

    args = MarketOrderArgs(token_id=token_id, amount=float(amount), side=side, price=float(limit_price))
    opts = PartialCreateOrderOptions(tick_size=tick, neg_risk=neg)
    resp = client.create_and_post_market_order(args, options=opts, order_type=OrderType.FOK)
    print("\nRESULT:", resp)


def main():
    a = sys.argv[1:]
    if not a:
        print(__doc__); return
    cmd, rest = a[0], a[1:]
    pos = [x for x in rest if not x.startswith("--")]
    do_it = "--yes" in rest
    try:
        if cmd == "address":
            cmd_address()
        elif cmd == "balance":
            cmd_balance()
        elif cmd == "setup":
            cmd_setup()
        elif cmd == "price":
            cmd_price(pos[0])
        elif cmd == "buy":
            token, amt = pos[0], float(pos[1])
            ref_cap = _flag(rest, "max-price", float)
            if ref_cap is None:
                # default slippage cap: current ask + 3¢, capped at 0.99
                c, _ = _client(require_key=False)
                try:
                    ref_cap = min(0.99, round(float(c.get_price(token, "BUY")) + 0.03, 2))
                except Exception:
                    ref_cap = 0.99
            _exec_market("buy", token, amt, ref_cap, do_it)
        elif cmd == "sell":
            token, shares = pos[0], float(pos[1])
            floor = _flag(rest, "min-price", float)
            if floor is None:
                c, _ = _client(require_key=False)
                try:
                    floor = max(0.01, round(float(c.get_price(token, "SELL")) - 0.03, 2))
                except Exception:
                    floor = 0.01
            _exec_market("sell", token, shares, floor, do_it)
        else:
            print(__doc__)
    except IndexError:
        print("Missing argument.\n" + __doc__); sys.exit(2)


if __name__ == "__main__":
    main()
