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
  if (cmd === 'find_index') return wait().then(() => ({
    base: args.folder, total_matched: 4, total_files: 4, items: [],
  }));
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
  b.addEventListener('click', () => show(key));
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

let candidates = [];
let selectedMask = new Uint8Array(0);
let selectedCount = 0;

const ROW_HEIGHT = 64;
const OVERSCAN = 4;

function teardownVirtualList(list) {
  if (list._onScroll) {
    list.removeEventListener('scroll', list._onScroll);
    list._onScroll = null;
  }
  if (list._raf) {
    cancelAnimationFrame(list._raf);
    list._raf = 0;
  }
  if (list._ro) {
    list._ro.disconnect();
    list._ro = null;
  }
  list._pool = null;
}

function mountVirtualList(list, items, createRow, updateRow) {
  teardownVirtualList(list);
  list.dataset.count = String(items.length);
  if (!items.length) {
    return;
  }
  let viewH = list.clientHeight || 420;
  const spacer = document.createElement('div');
  spacer.className = 'list-spacer';
  spacer.style.height = `${items.length * ROW_HEIGHT}px`;
  const viewport = document.createElement('div');
  viewport.className = 'list-viewport';
  spacer.appendChild(viewport);
  list.replaceChildren(spacer);
  list.scrollTop = 0;

  const pool = [];
  const free = [];
  const activeSlots = [];
  list._pool = pool;
  let lastStart = -1;
  let lastEnd = -1;
  const paint = (force = false) => {
    const scrollTop = list.scrollTop;
    const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
    const end = Math.min(items.length, Math.ceil((scrollTop + viewH) / ROW_HEIGHT) + OVERSCAN);
    if (!force && start === lastStart && end === lastEnd) return;
    lastStart = start;
    lastEnd = end;
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
    if (force) {
      for (let k = 0; k < pool.length; k++) pool[k]._idx = -1;
    }
    free.length = 0;
    if (activeSlots.length < needed) activeSlots.length = needed;
    activeSlots.fill(0, 0, needed);
    for (let k = 0; k < pool.length; k++) {
      const node = pool[k];
      const idx = node._idx;
      if (idx >= start && idx < end) {
        activeSlots[idx - start] = 1;
      } else {
        free.push(node);
      }
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
      const node = free[k];
      if (!node.style.visibility) node.style.visibility = 'hidden';
      node._idx = -1;
    }
    if (newFrag) viewport.appendChild(newFrag);
  };

  if (typeof ResizeObserver === 'function') {
    list._ro = new ResizeObserver(() => {
      const h = list.clientHeight || 420;
      if (h !== viewH) {
        viewH = h;
        paint(false);
      }
    });
    list._ro.observe(list);
  }

  list._onScroll = () => {
    if (list._raf) return;
    list._raf = requestAnimationFrame(() => {
      list._raf = 0;
      paint(false);
    });
  };
  list.addEventListener('scroll', list._onScroll, { passive: true });
  paint(true);
}

function renderClean(data) {
  candidates = Array.isArray(data.items) ? data.items : [];
  selectedMask = new Uint8Array(candidates.length);
  selectedMask.fill(1);
  selectedCount = candidates.length;
  $('sum-count').textContent = candidates.length;
  $('sum-size').textContent = data.total_human || fmt(data.total_bytes || 0);
  selectAllBtn.disabled = candidates.length === 0;

  const list = $('clean-list');
  if (!candidates.length) {
    teardownVirtualList(list);
    list.dataset.count = '0';
    list.innerHTML = '<div class="empty">Nothing disposable found.</div>';
    updateSelection();
    return;
  }
  let max = 1;
  for (let i = 0; i < candidates.length; i++) {
    const sz = candidates[i].size || 0;
    if (sz > max) max = sz;
  }
  const tpl = renderClean._tpl || (renderClean._tpl = (() => {
    const t = document.createElement('template');
    t.innerHTML = '<label class="row"><input type="checkbox" /><div class="row-main"><div class="row-label"></div><div class="row-path"></div><div class="bar"><i></i></div></div><div class="row-size"></div></label>';
    return t;
  })());
  mountVirtualList(
    list,
    candidates,
    () => {
      const row = tpl.content.firstElementChild.cloneNode(true);
      const box = row.firstElementChild;
      const main = box.nextElementSibling;
      row._refs = {
        box,
        label: main.children[0],
        path: main.children[1],
        bar: main.children[2].firstElementChild,
        size: main.nextElementSibling,
      };
      return row;
    },
    (row, c, i) => {
      const r = row._refs;
      r.box._idx = i;
      r.box.checked = selectedMask[i] === 1;
      r.label.textContent = c.label || c.path;
      r.path.textContent = c.path;
      r.bar.style.transform = `scaleX(${Math.max((c.size || 0) / max, 0.02)})`;
      r.size.textContent = c.size_human || c._sizeStr || (c._sizeStr = fmt(c.size));
    },
  );
  updateSelection();
}

const sumSelectedEl = $('sum-selected');
const applyCleanBtn = $('apply-clean');
const selectAllBtn = $('select-all');

$('clean-list').addEventListener('change', (e) => {
  const target = e.target;
  if (target && target.tagName === 'INPUT' && target.type === 'checkbox') {
    const n = target._idx;
    const next = target.checked ? 1 : 0;
    if (selectedMask[n] !== next) {
      selectedMask[n] = next;
      selectedCount += next ? 1 : -1;
      updateSelection();
    }
  }
});

