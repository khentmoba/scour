// Scour frontend. In the Tauri webview, commands go through the native bridge;
// outside it, a small mock keeps the UI usable for browser-based visual checks.
const $ = (id) => document.getElementById(id);

if (typeof window.__TAURI_INTERNALS__?.invoke !== 'function') {
  const callbacks = new Map();
  let nextId = 1;
  window.__TAURI_INTERNALS__ = {
    invoke: mockInvoke,
    transformCallback: (fn) => { const id = nextId++; callbacks.set(id, fn); return id; },
    unregisterCallback: (id) => callbacks.delete(id),
    runCallback: (id, data) => { const fn = callbacks.get(id); if (fn) fn(data); },
  };
}

async function invoke(cmd, args = {}) {
  return window.__TAURI_INTERNALS__.invoke(cmd, args);
}

async function listen(event, handler) {
  const bridge = window.__TAURI_INTERNALS__;
  if (typeof bridge.invoke !== 'function' || typeof bridge.transformCallback !== 'function') {
    return () => {};
  }
  const handlerId = bridge.transformCallback((e) => handler(e.payload));
  const eventId = await bridge.invoke('plugin:event|listen', {
    event,
    target: { kind: 'Any' },
    handler: handlerId,
  });
  return async () => {
    try { await bridge.invoke('plugin:event|unlisten', { event, eventId }); } catch (_) {}
    try { window.__TAURI_EVENT_PLUGIN_INTERNALS__?.unregisterListener?.(event, eventId); } catch (_) {}
  };
}

const mockListeners = new Map();
function mockEmit(event, payload) {
  for (const id of mockListeners.get(event) || []) {
    window.__TAURI_INTERNALS__.runCallback(id, { event, id: -1, payload });
  }
}

const GB = 1024 ** 3;
function mockInvoke(cmd, args) {
  const wait = () => new Promise((r) => setTimeout(r, 220));
  if (cmd === 'plugin:event|listen') {
    const ids = mockListeners.get(args.event) || [];
    ids.push(args.handler);
    mockListeners.set(args.event, ids);
    return wait().then(() => ids.length);
  }
  if (cmd === 'plugin:event|unlisten') {
    const ids = (mockListeners.get(args.event) || []).filter((id) => id !== args.eventId);
    mockListeners.set(args.event, ids);
    return wait().then(() => null);
  }
  if (cmd === 'app_version') return wait().then(() => '0.1.0');
  if (cmd === 'check_update') return wait().then(() => ({
    available: true, current_version: '0.1.0', version: '0.2.0',
    notes: 'Mock release — installation is disabled in the browser preview.', date: '2026-11-17',
  }));
  if (cmd === 'install_update') {
    return new Promise((resolve) => {
      let sent = 0;
      const timer = setInterval(() => {
        sent += 1;
        mockEmit('scour://update-progress', { phase: 'download', downloaded: sent * 25, total: 100, percent: sent * 25 });
        if (sent === 4) {
          clearInterval(timer);
          mockEmit('scour://update-progress', { phase: 'downloaded', percent: 100 });
          resolve({ installed: true, version: '0.2.0', restart_required: true });
        }
      }, 220);
    });
  }
  if (cmd === 'engine_version') return wait().then(() => ({ version: '0.1.0', engine: 'burrow-engine' }));
  if (cmd === 'scan_all') {
    const items = [
      { path: 'C:\\Users\\You\\.cache\\huggingface', label: 'Hugging Face models', category: 'Developer', bytes: 15.0 * GB, human: '15.0 GB', detail: 'Downloaded again when needed.', removable: true },
      { path: 'C:\\Users\\You\\source\\app\\node_modules', label: 'Node modules', category: 'Developer', bytes: 8.4 * GB, human: '8.4 GB', detail: 'Installed again with npm install.', removable: true },
      { path: 'C:\\Users\\You\\Downloads\\setup-old.exe', label: 'setup-old.exe', category: 'Installers', bytes: 3.1 * GB, human: '3.1 GB', detail: 'Installer older than 30 days.', removable: true },
      { path: 'C:\\Users\\You\\AppData\\Local\\Temp', label: 'Temporary files', category: 'Caches', bytes: 2.3 * GB, human: '2.3 GB', detail: 'Files apps left behind.', removable: true },
      { path: 'C:\\Users\\You\\.cache\\ms-playwright', label: 'Playwright browsers', category: 'Developer', bytes: 1.6 * GB, human: '1.6 GB', detail: 'Reinstalled when needed.', removable: true },
      { path: 'C:\\Users\\You\\source\\web\\.next', label: 'Next.js build', category: 'Developer', bytes: 0.9 * GB, human: '923.0 MB', detail: 'Rebuilt on the next build.', removable: true },
      { path: 'C:\\Users\\You\\.cargo\\registry', label: 'Cargo registry', category: 'Developer', bytes: 0.7 * GB, human: '717.0 MB', detail: 'Downloaded again when needed.', removable: true },
      { path: 'C:\\Users\\You\\AppData\\Local\\Google\\Chrome\\User Data\\Default\\Cache', label: 'Chrome cache', category: 'Caches', bytes: 0.4 * GB, human: '410.0 MB', detail: 'Regenerated automatically.', removable: true },
      { path: 'C:\\Users\\You\\.t3\\worktrees\\old-feature', label: 'Clean worktree', category: 'Developer', bytes: 0.2 * GB, human: '205.0 MB', detail: 'No local changes.', removable: true },
    ];
    const total = items.reduce((s, i) => s + i.bytes, 0);
    return wait().then(() => ({
      volume_total: 512 * GB,
      volume_used: 357.8 * GB,
      disk: [
        { name: 'Developer', bytes: 112.0 * GB, human: '112.0 GB' },
        { name: 'Apps', bytes: 52.6 * GB, human: '52.6 GB' },
        { name: 'Photos', bytes: 38.6 * GB, human: '38.6 GB' },
        { name: 'Documents', bytes: 31.7 * GB, human: '31.7 GB' },
        { name: 'Movies', bytes: 24.1 * GB, human: '24.1 GB' },
        { name: 'Caches', bytes: 22.0 * GB, human: '22.0 GB' },
        { name: 'Music', bytes: 12.1 * GB, human: '12.1 GB' },
        { name: 'Trash', bytes: 6.4 * GB, human: '6.4 GB' },
      ],
      items,
      total_bytes: total,
      total_human: fmt(total),
      notes: [],
      scanned_at: Date.now() / 1000,
    }));
  }
  if (cmd === 'app_inventory') return wait().then(() => [
    { name: 'Visual Studio Build Tools', bytes: 12.4 * GB, human: '12.4 GB', location: 'C:\\Program Files (x86)\\Microsoft Visual Studio', id: 'vs_buildtools' },
    { name: 'Google Chrome', bytes: 1.1 * GB, human: '1.1 GB', location: 'C:\\Program Files\\Google\\Chrome\\Application', id: 'Chrome' },
    { name: 'Node.js', bytes: 186.0 * 1024 * 1024, human: '186.0 MB', location: 'C:\\Program Files\\nodejs', id: 'nodejs' },
  ]);
  if (cmd === 'analyze_dir') return wait().then(() => ({
    path: args.path, overview: false, entries: [
      { name: 'Downloads', path: args.path + '\\Downloads', size: 2147000000, is_dir: true },
      { name: 'Videos', path: args.path + '\\Videos', size: 861800000, is_dir: true },
      { name: 'node_modules', path: args.path + '\\node_modules', size: 483700000, is_dir: true },
      { name: 'notes.txt', path: args.path + '\\notes.txt', size: 12043, is_dir: false },
    ],
  }));
  if (cmd === 'find_index') return wait().then(() => ({ base: args.folder, total_matched: 4, total_files: 4, items: [] }));
  if (cmd === 'find_search') {
    const all = [
      { path: args.folder + '\\setup-installer.exe', name: 'setup-installer.exe', size: 115800000, modified: 1750000000, score: 120 },
      { path: args.folder + '\\nested\\old_backup.log', name: 'old_backup.log', size: 2048000, modified: 1740000000, score: 90 },
      { path: args.folder + '\\draft-report.docx', name: 'draft-report.docx', size: 48210, modified: 1755000000, score: 60 },
      { path: args.folder + '\\notes.txt', name: 'notes.txt', size: 12043, modified: 1756000000, score: 40 },
    ];
    const q = (args.query || '').trim().toLowerCase().replace(/^\*\./, '');
    const items = q ? all.filter((f) => f.name.toLowerCase().includes(q.slice(0, Math.max(q.length - 1, 2)))) : all;
    return wait().then(() => ({ base: args.folder, total_matched: items.length, total_files: 4, items }));
  }
  if (cmd === 'apply_clean') return wait().then(() => [
    ...args.paths.map((path) => ({ event: 'removed', path, bytes: 214700000 })),
    { event: 'done', dry_run: false, freed_bytes: 214700000 * args.paths.length, count: args.paths.length },
  ]);
  if (cmd === 'system_status') return wait().then(() => ({ ok: false, error: { message: 'system metrics unavailable on this platform', kind: 'unsupported' } }));
  if (cmd === 'preview_optimize') return wait().then(() => ({ dry_run: true, tasks: [
    { name: 'flush_dns', description: 'Flush the DNS resolver cache' },
    { name: 'clear_thumbnails', description: 'Clear the thumbnail cache' },
  ] }));
  throw new Error('unknown command: ' + cmd);
}

