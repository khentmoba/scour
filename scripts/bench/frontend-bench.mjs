// Scour frontend benchmark: loads the real dist/ UI in headless Edge (CDP) and
// measures boot, init and render/selection performance. Outside the Tauri
// webview the app installs its own mock bridge, so this exercises the exact
// production DOM code paths with deterministic data.
//
// Usage: node scripts/bench/frontend-bench.mjs [--quick] [--out results.json]
// No visible window: Edge runs --headless=new --mute-audio and is killed on exit.

import { spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import os from 'node:os';
import { performance } from 'node:perf_hooks';

const args = process.argv.slice(2);
const quick = args.includes('--quick');
const outIdx = args.indexOf('--out');
const outFile = outIdx >= 0 ? args[outIdx + 1] : null;

const repoRoot = path.resolve(import.meta.dirname, '..', '..');
const distDir = path.join(repoRoot, 'dist');
const edgeCandidates = [
  'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
  'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
];
const edge = edgeCandidates.find((p) => fs.existsSync(p));
if (!edge) { console.error('Edge not found'); process.exit(1); }

const profileDir = fs.mkdtempSync(path.join(os.tmpdir(), 'scour-fe-bench-'));
const port = 9333;
const results = [];
const round = (n) => Math.round(n * 100) / 100;

const child = spawn(edge, [
  '--headless=new',
  `--remote-debugging-port=${port}`,
  `--user-data-dir=${profileDir}`,
  '--no-first-run', '--no-default-browser-check', '--disable-gpu',
  '--mute-audio', '--disable-background-networking', '--disable-extensions',
  '--window-size=1280,900',
  'about:blank',
], { windowsHide: true, stdio: 'ignore' });

let ws, msgId = 0;
const pending = new Map();
const events = [];

async function waitForDebugger() {
  for (let i = 0; i < 50; i++) {
    try {
      const r = await fetch(`http://127.0.0.1:${port}/json/list`);
      const tabs = await r.json();
      const page = tabs.find((t) => t.type === 'page');
      if (page) return page.webSocketDebuggerUrl;
    } catch { /* not up yet */ }
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error('headless Edge did not expose CDP');
}

function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++msgId;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });
}

async function evaluate(expression) {
  const r = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (r.exceptionDetails) throw new Error('page eval failed: ' + JSON.stringify(r.exceptionDetails.exception?.description || r.exceptionDetails.text));
  return r.result.value;
}

function record(label, samples, extra = {}) {
  samples = samples.slice().sort((a, b) => a - b);
  const entry = {
    suite: 'frontend',
    label,
    iterations: samples.length,
    ms_min: round(samples[0]),
    ms_median: round(samples[Math.floor(samples.length / 2)]),
    ms_max: round(samples[samples.length - 1]),
    ...extra,
  };
  results.push(entry);
  console.log(`[frontend] ${label.padEnd(46)} median ${String(entry.ms_median).padStart(9)} ms  (n=${samples.length})`);
}

async function freshPage(url) {
  await send('Page.navigate', { url });
  await new Promise((resolve) => {
    const check = setInterval(async () => {
      try {
        const state = await evaluate('document.readyState');
        if (state === 'complete') { clearInterval(check); resolve(); }
      } catch { /* navigating */ }
    }, 25);
  });
}

