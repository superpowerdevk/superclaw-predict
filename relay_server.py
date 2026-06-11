import os, subprocess, tempfile
from fastapi import FastAPI, Request

app = FastAPI()
BIN = os.path.expanduser("~/superclaw-predict/pm-trade/target/release/relay-submit")

@app.post("/relay")
async def relay(req: Request):
    body = await req.body()
    with tempfile.NamedTemporaryFile("wb", suffix=".json", delete=False) as f:
        f.write(body); path = f.name
    try:
        out = subprocess.run([BIN, path], capture_output=True, text=True)
        return {"ok": out.returncode == 0,
                "result": out.stdout.strip(),
                "error": out.stderr.strip() or None}
    finally:
        os.unlink(path)