const LOG_1024 = Math.log(1024);
const SIZE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];
function fmt(bytes) {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const i = Math.min(Math.floor(Math.log(bytes) / LOG_1024), SIZE_UNITS.length - 1);
  return `${(bytes / 1024 ** i).toFixed(i ? 1 : 0)} ${SIZE_UNITS[i]}`;
}

let toastTimer;
function toast(msg, bad = false) {
  const el = $('toast');
  el.textContent = msg;
  el.classList.toggle('bad', bad);
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { el.hidden = true; }, 4200);
}

let activeNav = document.querySelector('.nav-item.active[data-view]');
let activeView = document.querySelector('.view.active');
const viewMap = new Map();
document.querySelectorAll('.nav-item[data-view]').forEach((b) => {
  const key = b.dataset.view;
  viewMap.set(key, { nav: b, viewEl: $(`view-${key}`) });
  b.addEventListener('click', () => {
    show(key);
    if (key === 'clean' && overview && !overview.loaded) runScan();
  });
});

let appVersionLoaded = false;
function show(view) {
  const target = viewMap.get(view);
  if (!target) return;
  if (activeNav && activeNav !== target.nav) activeNav.classList.remove('active');
  if (activeView && activeView !== target.viewEl) activeView.classList.remove('active');
  target.nav.classList.add('active');
  if (target.viewEl) target.viewEl.classList.add('active');
  activeNav = target.nav;
  activeView = target.viewEl;
  if (view === 'update' && !appVersionLoaded) {
    appVersionLoaded = true;
    invoke('app_version')
      .then((v) => { $('update-current').textContent = v; })
      .catch(() => { $('update-current').textContent = 'unknown'; });
  }
}

/* =========================================================================
   OVERVIEW — dithered disk bar + treemap, inspired by Beautiful Disk.
   All dots are drawn on a grid with a Bayer/hash threshold; no image assets.
   ========================================================================= */
