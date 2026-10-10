"""Screenshots of the dev shot harness (sample data) for the in-app FAQ.

Run with the dev server on :1420 (npm run dev):  python scripts/faq-shots.py [tr|en] [shot ...]

Headless Edge per language (separate profile so the language is not cached),
PNG -> WebP into aleph-edge/src/faq/shots/<lang>/<name>.webp.
"""
import os, subprocess, sys, tempfile
from PIL import Image

EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src", "faq", "shots")
TMP = os.path.join(tempfile.gettempdir(), "aleph-faq-shots"); os.makedirs(TMP, exist_ok=True)
SHOTS = sys.argv[2:] or [
    "dashboard", "bots", "signal-bots", "bot-detail", "settings", "bot-new",
    "dca", "dca-detail", "dca-settings", "dca-new", "grid", "grid-detail",
    "signals", "presets", "preset", "backtest", "backtest-report", "guide",
    "positions", "positions-exchange", "history", "risk",
    "settings-page", "accounts", "settings-devices", "settings-about", "account",
]
langs = [sys.argv[1]] if len(sys.argv) > 1 and sys.argv[1] in ("tr", "en") else ["tr", "en"]

for lang in langs:
    os.makedirs(os.path.join(OUT, lang), exist_ok=True)
    profile = tempfile.mkdtemp(prefix=f"edge-{lang}-")
    for name in SHOTS:
        png = os.path.join(TMP, f"{lang}-{name}.png")
        cmd = [EDGE, "--headless=new", "--disable-gpu", "--hide-scrollbars", f"--lang={lang}",
               f"--user-data-dir={profile}", "--window-size=1440,900", "--virtual-time-budget=9000",
               f"--screenshot={png}", f"http://localhost:1420/?shot={name}"]
        subprocess.run(cmd, capture_output=True, timeout=120)
        if not os.path.exists(png):
            print("FAILED", lang, name); continue
        im = Image.open(png).convert("RGB")
        dst = os.path.join(OUT, lang, f"{name}.webp")
        im.save(dst, "WEBP", quality=82, method=6)
        print(lang, name, os.path.getsize(dst) // 1024, "KB")
