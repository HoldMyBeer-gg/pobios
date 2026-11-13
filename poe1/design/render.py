#!/usr/bin/env python3
"""Render every screen of the PoB iPad design canvas to PNG via headless Chromium.

The canvas is the Claude Design project "Path of Building iOS port"
(PoB iPad.dc.html), fetched into design/canvas/. Run from anywhere:
    python3 design/render.py            # every screen in screens.json
    python3 design/render.py tree calcs # just those
Output lands in design/shots/. The sample build the canvas loads is the
one in ios/PoB/Resources; symlink or copy it to design/canvas/uploads/build.xml.

Each screen is a copy of the canvas with (a) React redirected to local files
through window.__resources and (b) the component's initial state overridden
by spreading __SCREEN_OVERRIDE as the LAST entries of the state literal, so
they win. Served over HTTP because the canvas uses dynamic import() + fetch().
"""
import json, os, re, subprocess, sys, time, http.server, socketserver, threading, shutil

ROOT = os.path.dirname(os.path.abspath(__file__))
CANVAS = os.path.join(ROOT, 'canvas')
OUT = os.path.join(ROOT, 'shots')
CHROME = '/Applications/Chromium.app/Contents/MacOS/Chromium'
PORT = 8765
W, H = 1194, 834  # canvas $preview: iPad Pro 11" landscape, points
SCALE = 2

SCREENS = json.load(open(os.path.join(ROOT, 'screens.json')))
if len(sys.argv) > 1: SCREENS = {k: v for k, v in SCREENS.items() if k in sys.argv[1:]}

src = open(os.path.join(CANVAS, 'PoB iPad.dc.html'), encoding='utf-8').read()

# 1. React from local vendor files, keyed by the exact CDN URLs support.js uses.
inject = ('<script>window.__resources={'
          '"https://unpkg.com/react@18.3.1/umd/react.production.min.js":"./vendor/react.js",'
          '"https://unpkg.com/react-dom@18.3.1/umd/react-dom.production.min.js":"./vendor/react-dom.js"};'
          'window.__SCREEN_OVERRIDE=%s;</script>\n<script src="./support.js"></script>')
assert '<script src="./support.js"></script>' in src

# 2. Spread the override at the end of the state literal.
anchor = '  };\n  async componentDidMount()'
assert anchor in src, 'state literal anchor not found'

def make(name, override):
    h = src.replace('<script src="./support.js"></script>', inject % json.dumps(override), 1)
    h = h.replace(anchor, '  , ...window.__SCREEN_OVERRIDE };\n  async componentDidMount()', 1)
    h = h.replace('eqSel: null, notes: pob.notes || this.state.notes', 'eqSel: (window.__SCREEN_OVERRIDE.eqSel ?? null), notes: pob.notes || this.state.notes', 1)
    p = os.path.join(CANVAS, f'__{name}.html')
    open(p, 'w', encoding='utf-8').write(h)
    return f'__{name}.html'

class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *a): pass
    def end_headers(self):
        self.send_header('Cache-Control', 'no-store'); super().end_headers()

def serve():
    os.chdir(CANVAS)
    with socketserver.TCPServer(('127.0.0.1', PORT), Quiet) as httpd:
        httpd.serve_forever()

threading.Thread(target=serve, daemon=True).start()
time.sleep(0.5)
os.makedirs(OUT, exist_ok=True)

results = []
for name, override in SCREENS.items():
    page = make(name, override)
    out = os.path.join(OUT, f'{name}.png')
    cmd = [CHROME, '--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run',
           '--disable-extensions', f'--window-size={W},{H}', f'--force-device-scale-factor={SCALE}',
           '--virtual-time-budget=6000', '--run-all-compositor-stages-before-draw',
           f'--screenshot={out}', f'http://127.0.0.1:{PORT}/{page}']
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
    ok = os.path.exists(out) and os.path.getsize(out) > 10_000
    results.append((name, ok, os.path.getsize(out) if os.path.exists(out) else 0))
    print(f"{'ok ' if ok else 'BAD'} {name:16s} {results[-1][2]:>8} bytes", flush=True)
    if not ok:
        err = [l for l in r.stderr.splitlines() if 'ERROR' in l or 'error' in l.lower()][:3]
        for l in err: print('     ', l[:200])

print(f"\n{sum(1 for _,ok,_ in results if ok)}/{len(results)} rendered -> {OUT}")