const TONES = {
  Apps:      { base: '#0f88e6', dark: '#054476', light: '#88c2fd' },
  Developer: { base: '#129f4b', dark: '#015021', light: '#7cd490' },
  Photos:    { base: '#d34895', dark: '#6a244a', light: '#fa99c8' },
  Movies:    { base: '#9863e4', dark: '#4c3172', light: '#c8abfd' },
  Documents: { base: '#0197a3', dark: '#034b52', light: '#2dd4e3' },
  Music:     { base: '#9d8306', dark: '#4e4107', light: '#d6bb51' },
  System:    { base: '#81858c', dark: '#404246', light: '#b9bdc2' },
  Caches:    { base: '#c76809', dark: '#663200', light: '#fca568' },
  Trash:     { base: '#e04746', dark: '#712423', light: '#fd9f97' },
  Installers:{ base: '#c0843a', dark: '#5c3a12', light: '#f5c787' },
};
const CAT_ORDER = ['Apps', 'Developer', 'Photos', 'Movies', 'Documents', 'Music', 'System', 'Caches', 'Trash'];
const BAYER = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
const bayer = (c, r) => (BAYER[(r & 3) * 4 + (c & 3)] + 0.5) / 16;
function hash(x, y) {
  let h = Math.imul(x, 374761393) + Math.imul(y, 668265263);
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967295;
}
function rgba(hex, a) {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${a})`;
}
function roundRect(ctx, x, y, w, h, r) {
  const rr = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + rr, y);
  ctx.arcTo(x + w, y, x + w, y + h, rr);
  ctx.arcTo(x + w, y + h, x, y + h, rr);
  ctx.arcTo(x, y + h, x, y, rr);
  ctx.arcTo(x, y, x + w, y, rr);
  ctx.closePath();
}
function dot(ctx, c, r, cell, size, ox = 0, oy = 0) {
  ctx.fillRect(ox + c * cell + (cell - size) / 2, oy + r * cell + (cell - size) / 2, size, size);
}

const overview = {
  loaded: false,
  scanning: false,
  payload: null,
  selected: new Set(),
  hoverCat: null,
  hoverTile: null,
  tiles: [],
  raf: 0,
  animUntil: 0,
};

function fitCanvas(canvas) {
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth || 600;
  const h = canvas.clientHeight || 300;
  const nw = Math.max(1, Math.round(w * dpr));
  const nh = Math.max(1, Math.round(h * dpr));
  if (canvas.width !== nw || canvas.height !== nh) {
    canvas.width = nw;
    canvas.height = nh;
  }
  const ctx = canvas.getContext('2d');
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return { ctx, w, h };
}

function drawBar() {
  const p = overview.payload;
  const canvas = $('disk-bar');
  const { ctx, w, h } = fitCanvas(canvas);
  ctx.clearRect(0, 0, w, h);
  roundRect(ctx, 0.5, 0.5, w - 1, h - 1, 7);
  ctx.fillStyle = '#1b1b1e';
  ctx.fill();
  ctx.strokeStyle = 'rgba(255,255,255,.06)';
  ctx.lineWidth = 1;
  ctx.stroke();

  const CELL = 5, DOT = 3;
  const cols = Math.floor(w / CELL), rows = Math.floor(h / CELL);
  const total = p.volume_total || 1;
  const used = p.volume_used || 0;
  const usedCols = Math.min(cols, Math.floor((cols * used) / total));
  const flicker = Math.floor(Date.now() / 50);

  ctx.fillStyle = 'rgba(255,255,255,.045)';
  for (let c = usedCols; c < cols; c++) {
    for (let r = 0; r < rows; r++) dot(ctx, c, r, CELL, 2);
  }

  let col = 0;
  for (const name of CAT_ORDER) {
    const slice = (p.disk || []).find((s) => s.name === name);
    if (!slice || !slice.bytes) continue;
    const n = Math.max(1, Math.round((cols * slice.bytes) / total));
    const ends = Math.min(n, cols - col);
    if (ends <= 0) break;
    const tone = TONES[name] || TONES.System;
    const glow = overview.hoverCat ? (overview.hoverCat === name ? 1 : 0.15) : 0.45;

    roundRect(ctx, col * CELL + 1, 1, ends * CELL - 2, h - 2, 6);
    ctx.fillStyle = rgba(tone.dark, 0.22 + 0.3 * glow);
    ctx.fill();
    ctx.strokeStyle = rgba(tone.base, 0.35 + 0.65 * glow);
    ctx.lineWidth = 1;
    ctx.stroke();

    for (let k = 0; k < ends; k++) {
      for (let r = 0; r < rows; r++) {
        const fy = r / Math.max(1, rows - 1);
        const near = Math.max(0, 1 - (ends - 1 - k) / 8);
        const density = (0.62 - 0.5 * fy) * (0.3 + 0.7 * glow) + 0.28 * glow * glow + near * 0.9;
        if (hash((col + k + flicker) & 255, r) >= density) continue;
        ctx.fillStyle = rgba(tone.light, Math.min(1, 0.4 + 0.6 * Math.max(glow, near)));
        dot(ctx, col + k, r, CELL, DOT);
      }
    }
    col += n;
  }
}

function renderLegend() {
  const p = overview.payload;
  const box = $('disk-legend');
  const frag = document.createDocumentFragment();
  for (const name of CAT_ORDER) {
    const slice = (p.disk || []).find((s) => s.name === name);
    if (!slice || !slice.bytes) continue;
    const el = document.createElement('div');
    el.className = 'legend-item' + (overview.hoverCat && overview.hoverCat !== name ? ' dim' : '');
    const sw = document.createElement('span');
    sw.className = 'legend-swatch';
    sw.style.background = (TONES[name] || TONES.System).base;
    const nm = document.createElement('span');
    nm.className = 'legend-name';
    nm.textContent = name === 'System' ? 'System data' : name;
    const sz = document.createElement('span');
    sz.className = 'legend-size';
    sz.textContent = slice.human || fmt(slice.bytes);
    el.append(sw, nm, sz);
    el.addEventListener('mouseenter', () => { overview.hoverCat = name; refresh(); showCatStatus(name); });
    el.addEventListener('mouseleave', () => { overview.hoverCat = null; refresh(); defaultStatus(); });
    frag.appendChild(el);
  }
  box.replaceChildren(frag);
}

function showCatStatus(name) {
  const p = overview.payload;
  const slice = (p.disk || []).find((s) => s.name === name);
  if (!slice) return;
  const detail = {
    Apps: 'The largest app is the biggest slice here.',
    Developer: 'Build output and downloaded packages.',
    Photos: 'Pictures and photos in your profile.',
    Movies: 'Videos in your profile.',
    Documents: 'Files in Documents, Desktop and Downloads.',
    Music: 'Audio files in your profile.',
    System: 'Windows, app data and files outside your folders.',
    Caches: 'Apps rebuild these when they need them again.',
    Trash: 'Things you have already deleted.',
  }[name] || '';
  $('disk-status').textContent = `${name === 'System' ? 'System data' : name} — ${slice.human || fmt(slice.bytes)}. ${detail}`;
}
function defaultStatus() {
  const p = overview.payload;
  const available = p.volume_total && p.volume_used ? p.volume_total - p.volume_used : 0;
  $('disk-status').textContent = available ? `${fmt(available)} available` : 'Ready';
}

function layoutTreemap(items, x, y, w, h) {
  if (!items.length) return [];
  const sorted = [...items].sort((a, b) => b.bytes - a.bytes);
  const out = [];
  const split = (list, rx, ry, rw, rh) => {
    if (!list.length || rw <= 0 || rh <= 0) return;
    if (list.length === 1) {
      out.push({ item: list[0], x: rx, y: ry, w: rw, h: rh });
      return;
    }
    let sum = list.reduce((s, i) => s + i.bytes, 0) || 1;
    let acc = 0, cut = 0;
    for (; cut < list.length - 1; cut++) { acc += list[cut].bytes; if (acc >= sum / 2) break; }
    const a = list.slice(0, cut + 1), b = list.slice(cut + 1);
    const sa = a.reduce((s, i) => s + i.bytes, 0);
    const sb = b.reduce((s, i) => s + i.bytes, 0);
    const t = sa + sb || 1;
    if (rw >= rh) {
      const w1 = rw * (sa / t);
      split(a, rx, ry, w1, rh);
      split(b, rx + w1, ry, rw - w1, rh);
    } else {
      const h1 = rh * (sa / t);
      split(a, rx, ry, rw, h1);
      split(b, rx, ry + h1, rw, rh - h1);
    }
  };
  split(sorted, x, y, w, h);
  return out;
}

function drawTreemap() {
  const p = overview.payload;
  const canvas = $('treemap');
  const { ctx, w, h } = fitCanvas(canvas);
  ctx.clearRect(0, 0, w, h);
  const items = (p.items || []).filter((i) => i.removable !== false);
  overview.tiles = layoutTreemap(items, 1, 1, w - 2, h - 2);
  const CELL = 4, DOT = 3, GAP = 2;
  const max = Math.max(...items.map((i) => i.bytes), 1);

  for (const tile of overview.tiles) {
    const { x, y, w: tw, h: th, item } = tile;
    const sel = overview.selected.has(item.path);
    const hot = overview.hoverTile === item.path;
    const tone = TONES[item.category] || TONES.Developer;
    const glow = sel ? (hot ? 1 : 0.6) : (hot ? 0.5 : 0);

    roundRect(ctx, x + GAP, y + GAP, Math.max(0, tw - GAP * 2), Math.max(0, th - GAP * 2), 7);
    ctx.fillStyle = sel ? rgba(tone.dark, 0.18 + 0.12 * glow) : '#1b1b1e';
    ctx.fill();
    ctx.strokeStyle = sel ? rgba(tone.base, 0.45 + 0.55 * glow) : `rgba(255,255,255,${0.05 + 0.08 * glow})`;
    ctx.lineWidth = 1;
    ctx.stroke();

    ctx.save();
    ctx.beginPath();
    ctx.rect(x + GAP, y + GAP, Math.max(0, tw - GAP * 2), Math.max(0, th - GAP * 2));
    ctx.clip();
    const cols = Math.ceil((tw - GAP * 2) / CELL);
    const rows = Math.ceil((th - GAP * 2) / CELL);
    for (let c = 0; c < cols; c++) {
      for (let r = 0; r < rows; r++) {
        const u = c / Math.max(1, cols - 1);
        const v = r / Math.max(1, rows - 1);
        if (sel) {
          if (u < 0.38) continue; // keep the label band clear
          const density = (0.1 + 0.82 * Math.pow(v, 1.3) * (0.6 + 0.4 * u)) * (0.6 + 0.4 * glow);
          if (bayer(c, r) >= density) continue;
          ctx.fillStyle = rgba(tone.light, 0.55 + 0.45 * glow);
        } else {
          if ((c + r) % 4 !== 0) continue;
          ctx.fillStyle = `rgba(255,255,255,${0.13 + 0.1 * glow})`;
        }
        dot(ctx, c, r, CELL, DOT, x + GAP, y + GAP);
      }
    }
    ctx.restore();

    const tiny = tw < 122 || th < 62;
    const small = tw < 140 || th < 84;
    const pad = small ? 7 : 11;
    const innerW = Math.max(0, tw - GAP * 2 - pad * 2);
    ctx.textBaseline = 'top';
    if (!tiny && innerW > 24) {
      ctx.fillStyle = sel ? '#ededef' : '#77777c';
      ctx.font = `${small ? 12 : 13}px -apple-system, "Segoe UI", sans-serif`;
      ctx.fillText(item.label || 'Untitled', x + GAP + pad, y + GAP + pad, innerW);
    }
    if (!tiny && th >= 70 && tw >= 96) {
      ctx.fillStyle = sel ? rgba(tone.light, 0.95) : '#8b8b90';
      ctx.font = `bold ${small ? 10 : 12}px Menlo, Consolas, monospace`;
      ctx.fillText(item.human || fmt(item.bytes), x + GAP + pad, y + th - GAP - pad - (small ? 10 : 12), innerW);
    }
    void max;
  }
}

function updateSelectionUI() {
  const p = overview.payload;
  const chosen = (p.items || []).filter((i) => i.removable !== false && overview.selected.has(i.path));
  const bytes = chosen.reduce((s, i) => s + i.bytes, 0);
  $('clean-total').textContent = `${fmt(bytes)} can go`;
  $('clean-sub').textContent = chosen.length
    ? `${chosen.length} item${chosen.length === 1 ? '' : 's'} selected · moved to the Recycle Bin`
    : `${(p.items || []).length} things found · tick what you trust`;
  $('apply-clean').disabled = chosen.length === 0;
  $('clean-hint').textContent = chosen.length
    ? `${chosen.length} selected. Only these exact paths are deleted, and only to the Recycle Bin.`
    : 'Deletions go to the Recycle Bin — nothing is permanent.';
}

function positionTip(clientX, clientY) {
  const tip = $('item-tip');
  const pad = 14;
  const w = tip.offsetWidth || 360;
  const h = tip.offsetHeight || 120;
  let x = clientX + 16, y = clientY + 16;
  if (x + w > window.innerWidth - pad) x = clientX - w - 16;
  if (y + h > window.innerHeight - pad) y = window.innerHeight - h - pad;
  tip.style.left = `${Math.max(pad, x)}px`;
  tip.style.top = `${Math.max(pad, y)}px`;
}

function showItemTip(item, clientX, clientY) {
  const tip = $('item-tip');
  const tone = TONES[item.category] || TONES.Developer;
  tip.innerHTML = `
    <div class="tip-title"><span class="tip-name"></span><span class="tip-size"></span></div>
    <div class="tip-note"></div>
    <div class="tip-row"><span class="tip-path"></span><span class="tip-amt"></span></div>`;
  tip.querySelector('.tip-name').textContent = item.label;
  tip.querySelector('.tip-name').style.color = tone.light;
  tip.querySelector('.tip-size').textContent = item.human || fmt(item.bytes);
  tip.querySelector('.tip-note').textContent = item.detail || '';
  tip.querySelector('.tip-path').textContent = item.path;
  tip.querySelector('.tip-amt').textContent = 'Recycle Bin';
  tip.hidden = false;
  positionTip(clientX, clientY);
}

function refresh() {
  if (!overview.payload) return;
  drawBar();
  drawTreemap();
  updateSelectionUI();
}
function animate() {
  const now = performance.now();
  if (now < overview.animUntil) {
    drawBar();
    overview.raf = requestAnimationFrame(animate);
  } else {
    overview.raf = 0;
  }
}
function kick(ms = 400) {
  overview.animUntil = performance.now() + ms;
  if (!overview.raf) overview.raf = requestAnimationFrame(animate);
}

async function runScan() {
  if (overview.scanning) return;
  overview.scanning = true;
  const btn = $('scan-clean');
  btn.disabled = true;
  btn.textContent = 'Scanning…';
  $('clean-sub').textContent = 'Looking for things you can clear';
  $('disk-status').textContent = 'Calculating…';
  try {
    const payload = await invoke('scan_all');
    overview.payload = payload;
    overview.loaded = true;
    overview.selected = new Set((payload.items || []).filter((i) => i.removable !== false).map((i) => i.path));
    overview.hoverCat = null;
    overview.hoverTile = null;
    const used = payload.volume_used || 0;
    const total = payload.volume_total || 0;
    $('disk-used').textContent = fmt(used);
    $('disk-of').textContent = total ? `of ${fmt(total)} used` : '';
    renderLegend();
    defaultStatus();
    refresh();
    kick(650);
    if (payload.notes && payload.notes.length) toast(payload.notes[0]);
  } catch (e) {
    $('disk-status').textContent = 'scan failed';
    toast(String(e), true);
  } finally {
    overview.scanning = false;
    btn.disabled = false;
    btn.textContent = 'Rescan';
  }
}

$('scan-clean').addEventListener('click', runScan);

$('disk-bar').addEventListener('mousemove', (e) => {
  const p = overview.payload;
  if (!p || !p.volume_total) return;
  const rect = e.currentTarget.getBoundingClientRect();
  const frac = (e.clientX - rect.left) / rect.width;
  let acc = 0;
  let found = null;
  for (const name of CAT_ORDER) {
    const s = (p.disk || []).find((x) => x.name === name);
    if (!s || !s.bytes) continue;
    const share = s.bytes / p.volume_total;
    if (frac >= acc && frac < acc + share) { found = name; break; }
    acc += share;
  }
  if (found !== overview.hoverCat) {
    overview.hoverCat = found;
    renderLegend();
    drawBar();
    if (found) showCatStatus(found); else defaultStatus();
  }
});
$('disk-bar').addEventListener('mouseleave', () => {
  overview.hoverCat = null;
  renderLegend();
  drawBar();
  defaultStatus();
});

$('treemap').addEventListener('mousemove', (e) => {
  const rect = e.currentTarget.getBoundingClientRect();
  const x = e.clientX - rect.left, y = e.clientY - rect.top;
  const tile = overview.tiles.find((t) => x >= t.x && x <= t.x + t.w && y >= t.y && y <= t.y + t.h);
  const path = tile ? tile.item.path : null;
  if (path !== overview.hoverTile) {
    overview.hoverTile = path;
    drawTreemap();
  }
  if (tile) showItemTip(tile.item, e.clientX, e.clientY);
  else $('item-tip').hidden = true;
});
$('treemap').addEventListener('mouseleave', () => {
  overview.hoverTile = null;
  $('item-tip').hidden = true;
  drawTreemap();
});
$('treemap').addEventListener('click', (e) => {
  const rect = e.currentTarget.getBoundingClientRect();
  const x = e.clientX - rect.left, y = e.clientY - rect.top;
  const tile = overview.tiles.find((t) => x >= t.x && x <= t.x + t.w && y >= t.y && y <= t.y + t.h);
  if (!tile) return;
  if (overview.selected.has(tile.item.path)) overview.selected.delete(tile.item.path);
  else overview.selected.add(tile.item.path);
  drawTreemap();
  updateSelectionUI();
});
window.addEventListener('resize', () => refresh());

$('apply-clean').addEventListener('click', async () => {
  const chosen = (overview.payload.items || []).filter((i) => i.removable !== false && overview.selected.has(i.path));
  if (!chosen.length) return;
  const paths = chosen.map((i) => i.path);
  const bytes = chosen.reduce((s, i) => s + i.bytes, 0);
  $('confirm-text').textContent = `${paths.length} item(s), about ${fmt(bytes)}, will be moved to the Recycle Bin. Only these exact paths are deleted.`;
  const dlg = $('confirm-dialog');
  dlg.returnValue = '';
  dlg.showModal();
  const choice = await new Promise((resolve) => {
    dlg.addEventListener('close', () => resolve(dlg.returnValue), { once: true });
  });
  if (choice !== 'yes') return;

  const btn = $('apply-clean');
  btn.disabled = true;
  btn.textContent = 'Clearing…';
  try {
    const events = await invoke('apply_clean', { paths });
    const panel = $('result-panel');
    panel.hidden = false;
    let removed = 0;
    const failed = [];
    for (const ev of events) {
      if (ev.event === 'removed') removed++;
      if (ev.event === 'failed') failed.push(ev.path);
    }
    $('result-log').textContent = events.map((e) => JSON.stringify(e)).join('\n');
    if (failed.length === 0) {
      $('result-status').textContent = `${removed} removed`;
      $('result-status').className = 'pill ok';
      toast(`Done — ${removed} item(s) moved to the Recycle Bin.`);
      overview.payload.items = overview.payload.items.filter((i) => !paths.includes(i.path));
      overview.selected.clear();
      refresh();
      kick(500);
    } else {
      $('result-status').textContent = `${removed} of ${paths.length} removed — incomplete`;
      $('result-status').className = 'pill bad';
      toast(`Could not remove ${failed.length} item(s). Only those remain selected.`, true);
      overview.selected = new Set(failed);
      refresh();
    }
  } catch (e) {
    $('result-panel').hidden = false;
    $('result-status').textContent = 'failed';
    $('result-status').className = 'pill bad';
    $('result-log').textContent = String(e);
    toast(String(e), true);
  } finally {
    btn.disabled = false;
    btn.textContent = 'Clear';
  }
});

/* ===================== APPS (read-only inventory) ===================== */
async function loadApps() {
  const btn = $('apps-refresh');
  btn.disabled = true;
  try {
    const apps = await invoke('app_inventory');
    $('apps-list')._loaded = true;
    $('apps-count').textContent = `${apps.length} apps`;
    const list = $('apps-list');
    if (!apps.length) {
      list.innerHTML = '<div class="empty">No installed apps were reported.</div>';
      return;
    }
    let max = 1;
    for (const a of apps) max = Math.max(max, a.bytes || 0);
    mountVirtualList(
      list,
      apps,
      () => {
        const row = document.createElement('div');
        row.className = 'row';
        row.innerHTML = '<div class="row-main"><div class="row-label"></div><div class="row-path"></div><div class="bar"><i></i></div></div><div class="row-size"></div>';
        const main = row.firstElementChild;
        row._refs = { label: main.children[0], path: main.children[1], bar: main.children[2].firstElementChild, size: main.nextElementSibling };
        return row;
      },
      (row, a) => {
        const r = row._refs;
        r.label.textContent = a.name;
        r.path.textContent = a.location || a.id;
        r.bar.style.transform = `scaleX(${Math.max((a.bytes || 0) / max, 0.02)})`;
        r.size.textContent = a.human || fmt(a.bytes);
      }
    );
  } catch (e) {
    toast(String(e), true);
  } finally {
    btn.disabled = false;
  }
}
$('apps-refresh').addEventListener('click', loadApps);
viewMap.get('apps').nav.addEventListener('click', () => { if (!$('apps-list')._pool && !$('apps-list')._loaded) loadApps(); });

/* ===================== ANALYZE ===================== */
function teardownVirtualList(list) {
  if (list._onScroll) { list.removeEventListener('scroll', list._onScroll); list._onScroll = null; }
  if (list._raf) { cancelAnimationFrame(list._raf); list._raf = 0; }
  if (list._ro) { list._ro.disconnect(); list._ro = null; }
  list._pool = null;
}

function mountVirtualList(list, items, createRow, updateRow) {
  teardownVirtualList(list);
  list.dataset.count = String(items.length);
  if (!items.length) return;
  let viewH = list.clientHeight || 420;
  const ROW_HEIGHT = 64, OVERSCAN = 4;
  const spacer = document.createElement('div');
  spacer.className = 'list-spacer';
  spacer.style.height = `${items.length * ROW_HEIGHT}px`;
  const viewport = document.createElement('div');
  viewport.className = 'list-viewport';
  spacer.appendChild(viewport);
  list.replaceChildren(spacer);
  list.scrollTop = 0;

  const pool = [], free = [], activeSlots = [];
  list._pool = pool;
  let lastStart = -1, lastEnd = -1;
  const paint = (force = false) => {
    const scrollTop = list.scrollTop;
    const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
    const end = Math.min(items.length, Math.ceil((scrollTop + viewH) / ROW_HEIGHT) + OVERSCAN);
    if (!force && start === lastStart && end === lastEnd) return;
    lastStart = start; lastEnd = end;
    const needed = end - start;
    let newFrag = null;
    if (pool.length < needed) {
      newFrag = document.createDocumentFragment();
      while (pool.length < needed) {
        const node = createRow();
        node._idx = -1;
        pool.push(node);
        newFrag.appendChild(node);
      }
    }
    if (force) for (let k = 0; k < pool.length; k++) pool[k]._idx = -1;
    free.length = 0;
    if (activeSlots.length < needed) activeSlots.length = needed;
    activeSlots.fill(0, 0, needed);
    for (let k = 0; k < pool.length; k++) {
      const node = pool[k];
      const idx = node._idx;
      if (idx >= start && idx < end) activeSlots[idx - start] = 1;
      else free.push(node);
    }
    for (let k = 0; k < needed; k++) {
      if (activeSlots[k]) continue;
      const idx = start + k;
      const node = free.pop();
      node._idx = idx;
      if (node.style.visibility) node.style.visibility = '';
      node.style.transform = `translateY(${idx * ROW_HEIGHT}px)`;
      updateRow(node, items[idx], idx);
    }
    for (let k = 0; k < free.length; k++) {
      free[k].style.visibility = 'hidden';
      free[k]._idx = -1;
    }
    if (newFrag) viewport.appendChild(newFrag);
  };
  if (typeof ResizeObserver === 'function') {
    list._ro = new ResizeObserver(() => {
      const hh = list.clientHeight || 420;
      if (hh !== viewH) { viewH = hh; paint(false); }
    });
    list._ro.observe(list);
  }
  list._onScroll = () => {
    if (list._raf) return;
    list._raf = requestAnimationFrame(() => { list._raf = 0; paint(false); });
  };
  list.addEventListener('scroll', list._onScroll, { passive: true });
  paint(true);
}

function renderAnalyzeTpl() {
  return renderAnalyzeTpl._tpl || (renderAnalyzeTpl._tpl = (() => {
    const t = document.createElement('template');
    t.innerHTML = '<div class="row"><div class="row-main"><div class="row-label"></div><div class="row-path"></div><div class="bar"><i></i></div></div><div class="row-size"></div></div>';
    return t;
  })());
}

$('scan-analyze').addEventListener('click', async () => {
  const path = $('analyze-path').value.trim();
  if (!path) { toast('Enter a folder path first.', true); return; }
  const btn = $('scan-analyze');
  btn.disabled = true; btn.textContent = 'Analyzing…';
  try {
    const data = await invoke('analyze_dir', { path });
    const entries = data.entries || [];
    const list = $('analyze-list');
    if (!entries.length) {
      teardownVirtualList(list);
      list.dataset.count = '0';
      list.innerHTML = '<div class="empty">No entries found.</div>';
      return;
    }
    let max = 1;
    for (let i = 0; i < entries.length; i++) max = Math.max(max, entries[i].size || 0);
    const tpl = renderAnalyzeTpl();
    mountVirtualList(list, entries,
      () => {
        const row = tpl.content.firstElementChild.cloneNode(true);
        const main = row.firstElementChild;
        row._refs = { label: main.children[0], path: main.children[1], bar: main.children[2].firstElementChild, size: main.nextElementSibling };
        return row;
      },
      (row, e) => {
        const r = row._refs;
        r.label.textContent = e.name;
        r.path.textContent = e.path;
        r.bar.style.transform = `scaleX(${Math.max((e.size || 0) / max, 0.02)})`;
        r.size.textContent = e._sizeStr || (e._sizeStr = e.is_dir ? fmt(e.size) + '  ▸' : fmt(e.size));
      });
  } catch (e) {
    toast(String(e), true);
  } finally {
    btn.disabled = false; btn.textContent = 'Analyze folder';
  }
});

/* ===================== FIND ===================== */
let findHits = [];
let findSelected = new Set();
let findBase = '';
let findSeq = 0;

function fmtDate(secs) {
  if (!secs) return '';
  return new Date(secs * 1000).toISOString().slice(0, 10);
}

function renderFind(data) {
  findHits = data.items || [];
  findBase = data.base || findBase;
  findSelected = new Set();
  $('find-summary').textContent = `${data.total_matched} match(es) of ${data.total_files} indexed`;
  $('find-select-all').disabled = findHits.length === 0;
  $('find-select-all').textContent = 'Select all';
  const list = $('find-list');
  list.innerHTML = '';
  if (!findHits.length) {
    list.innerHTML = '<div class="empty">No matches. Try fewer characters or a different spelling.</div>';
    updateFindSelection();
    return;
  }
  const max = Math.max(...findHits.map((h) => h.size || 0), 1);
  findHits.forEach((h, i) => {
    const row = document.createElement('label');
    row.className = 'row';
    row.innerHTML = `
      <input type="checkbox" data-i="${i}" />
      <div class="row-main">
        <div class="row-label"></div>
        <div class="row-path"></div>
        <div class="bar"><i style="width:${Math.max((h.size || 0) / max * 100, 2)}%"></i></div>
      </div>
      <div class="row-size"></div>`;
    row.querySelector('.row-label').textContent = h.name;
    row.querySelector('.row-path').textContent = h.path + (h.modified ? '  ·  ' + fmtDate(h.modified) : '');
    row.querySelector('.row-size').textContent = fmt(h.size);
    row.querySelector('input').addEventListener('change', (e) => {
      const n = Number(e.target.dataset.i);
      e.target.checked ? findSelected.add(n) : findSelected.delete(n);
      updateFindSelection();
    });
    list.appendChild(row);
  });
  updateFindSelection();
}

function updateFindSelection() {
  $('find-clean').disabled = findSelected.size === 0;
  $('find-select-all').textContent = findHits.length && findSelected.size === findHits.length ? 'Deselect all' : 'Select all';
}

async function runFindSearch() {
  const seq = ++findSeq;
  try {
    const data = await invoke('find_search', { query: $('find-query').value, limit: 100 });
    if (seq === findSeq) renderFind(data);
  } catch (e) {
    toast(String(e), true);
  }
}

$('find-index').addEventListener('click', async () => {
  const folder = $('find-path').value.trim();
  if (!folder) { toast('Enter a folder path first.', true); return; }
  const btn = $('find-index');
  btn.disabled = true; btn.textContent = 'Indexing…';
  $('find-state').textContent = 'indexing…';
  $('find-state').className = 'pill';
  try {
    const data = await invoke('find_index', { folder });
    findBase = data.base;
    $('find-query').disabled = false;
    $('find-state').textContent = `${data.total_files} files indexed`;
    $('find-state').className = 'pill ok';
    $('find-query').focus();
    await runFindSearch();
  } catch (e) {
    $('find-state').textContent = 'index failed';
    $('find-state').className = 'pill bad';
    toast(String(e), true);
  } finally {
    btn.disabled = false; btn.textContent = 'Index folder';
  }
});

let findDebounce;
$('find-query').addEventListener('input', () => {
  clearTimeout(findDebounce);
  findDebounce = setTimeout(runFindSearch, 120);
});

$('find-select-all').addEventListener('click', () => {
  if (findSelected.size === findHits.length) findSelected.clear();
  else findHits.forEach((_, i) => findSelected.add(i));
  document.querySelectorAll('#find-list input[type=checkbox]').forEach((box) => {
    box.checked = findSelected.has(Number(box.dataset.i));
  });
  updateFindSelection();
});

$('find-clean').addEventListener('click', async () => {
  const paths = [...findSelected].map((i) => findHits[i].path);
  if (!paths.length) return;
  const total = [...findSelected].reduce((s, i) => s + (findHits[i].size || 0), 0);
  $('confirm-text').textContent = `${paths.length} item(s), about ${fmt(total)}, will be moved to the Recycle Bin. Only these exact paths are deleted.`;
  const dlg = $('confirm-dialog');
  dlg.returnValue = '';
  dlg.showModal();
  const choice = await new Promise((resolve) => {
    dlg.addEventListener('close', () => resolve(dlg.returnValue), { once: true });
  });
  if (choice !== 'yes') return;
  const btn = $('find-clean');
  btn.disabled = true; btn.textContent = 'Cleaning…';
  try {
    const events = await invoke('apply_clean', { paths });
    const removedPaths = events.filter((e) => e.event === 'removed').map((e) => e.path);
    const failed = events.filter((e) => e.event === 'failed');
    if (failed.length === 0 && removedPaths.length === paths.length) {
      toast(`Done — ${removedPaths.length} item(s) moved to the Recycle Bin.`);
      findHits = findHits.filter((h) => !removedPaths.includes(h.path));
      renderFind({ base: findBase, total_matched: findHits.length, total_files: findHits.length, items: findHits });
    } else {
      toast(`Incomplete — could not remove: ${failed.map((e) => e.path).join(', ') || 'see log'}.`, true);
      await runFindSearch();
    }
  } catch (e) {
    toast(String(e), true);
  } finally {
    btn.disabled = false; btn.textContent = 'Clean selected';
  }
});

/* ===================== STATUS ===================== */
$('refresh-status').addEventListener('click', async () => {
  try {
    const data = await invoke('system_status');
    const cards = $('status-cards');
    const d = data && data.ok !== false ? data.data || data : null;
    const metrics = d && !d.error ? [
      ['Health', d.health_score ?? '—'],
      ['CPU', (d.cpu && d.cpu.usage != null) ? d.cpu.usage + '%' : '—'],
      ['Memory', d.memory && d.memory.total ? fmt(d.memory.total) : '—'],
    ] : [['Result', 'unavailable']];
    const tpl = cards._tpl || (cards._tpl = (() => {
      const t = document.createElement('template');
      t.innerHTML = '<div class="card"><div class="card-label"></div><div class="card-value"></div></div>';
      return t;
    })());
    const frag = document.createDocumentFragment();
    metrics.forEach(([label, value]) => {
      const c = tpl.content.firstElementChild.cloneNode(true);
      c.firstElementChild.textContent = label;
      c.lastElementChild.textContent = value;
      frag.appendChild(c);
    });
    cards.replaceChildren(frag);
    $('status-raw').textContent = JSON.stringify(data, null, 2);
  } catch (e) {
    $('status-raw').textContent = String(e);
    toast(String(e), true);
  }
});

/* ===================== OPTIMIZE ===================== */
$('preview-optimize').addEventListener('click', async () => {
  try {
    const data = await invoke('preview_optimize');
    const list = $('optimize-list');
    const tpl = list._tpl || (list._tpl = (() => {
      const t = document.createElement('template');
      t.innerHTML = '<div class="row"><div class="row-main"><div class="row-label"></div></div></div>';
      return t;
    })());
    const frag = document.createDocumentFragment();
    (data.tasks || []).forEach((t) => {
      const row = tpl.content.firstElementChild.cloneNode(true);
      row.firstElementChild.firstElementChild.textContent = t.description || t.name;
      frag.appendChild(row);
    });
    list.replaceChildren(frag);
  } catch (e) { toast(String(e), true); }
});

/* ===================== UPDATES ===================== */
const UPDATE_EVENT = 'scour://update-progress';
let updateAvailable = null;

function setUpdateState(text, bad = false) {
  const pill = $('update-state');
  pill.textContent = text;
  pill.className = 'pill' + (bad ? ' bad' : text === 'up to date' ? ' ok' : '');
  $('update-status-card').textContent = text;
}

$('check-update').addEventListener('click', async () => {
  const btn = $('check-update');
  btn.disabled = true; btn.textContent = 'Checking…';
  updateAvailable = null;
  $('install-update').hidden = true;
  $('update-log').hidden = true;
  $('update-progress').hidden = true;
  $('update-bar').style.width = '0%';
  $('update-latest').textContent = '—';
  setUpdateState('checking…');
  $('update-notes').textContent = 'Checking GitHub Releases…';
  try {
    const result = await invoke('check_update');
    $('update-log').hidden = false;
    $('update-log').textContent = JSON.stringify(result, null, 2);
    $('update-latest').textContent = result.version || '—';
    if (result.available) {
      updateAvailable = result;
      setUpdateState('update available');
      $('update-notes').textContent = result.notes || `Version ${result.version} is ready.`;
      $('install-update').hidden = false;
      $('update-detail').textContent = `Version ${result.current_version} → ${result.version}. One click downloads the setup from GitHub Releases, installs it and restarts Scour.`;
    } else {
      setUpdateState('up to date');
      $('update-notes').textContent = `You are running the latest version (${result.version}).`;
      $('update-detail').textContent = 'Checks GitHub Releases only when you click. Nothing is checked or downloaded in the background.';
    }
  } catch (e) {
    setUpdateState('check failed', true);
    $('update-notes').textContent = String(e);
    toast(String(e), true);
  } finally {
    btn.disabled = false; btn.textContent = 'Check for updates';
  }
});

$('install-update').addEventListener('click', async () => {
  if (!updateAvailable) return;
  const btn = $('install-update');
  btn.disabled = true; btn.textContent = 'Installing…';
  $('check-update').disabled = true;
  $('update-progress').hidden = false;
  $('update-bar').style.width = '0%';
  setUpdateState('downloading…');
  $('update-detail').textContent = 'Downloading the installer from GitHub Releases. Scour will restart automatically.';
  let unlisten = () => {};
  try {
    unlisten = await listen(UPDATE_EVENT, (p) => {
      if (p.phase === 'downloaded') {
        $('update-bar').style.width = '100%';
        setUpdateState('installing…');
        $('update-detail').textContent = 'Download finished. Installing and restarting Scour…';
        return;
      }
      const pct = Number(p.percent || 0);
      $('update-bar').style.width = `${pct}%`;
      setUpdateState(`downloading ${Math.round(pct)}%`);
    });
    const result = await invoke('install_update');
    $('update-log').hidden = false;
    $('update-log').textContent = JSON.stringify(result, null, 2);
    setUpdateState('installed');
    $('update-detail').textContent = result.restart_required
      ? 'Update installed. Close and reopen Scour to run the new version.'
      : 'Update is installing — Scour will restart in a moment.';
  } catch (e) {
    setUpdateState('install failed', true);
    $('update-detail').textContent = String(e);
    toast(String(e), true);
    $('update-progress').hidden = true;
  } finally {
    unlisten();
    btn.disabled = false; btn.textContent = 'Update & restart';
    $('check-update').disabled = false;
  }
});

(async function init() {
  runScan();
  try {
    const v = await invoke('engine_version');
    $('about-version').textContent = v.version || 'unknown';
    $('engine-label').textContent = 'engine: ' + (v.engine || 'burrow-engine') + ' v' + (v.version || '?');
    $('engine-dot').className = 'dot ok';
  } catch (e) {
    $('engine-label').textContent = 'engine: unavailable';
    $('engine-dot').className = 'dot bad';
  }
})();