function updateSelection() {
  sumSelectedEl.textContent = selectedCount;
  applyCleanBtn.disabled = selectedCount === 0;
  selectAllBtn.textContent = selectedCount === candidates.length ? 'Deselect all' : 'Select all';
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
  const checkAll = selectedCount !== candidates.length;
  selectedMask.fill(checkAll ? 1 : 0);
  selectedCount = checkAll ? candidates.length : 0;
  const pool = $('clean-list')._pool;
  if (pool) {
    for (let i = 0; i < pool.length; i++) {
      if (pool[i]._idx >= 0) pool[i]._refs.box.checked = checkAll;
    }
  }
  updateSelection();
});

$('apply-clean').addEventListener('click', async () => {
  if (!selectedCount) return;
  const paths = [];
  let total = 0;
  for (let i = 0; i < candidates.length; i++) {
    if (selectedMask[i]) {
      const item = candidates[i];
      paths.push(item.path);
      total += item.size || 0;
    }
  }
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
    for (const ev of events) {
      if (ev.event === 'removed') removed++;
      if (ev.event === 'failed') failed.push(ev.path);
    }
    const complete = failed.length === 0 && removed === paths.length;
    const maxLog = 200;
    if (events.length <= maxLog) {
      $('result-log').textContent = events.map((e) => JSON.stringify(e)).join('\n');
    } else {
      const head = events.slice(0, 100).map((e) => JSON.stringify(e));
      const tail = events.slice(-50).map((e) => JSON.stringify(e));
      $('result-log').textContent = `${head.join('\n')}\n… (${events.length - 150} more events omitted) …\n${tail.join('\n')}`;
    }
    if (complete) {
      $('result-status').textContent = `${removed} removed`;
      $('result-status').className = 'pill ok';
      toast(`Done — ${removed} item(s) moved to the Recycle Bin.`);
      selectedCount = 0;
      renderClean({ items: [], total_bytes: 0, total_human: '0 B' });
    } else {
      let missedPaths = failed;
      if (!missedPaths.length) {
        const removedSet = new Set();
        for (const ev of events) {
          if (ev.event === 'removed') removedSet.add(ev.path);
        }
        missedPaths = paths.filter((p) => !removedSet.has(p));
      }
      // Keep only what actually failed. Removed paths no longer exist, and the
      // backend refuses a batch containing a missing path.
      const retry = new Set(missedPaths);
      let nextCount = 0;
      for (let i = 0; i < candidates.length; i++) {
        if (selectedMask[i] && retry.has(candidates[i].path)) {
          selectedMask[i] = 1;
          nextCount++;
        } else {
          selectedMask[i] = 0;
        }
      }
      selectedCount = nextCount;
      const pool = $('clean-list')._pool;
      if (pool) {
        for (let i = 0; i < pool.length; i++) {
          const row = pool[i];
          if (row._idx >= 0) row._refs.box.checked = selectedMask[row._idx] === 1;
        }
      }
      updateSelection();
      $('result-status').textContent = `${removed} of ${paths.length} removed — incomplete`;
      $('result-status').className = 'pill bad';
      const shown = missedPaths.slice(0, 5).join(', ');
      const extra = missedPaths.length > 5 ? ` (+${missedPaths.length - 5} more)` : '';
      toast(`Incomplete — could not remove: ${shown}${extra}. Only those remain selected for a retry.`, true);
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
    if (!entries.length) {
      teardownVirtualList(list);
      list.dataset.count = '0';
      list.innerHTML = '<div class="empty">No entries found.</div>';
      return;
    }
    let max = 1;
    for (let i = 0; i < entries.length; i++) {
      const sz = entries[i].size || 0;
      if (sz > max) max = sz;
    }
    const tpl = renderAnalyzeTpl();
    mountVirtualList(
      list,
      entries,
      () => {
        const row = tpl.content.firstElementChild.cloneNode(true);
        const main = row.firstElementChild;
        row._refs = {
          label: main.children[0],
          path: main.children[1],
          bar: main.children[2].firstElementChild,
          size: main.nextElementSibling,
        };
        return row;
      },
      (row, e) => {
        const r = row._refs;
        r.label.textContent = e.name;
        r.path.textContent = e.path;
        r.bar.style.transform = `scaleX(${Math.max((e.size || 0) / max, 0.02)})`;
        r.size.textContent = e._sizeStr || (e._sizeStr = e.is_dir ? fmt(e.size) + '  ▸' : fmt(e.size));
      },
    );
  } catch (e) {
    toast(String(e), true);
  } finally {
    btn.disabled = false; btn.textContent = 'Analyze folder';
  }
});

// FIND — fff-backed fuzzy search over one user-picked folder.
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
  $('find-select-all').textContent =
    findHits.length && findSelected.size === findHits.length ? 'Deselect all' : 'Select all';
}

async function runFindSearch() {
  const seq = ++findSeq;
  try {
    const data = await invoke('find_search', { query: $('find-query').value, limit: 100 });
    if (seq === findSeq) renderFind(data); // drop stale responses while typing
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

function renderAnalyzeTpl() {
  return renderAnalyzeTpl._tpl || (renderAnalyzeTpl._tpl = (() => {
    const t = document.createElement('template');
    t.innerHTML = '<div class="row"><div class="row-main"><div class="row-label"></div><div class="row-path"></div><div class="bar"><i></i></div></div><div class="row-size"></div></div>';
    return t;
  })());
}

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
})();
