// Scour frontend. In the Tauri webview, commands go through the native bridge;
// outside it, a small mock keeps the UI usable for browser-based visual checks.
const $ = (id) => document.getElementById(id);

// In a plain browser, install a tiny shim with the same invoke/callback shape as
// Tauri's bridge so the whole UI — including update progress — can be previewed.
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

// Subscribe to a Rust `app.emit` event using the bridge primitives the Tauri
// runtime already ships, so no npm/event package is needed in this no-bundler app.
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
    available: true,
    current_version: '0.1.0',
    version: '0.2.0',
    notes: 'Mock release — installation is disabled in the browser preview.',
    date: '2026-11-17',
  }));
  if (cmd === 'install_update') {
    return new Promise((resolve) => {
      let sent = 0;
      const timer = setInterval(() => {
        sent += 1;
        mockEmit('scour://update-progress', {
          phase: 'download', downloaded: sent * 25, total: 100, percent: sent * 25,
        });
        if (sent === 4) {
          clearInterval(timer);
          mockEmit('scour://update-progress', { phase: 'downloaded', percent: 100 });
          resolve({ installed: true, version: '0.2.0', restart_required: true });
        }
      }, 220);
    });
  }
  if (cmd === 'engine_version') return wait().then(() => ({ version: '0.1.0', engine: 'burrow-engine' }));
  if (cmd === 'scan_clean') return wait().then(() => ({
    dry_run: true, total_bytes: 482100000, total_human: '482.1MB', items: [
      { path: 'C:\\Users\\You\\AppData\\Local\\Temp', label: 'Temporary files', size: 214700000, size_human: '214.7MB' },
      { path: 'C:\\Users\\You\\.cache', label: 'Application cache', size: 151600000, size_human: '151.6MB' },
      { path: 'C:\\Users\\You\\Downloads\\old-installers', label: 'Old installers', size: 115800000, size_human: '115.8MB' },
    ],
  }));
  if (cmd === 'analyze_dir') return wait().then(() => ({
    path: args.path, overview: false, entries: [
      { name: 'Downloads', path: args.path + '\\Downloads', size: 2147000000, is_dir: true },
      { name: 'Videos', path: args.path + '\\Videos', size: 861800000, is_dir: true },
      { name: 'node_modules', path: args.path + '\\node_modules', size: 483700000, is_dir: true },
      { name: 'notes.txt', path: args.path + '\\notes.txt', size: 12043, is_dir: false },
    ],
  }));
  if (cmd === 'apply_clean') return wait().then(() => [
    { event: 'removed', path: args.paths[0], bytes: 214700000 },
    { event: 'done', dry_run: false, freed_bytes: 214700000, count: args.paths.length },
  ]);
  if (cmd === 'system_status') return wait().then(() => ({ ok: false, error: { message: 'system metrics unavailable on this platform', kind: 'unsupported' } }));
  if (cmd === 'preview_optimize') return wait().then(() => ({ dry_run: true, tasks: [
    { name: 'flush_dns', description: 'Flush the DNS resolver cache' },
    { name: 'clear_thumbnails', description: 'Clear the thumbnail cache' },
  ] }));
  throw new Error('unknown command: ' + cmd);
}

