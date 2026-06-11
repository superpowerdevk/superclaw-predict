import os, time, hmac, hashlib, base64, httpx
from fastapi import FastAPI, Request

app = FastAPI()
RELAYER = "https://relayer-v2.polymarket.com/submit"

def _b64decode_any(s):
    s = s + "=" * (-len(s) % 4)
    try: return base64.b64decode(s)
    except Exception: return base64.urlsafe_b64decode(s.replace("-", "+").replace("_", "/"))

def _sign(method, path, body):
    ts = str(int(time.time()))
    mac = hmac.new(_b64decode_any(os.environ["BUILDER_SECRET"]),
                   (ts + method + path + body).encode(), hashlib.sha256).digest()
    sig = base64.b64encode(mac).decode().replace("+", "-").replace("/", "_")
    return ts, sig

@app.get("/")
def health():
    return {"ok": True, "service": "superclaw-relay"}

@app.post("/relay")
async def relay(req: Request):
    body = (await req.body()).decode()
    ts, sig = _sign("POST", "/submit", body)
    headers = {
        "POLY_BUILDER_API_KEY": os.environ["BUILDER_KEY"],
        "POLY_BUILDER_TIMESTAMP": ts,
        "POLY_BUILDER_PASSPHRASE": os.environ["BUILDER_PASSPHRASE"],
        "POLY_BUILDER_SIGNATURE": sig,
        "Content-Type": "application/json",
    }
    async with httpx.AsyncClient() as c:
        r = await c.post(RELAYER, content=body, headers=headers, timeout=30)
    return {"ok": r.status_code < 300, "status": r.status_code, "result": r.text}