const mime = { '.html': 'text/html; charset=utf-8', '.js': 'application/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.svg': 'image/svg+xml', '.png': 'image/png' };
const server = http.createServer((req, res) => {
  const cleanPath = (req.url || '/').split('?')[0];
  const rel = cleanPath === '/' ? 'index.html' : cleanPath.replace(/^\/+/, '');
  const full = path.join(distDir, rel);
  if (!full.startsWith(distDir) || !fs.existsSync(full) || fs.statSync(full).isDirectory()) {
    res.writeHead(404); res.end('Not found'); return;
  }
  res.writeHead(200, { 'Content-Type': mime[path.extname(full)] || 'application/octet-stream' });
  fs.createReadStream(full).pipe(res);
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const baseUrl = `http://127.0.0.1:${server.address().port}/index.html`;

try {
  const wsUrl = await waitForDebugger();
  ws = new WebSocket(wsUrl);
  await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
    } else if (msg.method) {
      events.push(msg);
    }
  };
  await send('Page.enable');
  await send('Runtime.enable');

  // --- cold boot runs ---------------------------------------------------------
  const boots = quick ? 2 : 4;
  const bootSamples = { domContentLoaded: [], load: [], firstPaint: [], initDone: [] };
  for (let i = 0; i < boots; i++) {
    events.length = 0;
    await freshPage(baseUrl + '?boot=' + i);
    await new Promise((r) => setTimeout(r, 600)); // let init() finish its mock invokes
    const timing = await evaluate(`(() => {
      const nav = performance.getEntriesByType('navigation')[0];
      const paints = performance.getEntriesByType('paint');
      const fp = paints.find(p => p.name === 'first-paint');
      const fcp = paints.find(p => p.name === 'first-contentful-paint');
      return {
        domContentLoaded: nav ? nav.domContentLoadedEventEnd : null,
        load: nav ? nav.loadEventEnd : null,
        firstPaint: fp ? fp.startTime : null,
        firstContentfulPaint: fcp ? fcp.startTime : null,
        engineLabel: document.getElementById('engine-label').textContent,
        appVersion: document.getElementById('update-current').textContent,
      };
    })()`);
    if (timing.domContentLoaded != null) bootSamples.domContentLoaded.push(timing.domContentLoaded);
    if (timing.load != null) bootSamples.load.push(timing.load);
    if (timing.firstPaint != null) bootSamples.firstPaint.push(timing.firstPaint);
    // init() completes when both version fields are populated by the mock bridge
    if (timing.engineLabel.includes('burrow-engine') && timing.appVersion !== '' && timing.appVersion !== 'unknown') {
      bootSamples.initDone.push(timing.load ?? timing.domContentLoaded);
    }
  }
  record('boot: DOMContentLoaded', bootSamples.domContentLoaded);
  record('boot: load event', bootSamples.load);
  record('boot: first paint', bootSamples.firstPaint);

  // --- resource sizes ----------------------------------------------------------
  const sizes = {};
  for (const f of ['index.html', 'main.js', 'styles.css']) {
    sizes[f] = fs.statSync(path.join(distDir, f)).size;
  }
  results.push({ suite: 'frontend', label: 'asset sizes (bytes)', ...sizes });
  console.log(`[frontend] assets: ${JSON.stringify(sizes)}`);

  // --- overview scaling (production scan_all -> dithered bar + treemap) --------
  await freshPage(baseUrl + '?render');
  await new Promise((r) => setTimeout(r, 600));
  const sizesToTest = quick ? [100, 1000] : [100, 1000, 5000];
  for (const n of sizesToTest) {
    const reps = quick ? 2 : 3;
    const samples = [];
    let domNodes = 0, heapMB = 0;
    for (let r = 0; r < reps; r++) {
      const m = await evaluate(`(async () => {
        const items = Array.from({length: ${n}}, (_, i) => ({
          path: 'C:\\\\bench\\\\item-' + i, label: 'Item ' + i,
          category: i % 2 ? 'Caches' : 'Developer', bytes: 1024 * (i % 997),
          human: (i % 997) + 'KB', detail: 'benchmark', removable: true
        }));
        const total = items.reduce((s, x) => s + x.bytes, 0);
        window.__TAURI_INTERNALS__.invoke = async (cmd) => {
          if (cmd === 'scan_all') return {
            volume_total: 512 * 1024 ** 3, volume_used: 300 * 1024 ** 3,
            disk: [{ name: 'Developer', bytes: 100 * 1024 ** 3, human: '100 GB' },
                   { name: 'Caches', bytes: 50 * 1024 ** 3, human: '50 GB' }],
            items, total_bytes: total, total_human: 'x', notes: [], scanned_at: 0
          };
          if (cmd === 'engine_version') return { version: '0.1.0', engine: 'burrow-engine' };
          throw new Error('unexpected ' + cmd);
        };
        const totalEl = document.getElementById('clean-total');
        totalEl.textContent = '—';
        const t0 = performance.now();
        document.getElementById('scan-clean').click();
        while (totalEl.textContent === '—') {
          await new Promise(r => setTimeout(r, 1));
        }
        await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
        const ms = performance.now() - t0;
        return { ms, domNodes: document.getElementsByTagName('*').length,
                 heapMB: performance.memory ? performance.memory.usedJSHeapSize / 1048576 : null };
      })()`);
      samples.push(m.ms);
      domNodes = m.domNodes;
      heapMB = m.heapMB ?? 0;
    }
    record(`overview ${n} items (scan click -> treemap painted)`, samples, { dom_nodes: domNodes, heap_mb: round(heapMB) });

    // tile selection toggles through the canvas hit-test
    const selSamples = [];
    for (let r = 0; r < reps; r++) {
      const ms = await evaluate(`(() => {
        const t0 = performance.now();
        const c = document.getElementById('treemap');
        const box = c.getBoundingClientRect();
        for (let i = 0; i < 12; i++) {
          c.dispatchEvent(new MouseEvent('click', {
            clientX: box.left + 12 + i * 40, clientY: box.top + 40, bubbles: true
          }));
        }
        return performance.now() - t0;
      })()`);
      selSamples.push(ms);
    }
    record(`treemap selection toggles x12 (${n} items)`, selSamples);
  }

  // --- analyze render ----------------------------------------------------------
  const analyzeN = quick ? 500 : 2000;
  const analyzeSamples = [];
  for (let r = 0; r < (quick ? 2 : 3); r++) {
    const ms = await evaluate(`(async () => {
      const entries = Array.from({length: ${analyzeN}}, (_, i) => ({
        name: 'entry-' + i, path: 'C:\\\\bench\\\\entry-' + i, size: 2048 * (i % 500), is_dir: i % 3 === 0
      }));
      window.__TAURI_INTERNALS__.invoke = async (cmd) => {
        if (cmd === 'analyze_dir') return { path: 'C:\\\\bench', overview: false, entries };
        throw new Error('unexpected ' + cmd);
      };
      document.getElementById('analyze-path').value = 'C:\\\\bench';
      const list = document.getElementById('analyze-list');
      delete list.dataset.count;
      list.innerHTML = '';
      const t0 = performance.now();
      document.getElementById('scan-analyze').click();
      while (Number(list.dataset.count) !== ${analyzeN} && list.children.length !== ${analyzeN}) {
        await new Promise(r => setTimeout(r, 1));
      }
      await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
      return performance.now() - t0;
    })()`);
    analyzeSamples.push(ms);
  }
  record(`analyze render ${analyzeN} entries`, analyzeSamples);

  // --- idle heap / DOM baseline -------------------------------------------------
  const idle = await evaluate(`({
    domNodes: document.getElementsByTagName('*').length,
    heapMB: performance.memory ? performance.memory.usedJSHeapSize / 1048576 : null,
    listeners: 'n/a'
  })`);
  results.push({ suite: 'frontend', label: 'idle page metrics', dom_nodes: idle.domNodes, heap_mb: idle.heapMB != null ? round(idle.heapMB) : null });
} finally {
  try { ws?.close(); } catch {}
  try { server.close(); } catch {}
  child.kill();
  // give Edge a moment to exit, then sweep its profile dir
  await new Promise((r) => setTimeout(r, 800));
  try { fs.rmSync(profileDir, { recursive: true, force: true }); } catch {}
}

const payload = { suite: 'frontend', quick, results };
if (outFile) fs.writeFileSync(outFile, JSON.stringify(payload, null, 2));
console.log(`[frontend] ${results.length} measurements${outFile ? ` -> ${outFile}` : ''}`);