function fmt(bytes) {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return `${(bytes / 1024 ** i).toFixed(i ? 1 : 0)} ${units[i]}`;
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

function show(view) {
  document.querySelectorAll('.nav-item[data-view]').forEach((b) => b.classList.toggle('active', b.dataset.view === view));
  document.querySelectorAll('.view').forEach((v) => v.classList.toggle('active', v.id === `view-${view}`));
}
document.querySelectorAll('.nav-item[data-view]').forEach((b) => b.addEventListener('click', () => show(b.dataset.view)));

let candidates = [];
let selected = new Set();

function renderClean(data) {
  candidates = Array.isArray(data.items) ? data.items : [];
  selected = new Set(candidates.map((_, i) => i));
  $('sum-count').textContent = candidates.length;
  $('sum-size').textContent = data.total_human || fmt(data.total_bytes || 0);
  $('select-all').disabled = candidates.length === 0;
  $('apply-clean').disabled = candidates.length === 0;
  $('select-all').textContent = 'Deselect all';

  const list = $('clean-list');
  list.innerHTML = '';
  if (!candidates.length) {
    list.innerHTML = '<div class="empty">Nothing disposable found.</div>';
    updateSelection();
    return;
  }
  const max = Math.max(...candidates.map((c) => c.size || 0), 1);
  candidates.forEach((c, i) => {
    const row = document.createElement('label');
    row.className = 'row';
    const size = c.size_human || fmt(c.size);
    row.innerHTML = `
      <input type="checkbox" checked data-i="${i}" />
      <div class="row-main">
        <div class="row-label"></div>
        <div class="row-path"></div>
        <div class="bar"><i style="width:${Math.max((c.size || 0) / max * 100, 2)}%"></i></div>
      </div>
      <div class="row-size"></div>`;
    row.querySelector('.row-label').textContent = c.label || c.path;
    row.querySelector('.row-path').textContent = c.path;
    row.querySelector('.row-size').textContent = size;
    row.querySelector('input').addEventListener('change', (e) => {
      const n = Number(e.target.dataset.i);
      e.target.checked ? selected.add(n) : selected.delete(n);
      updateSelection();
    });
    list.appendChild(row);
  });
  updateSelection();
}

function updateSelection() {
  $('sum-selected').textContent = selected.size;
  $('apply-clean').disabled = selected.size === 0;
  $('select-all').textContent = selected.size === candidates.length ? 'Deselect all' : 'Select all';
}

$('scan-clean').addEventListener('click', async () => {
  const btn = $('scan-clean');
  btn.disabled = true; btn.textContent = 'Scanning…';
  try {
    renderClean(await invoke('scan_clean'));
  } catch (e) {
    toast(String(e), true);
  } finally {
    btn.disabled = false; btn.textContent = 'Scan for junk';
  }
});

$('select-all').addEventListener('click', () => {
  if (selected.size === candidates.length) selected.clear();
  else candidates.forEach((_, i) => selected.add(i));
  document.querySelectorAll('#clean-list input[type=checkbox]').forEach((box) => {
    box.checked = selected.has(Number(box.dataset.i));
  });
  updateSelection();
});

$('apply-clean').addEventListener('click', async () => {
  const paths = [...selected].map((i) => candidates[i].path);
  if (!paths.length) return;
  const total = [...selected].reduce((s, i) => s + (candidates[i].size || 0), 0);
  $('confirm-text').textContent = `${paths.length} item(s), about ${fmt(total)}, will be moved to the Recycle Bin. Only these exact paths are deleted.`;
  const dlg = $('confirm-dialog');
  dlg.returnValue = '';
  dlg.showModal();
  const choice = await new Promise((resolve) => {
    dlg.addEventListener('close', () => resolve(dlg.returnValue), { once: true });
  });
  if (choice !== 'yes') return;

  const btn = $('apply-clean');
  btn.disabled = true; btn.textContent = 'Cleaning…';
  try {
    const events = await invoke('apply_clean', { paths });
    const panel = $('result-panel');
    panel.hidden = false;
    let removed = 0;
    const failed = [];
    const removedPaths = [];
    for (const ev of events) {
      if (ev.event === 'removed') { removed++; removedPaths.push(ev.path); }
      if (ev.event === 'failed') failed.push(ev.path);
    }
    const complete = failed.length === 0 && removed === paths.length;
    $('result-log').textContent = events.map((e) => JSON.stringify(e)).join('\n');
    if (complete) {
      $('result-status').textContent = `${removed} removed`;
      $('result-status').className = 'pill ok';
      toast(`Done — ${removed} item(s) moved to the Recycle Bin.`);
      selected.clear();
      renderClean({ items: [], total_bytes: 0, total_human: '0 B' });
    } else {
      const missedPaths = failed.length
        ? failed
        : paths.filter((p) => !removedPaths.includes(p));
      // Keep only what actually failed. Removed paths no longer exist, and the
      // backend refuses a batch containing a missing path.
      const retry = new Set(missedPaths);
      selected = new Set([...selected].filter((i) => retry.has(candidates[i].path)));
      document.querySelectorAll('#clean-list input[type=checkbox]').forEach((box) => {
        box.checked = retry.has(candidates[Number(box.dataset.i)].path);
      });
      updateSelection();
      $('result-status').textContent = `${removed} of ${paths.length} removed — incomplete`;
      $('result-status').className = 'pill bad';
      toast(`Incomplete — could not remove: ${missedPaths.join(', ')}. Only those remain selected for a retry.`, true);
    }
  } catch (e) {
    $('result-panel').hidden = false;
    $('result-status').textContent = 'failed';
    $('result-status').className = 'pill bad';
    $('result-log').textContent = String(e);
    toast(String(e), true);
  } finally {
    btn.disabled = false; btn.textContent = 'Clean selected';
  }
});

$('scan-analyze').addEventListener('click', async () => {
  const path = $('analyze-path').value.trim();
  if (!path) { toast('Enter a folder path first.', true); return; }
  const btn = $('scan-analyze');
  btn.disabled = true; btn.textContent = 'Analyzing…';
  try {
    const data = await invoke('analyze_dir', { path });
    const entries = data.entries || [];
    const list = $('analyze-list');
    list.innerHTML = '';
    if (!entries.length) { list.innerHTML = '<div class="empty">No entries found.</div>'; return; }
    const max = Math.max(...entries.map((e) => e.size || 0), 1);
    entries.forEach((e) => {
      const row = document.createElement('div');
      row.className = 'row';
      row.innerHTML = `<div class="row-main"><div class="row-label"></div><div class="row-path"></div><div class="bar"><i style="width:${Math.max(e.size / max * 100, 2)}%"></i></div></div><div class="row-size"></div>`;
      row.querySelector('.row-label').textContent = e.name;
      row.querySelector('.row-path').textContent = e.path;
      row.querySelector('.row-size').textContent = e.is_dir ? fmt(e.size) + '  ▸' : fmt(e.size);
      list.appendChild(row);
    });
  } catch (e) {
    toast(String(e), true);
  } finally {
    btn.disabled = false; btn.textContent = 'Analyze folder';
  }
});

$('refresh-status').addEventListener('click', async () => {
  try {
    const data = await invoke('system_status');
    const cards = $('status-cards');
    cards.innerHTML = '';
    const d = data && data.ok !== false ? data.data || data : null;
    const metrics = d && !d.error ? [
      ['Health', d.health_score ?? '—'],
      ['CPU', (d.cpu && d.cpu.usage != null) ? d.cpu.usage + '%' : '—'],
      ['Memory', d.memory && d.memory.total ? fmt(d.memory.total) : '—'],
    ] : [['Result', 'unavailable']];
    metrics.forEach(([label, value]) => {
      const c = document.createElement('div');
      c.className = 'card';
      c.innerHTML = `<div class="card-label"></div><div class="card-value"></div>`;
      c.querySelector('.card-label').textContent = label;
      c.querySelector('.card-value').textContent = value;
      cards.appendChild(c);
    });
    $('status-raw').textContent = JSON.stringify(data, null, 2);
  } catch (e) {
    $('status-raw').textContent = String(e);
    toast(String(e), true);
  }
});

$('preview-optimize').addEventListener('click', async () => {
  try {
    const data = await invoke('preview_optimize');
    const list = $('optimize-list');
    list.innerHTML = '';
    (data.tasks || []).forEach((t) => {
      const row = document.createElement('div');
      row.className = 'row';
      row.innerHTML = '<div class="row-main"><div class="row-label"></div></div>';
      row.querySelector('.row-label').textContent = t.description || t.name;
      list.appendChild(row);
    });
  } catch (e) { toast(String(e), true); }
});

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
      $('update-detail').textContent = `Version ${result.current_version} → ${result.version}. One click downloads, verifies and installs, then restarts Scour.`;
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
  $('update-detail').textContent = 'Downloading and verifying the signed installer. Scour will restart automatically.';
  try {
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
    btn.disabled = false; btn.textContent = 'Update & restart';
    $('check-update').disabled = false;
  }
});

listen(UPDATE_EVENT, (p) => {
  if (p.phase === 'downloaded') {
    $('update-bar').style.width = '100%';
    setUpdateState('installing…');
    $('update-detail').textContent = 'Download verified. Installing and restarting Scour…';
    return;
  }
  const pct = Number(p.percent || 0);
  $('update-bar').style.width = `${pct}%`;
  setUpdateState(`downloading ${Math.round(pct)}%`);
});

(async function init() {
  try {
    const v = await invoke('engine_version');
    $('about-version').textContent = v.version || 'unknown';
    $('engine-label').textContent = 'engine: ' + (v.engine || 'burrow-engine') + ' v' + (v.version || '?');
    $('engine-dot').className = 'dot ok';
  } catch (e) {
    $('engine-label').textContent = 'engine: unavailable';
    $('engine-dot').className = 'dot bad';
  }
  try {
    $('update-current').textContent = await invoke('app_version');
  } catch (e) {
    $('update-current').textContent = 'unknown';
  }
})();
