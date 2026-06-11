#!/usr/bin/env python3
"""SuperClaw Predict — managed-wallet Polymarket trading (real money, Polygon).

Model: SuperClaw GENERATES and manages a dedicated Polygon wallet for the user.
The user's ONLY step is to deposit USDC (Polygon) + a little POL (gas) to the
address this prints. No Polymarket signup, no key export, no env vars. The
generated key is stored locally (git-ignored) and never shown unless the user
explicitly runs `export`. Everything else — allowance, signing, orders,
positions, redeem, withdraw — is automated here.

Safety: buy/sell/withdraw/redeem are DRY-RUN unless you pass --yes. Orders carry
a slippage price cap. Uses the verified py-clob-client-v2 API + web3 for on-chain.

Commands:
  wallet                                  create (if needed) + show your deposit address
  balance                                 USDC + POL balance
  wrap   <usdc_amount>  [--yes]            convert USDC -> pUSD (the tradeable collateral)
  setup                                   one-time: approve pUSD allowance (needs a little POL gas)
  price  <token_id>                       current buy/sell price
  buy    <token_id> <usdc>  [--max-price=] [--yes]
  sell   <token_id> <shares>[--min-price=] [--yes]
  positions                               your open positions (Polymarket Data API)
  redeem <condition_id> [--neg-risk] [--yes]   claim winnings from a RESOLVED market
  withdraw <to_address> <usdc> [--yes]    send USDC out to any wallet
  export                                  reveal your private key (to import into MetaMask/polymarket.com)
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

HOST = os.environ.get("POLYMARKET_HOST", "https://clob.polymarket.com")
CHAIN = int(os.environ.get("POLYMARKET_CHAIN", "137"))
RPCS = ([os.environ["POLYGON_RPC"]] if os.environ.get("POLYGON_RPC") else []) + [
    "https://polygon-bor-rpc.publicnode.com",
    "https://polygon.llamarpc.com",
    "https://1rpc.io/matic",
    "https://polygon-rpc.com",
]
WALLET_PATH = Path(os.environ.get("SUPERCLAW_WALLET", str(Path.home() / ".superclaw-predict" / "wallet.json")))

# Polygon contracts (verified from Polymarket docs)
USDCE = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"  # bridged USDC.e
USDC_NATIVE = "0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359"  # native Polygon USDC
PUSD = "0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB"   # Polymarket USD — the V2 collateral token
ONRAMP = "0x93070a847efEf7F70739046A929D47a521F5B8ee"  # CollateralOnramp: wrap(asset,to,amount) USDC->pUSD
ROUTER = "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45"  # Uniswap V3 SwapRouter02 (Polygon)
CTF = "0x4D97DCd97eC945f40cF65F87097ACe5EA0476045"     # Conditional Tokens Framework
DATA_API = "https://data-api.polymarket.com"

ERC20_ABI = json.loads('[{"constant":true,"inputs":[{"name":"a","type":"address"}],"name":"balanceOf","outputs":[{"name":"","type":"uint256"}],"type":"function"},{"constant":false,"inputs":[{"name":"to","type":"address"},{"name":"v","type":"uint256"}],"name":"transfer","outputs":[{"name":"","type":"bool"}],"type":"function"},{"constant":false,"inputs":[{"name":"s","type":"address"},{"name":"v","type":"uint256"}],"name":"approve","outputs":[{"name":"","type":"bool"}],"type":"function"}]')
ONRAMP_ABI = json.loads('[{"inputs":[{"name":"_asset","type":"address"},{"name":"_to","type":"address"},{"name":"_amount","type":"uint256"}],"name":"wrap","outputs":[],"stateMutability":"nonpayable","type":"function"}]')
ROUTER_ABI = json.loads('[{"inputs":[{"components":[{"name":"tokenIn","type":"address"},{"name":"tokenOut","type":"address"},{"name":"fee","type":"uint24"},{"name":"recipient","type":"address"},{"name":"amountIn","type":"uint256"},{"name":"amountOutMinimum","type":"uint256"},{"name":"sqrtPriceLimitX96","type":"uint160"}],"name":"params","type":"tuple"}],"name":"exactInputSingle","outputs":[{"name":"amountOut","type":"uint256"}],"stateMutability":"payable","type":"function"}]')
CTF_ABI = json.loads('[{"inputs":[{"name":"collateralToken","type":"address"},{"name":"parentCollectionId","type":"bytes32"},{"name":"conditionId","type":"bytes32"},{"name":"indexSets","type":"uint256[]"}],"name":"redeemPositions","outputs":[],"stateMutability":"nonpayable","type":"function"}]')


def _banner():
    print("⚠️  LIVE — POLYGON MAINNET · REAL MONEY · orders are final\n" if CHAIN == 137
          else f"🧪 TEST MODE — chain {CHAIN}\n")


def _wallet() -> dict:
    """Load the managed wallet, generating one on first use."""
    if WALLET_PATH.exists():
        return json.loads(WALLET_PATH.read_text())
    from eth_account import Account
    acct = Account.create()
    data = {"address": acct.address, "private_key": acct.key.hex()}
    WALLET_PATH.parent.mkdir(parents=True, exist_ok=True)
    WALLET_PATH.write_text(json.dumps(data))
    try:
        os.chmod(WALLET_PATH, 0o600)
    except Exception:
        pass
    return data


def _w3():
    """Connect to the first working public Polygon RPC (they get gated/rate-limited,
    so we try several). Override with POLYGON_RPC env var."""
    from web3 import Web3
    last = None
    for url in RPCS:
        try:
            w3 = Web3(Web3.HTTPProvider(url, request_kwargs={"timeout": 15}))
            if w3.is_connected():
                return w3
        except Exception as e:  # noqa: BLE001
            last = e
    raise RuntimeError(f"No working Polygon RPC reachable. Last error: {last}")


def _client():
    w = _wallet()
    from py_clob_client_v2 import ClobClient
    c = ClobClient(HOST, chain_id=CHAIN, key=w["private_key"], signature_type=0, funder=w["address"])
    c.set_api_creds(c.create_or_derive_api_key())
    return c, w["address"]


def _flag(args, name, cast=str, default=None):
    for a in args:
        if a.startswith(f"--{name}="):
            try:
                return cast(a.split("=", 1)[1])
            except Exception:
                return default
    return default


def cmd_wallet():
    w = _wallet()
    print("🔮 Your SuperClaw prediction wallet (Polygon)\n")
    print(f"  Deposit address:  {w['address']}\n")
    print("To start betting, send to that address on the **Polygon network**:")
    print("  • USDC  — your betting balance")
    print("  • a little POL (MATIC) — for one-time gas (≈ $1 is plenty)")
    print("\nThis wallet is yours. Run `export` anytime to take the key to MetaMask / polymarket.com,")
    print("or `withdraw` to send funds back out. Keep the key private — it controls these funds.")


def _erc20_bal(w3, token, addr):
    return w3.eth.contract(address=w3.to_checksum_address(token), abi=ERC20_ABI).functions.balanceOf(addr).call() / 1e6


def cmd_balance():
    w = _wallet()
    try:
        w3 = _w3()
        addr = w3.to_checksum_address(w["address"])
        pusd = _erc20_bal(w3, PUSD, addr)
        usdce = _erc20_bal(w3, USDCE, addr)
        usdcn = _erc20_bal(w3, USDC_NATIVE, addr)
        pol = w3.eth.get_balance(addr) / 1e18
        _banner()
        print(f"Address: {w['address']}")
        print(f"  pUSD (tradeable): ${pusd:,.2f}   <- this is what you bet with")
        print(f"  USDC.e:           ${usdce:,.2f}")
        print(f"  USDC (native):    ${usdcn:,.2f}")
        print(f"  POL (gas):        {pol:.4f}{'  ⚠️ low — add a little POL' if pol < 0.05 else ''}")
        if pusd < 1 and (usdce >= 1 or usdcn >= 1):
            src = "USDC.e" if usdce >= 1 else "native USDC"
            print(f"  ➜ You have {src} but no pUSD. Run `wrap <amount>` to convert it to pUSD before betting.")
        elif pusd < 1:
            print("  ⚠️ No pUSD/USDC yet — deposit USDC (Polygon) to this address to start.")
    except Exception as e:  # noqa: BLE001
        print(f"Could not read balance: {e}\nIs web3 installed? (pip install web3)")


def cmd_swap(amount, do_it, fee=100):
    """Same-chain swap native Polygon USDC -> USDC.e via Uniswap V3."""
    from eth_account import Account
    w3 = _w3(); w = _wallet(); acct = Account.from_key(w["private_key"])
    addr = w3.to_checksum_address(w["address"])
    _banner()
    have = _erc20_bal(w3, USDC_NATIVE, addr)
    print(f"PLAN: swap {amount} native USDC -> USDC.e via Uniswap V3 (fee {fee/10000}%)")
    if have < float(amount):
        print(f"  Not enough native USDC (have ${have:.2f})."); return
    if not do_it:
        print("\n(dry run — add --yes to swap. Needs POL gas.)"); return
    amt = int(round(float(amount) * 1e6)); min_out = int(amt * 0.99)
    try:
        tok = w3.eth.contract(address=w3.to_checksum_address(USDC_NATIVE), abi=ERC20_ABI)
        router = w3.eth.contract(address=w3.to_checksum_address(ROUTER), abi=ROUTER_ABI)
        n = w3.eth.get_transaction_count(addr)
        fd = {"maxFeePerGas": w3.eth.gas_price * 2, "maxPriorityFeePerGas": w3.to_wei(30, "gwei")}
        ap = tok.functions.approve(w3.to_checksum_address(ROUTER), amt).build_transaction({"from": addr, "nonce": n, "gas": 90000, **fd})
        h1 = w3.eth.send_raw_transaction(acct.sign_transaction(ap).raw_transaction)
        print("approve tx:", h1.hex(), "(waiting...)"); w3.eth.wait_for_transaction_receipt(h1, timeout=180)
        params = (w3.to_checksum_address(USDC_NATIVE), w3.to_checksum_address(USDCE), int(fee), addr, amt, min_out, 0)
        sw = router.functions.exactInputSingle(params).build_transaction({"from": addr, "nonce": n + 1, "gas": 300000, **fd})
        h2 = w3.eth.send_raw_transaction(acct.sign_transaction(sw).raw_transaction)
        print("swap tx:", h2.hex())
        print("Give it ~15s, then `balance` (expect USDC.e), then `wrap`.")
        print("If it reverted, retry: `swap %s --fee=500 --yes` (or --fee=3000)" % amount)
    except Exception as e:
        print("Swap failed:", e)


def cmd_wrap(amount, do_it):
    """Convert USDC.e (or native USDC) into pUSD via the CollateralOnramp."""
    from eth_account import Account
    w3 = _w3(); w = _wallet(); acct = Account.from_key(w["private_key"])
    addr = w3.to_checksum_address(w["address"])
    _banner()
    usdce = _erc20_bal(w3, USDCE, addr); usdcn = _erc20_bal(w3, USDC_NATIVE, addr)
    asset = USDCE if usdce >= float(amount) else (USDC_NATIVE if usdcn >= float(amount) else None)
    if asset is None:
        print(f"Not enough USDC to wrap {amount}. USDC.e ${usdce:.2f}, native ${usdcn:.2f}."); return
    print(f"PLAN: wrap {amount} {'USDC.e' if asset==USDCE else 'native USDC'} -> pUSD via onramp")
    if asset == USDC_NATIVE:
        print("  NOTE: onramp is documented for USDC.e; if native wrap reverts, swap native->USDC.e first.")
    if not do_it:
        print("\n(dry run — add --yes to wrap. Needs POL for gas: an approve tx + a wrap tx.)"); return
    amt = int(round(float(amount) * 1e6))
    try:
        tok = w3.eth.contract(address=w3.to_checksum_address(asset), abi=ERC20_ABI)
        onramp = w3.eth.contract(address=w3.to_checksum_address(ONRAMP), abi=ONRAMP_ABI)
        n = w3.eth.get_transaction_count(addr)
        fee = {"maxFeePerGas": w3.eth.gas_price * 2, "maxPriorityFeePerGas": w3.to_wei(30, "gwei")}
        ap = tok.functions.approve(w3.to_checksum_address(ONRAMP), amt).build_transaction(
            {"from": addr, "nonce": n, "gas": 90000, **fee})
        h1 = w3.eth.send_raw_transaction(acct.sign_transaction(ap).raw_transaction)
        print("approve tx:", h1.hex(), "(waiting...)")
        w3.eth.wait_for_transaction_receipt(h1, timeout=180)
        wr = onramp.functions.wrap(w3.to_checksum_address(asset), addr, amt).build_transaction(
            {"from": addr, "nonce": n + 1, "gas": 200000, **fee})
        h2 = w3.eth.send_raw_transaction(acct.sign_transaction(wr).raw_transaction)
        print("wrap tx:", h2.hex())
        print("✅ Wrapped. Run `balance` to see your pUSD, then `setup`, then bet.")
    except Exception as e:  # noqa: BLE001
        print("Wrap failed:", e)


def cmd_setup():
    from py_clob_client_v2 import BalanceAllowanceParams, AssetType
    c, addr = _client()
    _banner()
    print("Approving USDC allowance for the exchange (one-time, uses a little POL gas)...")
    try:
        c.update_balance_allowance(BalanceAllowanceParams(asset_type=AssetType.COLLATERAL))
        print("✅ Allowance set. Ready to bet.")
    except Exception as e:  # noqa: BLE001
        print(f"Allowance setup failed: {e}")
        sys.exit(1)


def cmd_price(token_id: str):
    from py_clob_client_v2 import ClobClient
    c = ClobClient(HOST, chain_id=CHAIN)
    try:
        print(f"token {token_id}\n  ask (buy): {c.get_price(token_id,'BUY')}\n  bid (sell): {c.get_price(token_id,'SELL')}")
    except Exception as e:  # noqa: BLE001
        print("Could not fetch price:", e)


def _market(side_name, token_id, amount, limit_price, do_it):
    from py_clob_client_v2 import (MarketOrderArgs, PartialCreateOrderOptions, OrderType, Side,
                                   AssetType, BalanceAllowanceParams)
    c, addr = _client()
    _banner()
    side = Side.BUY if side_name == "buy" else Side.SELL
    try:
        tick = c.get_tick_size(token_id); neg = c.get_neg_risk(token_id)
        ref = c.get_price(token_id, "BUY" if side == Side.BUY else "SELL")
    except Exception as e:  # noqa: BLE001
        print("Could not load market context:", e); sys.exit(1)
    unit = "USDC" if side == Side.BUY else "shares"
    print(f"PLAN: {side_name.upper()} {amount} {unit} of token {token_id}")
    print(f"  {'ask' if side==Side.BUY else 'bid'} {ref} · your price cap {limit_price} · tick {tick} · negRisk {neg}")
    if not do_it:
        print("\n(dry run — nothing placed. add --yes to execute.)"); return
    if side == Side.SELL:
        try:
            c.update_balance_allowance(BalanceAllowanceParams(asset_type=AssetType.CONDITIONAL, token_id=token_id))
        except Exception:
            pass
    args = MarketOrderArgs(token_id=token_id, amount=float(amount), side=side, price=float(limit_price))
    opts = PartialCreateOrderOptions(tick_size=tick, neg_risk=neg)
    print("\nRESULT:", c.create_and_post_market_order(args, options=opts, order_type=OrderType.FOK))


def cmd_positions():
    import urllib.request
    w = _wallet()
    try:
        url = f"{DATA_API}/positions?user={w['address']}&sizeThreshold=0.1"
        req = urllib.request.Request(url, headers={"User-Agent": "superclaw"})
        with urllib.request.urlopen(req, timeout=20) as r:
            pos = json.loads(r.read().decode())
    except Exception as e:  # noqa: BLE001
        print("Could not fetch positions:", e); return
    if not pos:
        print("No open positions."); return
    print("📌 Your Polymarket positions\n")
    for p in pos:
        title = p.get("title") or p.get("slug") or p.get("conditionId", "")[:14]
        out = p.get("outcome", "?"); sz = p.get("size", 0); cur = p.get("curPrice") or p.get("avgPrice")
        val = p.get("currentValue", 0); pnl = p.get("cashPnl", 0)
        redeemable = p.get("redeemable")
        tag = "  ✅ REDEEMABLE (winnings — run `redeem`)" if redeemable else ""
        print(f"- {title}\n    {out}: {sz} shares · now ~{cur} · value ${float(val):,.2f} · PnL ${float(pnl):,.2f}{tag}")
        if redeemable:
            print(f"    conditionId: {p.get('conditionId')}")


def cmd_redeem(condition_id, neg_risk, do_it):
    from eth_account import Account
    w3 = _w3(); w = _wallet(); acct = Account.from_key(w["private_key"])
    addr = w3.to_checksum_address(w["address"])
    _banner()
    print(f"PLAN: redeem winnings from condition {condition_id}" + (" (neg-risk)" if neg_risk else ""))
    if neg_risk:
        print("  NOTE: neg-risk markets redeem via the NegRiskAdapter — not supported in this build yet; "
              "redeem these on polymarket.com by importing your key (run `export`).")
        return
    if not do_it:
        print("\n(dry run — add --yes to submit the on-chain redeem.)"); return
    try:
        ctf = w3.eth.contract(address=w3.to_checksum_address(CTF), abi=CTF_ABI)
        cid = condition_id if condition_id.startswith("0x") else "0x" + condition_id
        parent = "0x" + "00" * 32
        tx = ctf.functions.redeemPositions(w3.to_checksum_address(PUSD), parent, cid, [1, 2]).build_transaction({
            "from": addr, "nonce": w3.eth.get_transaction_count(addr), "gas": 250000,
            "maxFeePerGas": w3.eth.gas_price * 2, "maxPriorityFeePerGas": w3.to_wei(30, "gwei"),
        })
        signed = acct.sign_transaction(tx)
        h = w3.eth.send_raw_transaction(signed.raw_transaction)
        print("Redeem tx:", h.hex())
    except Exception as e:  # noqa: BLE001
        print("Redeem failed:", e)


def cmd_withdraw(to, usdc_amt, do_it):
    from eth_account import Account
    w3 = _w3(); w = _wallet(); acct = Account.from_key(w["private_key"])
    addr = w3.to_checksum_address(w["address"]); to = w3.to_checksum_address(to)
    _banner()
    print(f"PLAN: withdraw {usdc_amt} USDC  →  {to}")
    if not do_it:
        print("\n(dry run — add --yes to send.)"); return
    try:
        usdc = w3.eth.contract(address=w3.to_checksum_address(PUSD), abi=ERC20_ABI)
        amt = int(round(float(usdc_amt) * 1e6))
        tx = usdc.functions.transfer(to, amt).build_transaction({
            "from": addr, "nonce": w3.eth.get_transaction_count(addr), "gas": 250000,
            "maxFeePerGas": w3.eth.gas_price * 2, "maxPriorityFeePerGas": w3.to_wei(30, "gwei"),
        })
        signed = acct.sign_transaction(tx)
        h = w3.eth.send_raw_transaction(signed.raw_transaction)
        print("Withdraw tx:", h.hex())
    except Exception as e:  # noqa: BLE001
        print("Withdraw failed:", e)


def cmd_export():
    w = _wallet()
    print("🔒 PRIVATE KEY — anyone with this controls your funds. Store it safely, never paste it back into chat.\n")
    print(f"  Address:     {w['address']}")
    print(f"  Private key: {w['private_key']}\n")
    print("Import into MetaMask, then connect at polymarket.com to see full history/positions on the website.")


def main():
    a = sys.argv[1:]
    if not a:
        print(__doc__); return
    cmd, rest = a[0], a[1:]
    pos = [x for x in rest if not x.startswith("--")]
    do_it = "--yes" in rest
    try:
        if cmd == "wallet": cmd_wallet()
        elif cmd == "balance": cmd_balance()
        elif cmd == "setup": cmd_setup()
        elif cmd == "price": cmd_price(pos[0])
        elif cmd == "buy":
            tok, amt = pos[0], float(pos[1])
            cap = _flag(rest, "max-price", float)
            if cap is None:
                from py_clob_client_v2 import ClobClient
                try: cap = min(0.99, round(float(ClobClient(HOST, chain_id=CHAIN).get_price(tok, "BUY")) + 0.03, 2))
                except Exception: cap = 0.99
            _market("buy", tok, amt, cap, do_it)
        elif cmd == "sell":
            tok, sh = pos[0], float(pos[1])
            floor = _flag(rest, "min-price", float)
            if floor is None:
                from py_clob_client_v2 import ClobClient
                try: floor = max(0.01, round(float(ClobClient(HOST, chain_id=CHAIN).get_price(tok, "SELL")) - 0.03, 2))
                except Exception: floor = 0.01
            _market("sell", tok, sh, floor, do_it)
        elif cmd == "swap": cmd_swap(float(pos[0]), do_it, int(_flag(rest, "fee", int, 100)))
        elif cmd == "wrap": cmd_wrap(float(pos[0]), do_it)
        elif cmd == "positions": cmd_positions()
        elif cmd == "redeem": cmd_redeem(pos[0], "--neg-risk" in rest, do_it)
        elif cmd == "withdraw": cmd_withdraw(pos[0], float(pos[1]), do_it)
        elif cmd == "export": cmd_export()
        else: print(__doc__)
    except IndexError:
        print("Missing argument.\n" + __doc__); sys.exit(2)


if __name__ == "__main__":
    main()
