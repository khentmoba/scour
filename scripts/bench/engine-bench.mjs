// Scour engine benchmark: times the real burrow-engine CLI exactly the way
// src-tauri/src/engine.rs invokes it (argv in, JSON out, one process per call).
//
// Usage: node scripts/bench/engine-bench.mjs [--quick] [--out results.json]
// Fixtures are synthetic files under the session temp dir and are removed on exit.
// Nothing outside the fixture root and the engine's own dry-run reads is touched.

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { performance } from 'node:perf_hooks';

const args = process.argv.slice(2);
const quick = args.includes('--quick');
const wholePc = args.includes('--whole-pc');
const outIdx = args.indexOf('--out');
const outFile = outIdx >= 0 ? args[outIdx + 1] : null;

const repoRoot = path.resolve(import.meta.dirname, '..', '..');
const engineExe = process.env.SCOUR_ENGINE
  || path.join(repoRoot, 'src-tauri', 'binaries', 'burrow-engine.exe');
if (!fs.existsSync(engineExe)) {
  console.error(`engine not found: ${engineExe}`);
  process.exit(1);
}

const benchRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'scour-engine-bench-'));
const results = [];

function runEngine(argv, { iterations = 3, label = argv.join(' '), maxBuffer = 256 * 1024 * 1024 } = {}) {
  const samples = [];
  let last = null;
  for (let i = 0; i < iterations; i++) {
    const t0 = performance.now();
    const r = spawnSync(engineExe, argv, { encoding: 'utf8', maxBuffer, windowsHide: true });
    const ms = performance.now() - t0;
    samples.push(ms);
    last = r;
  }
  let parsed = null;
  let dataInfo = {};
  try {
    parsed = JSON.parse((last.stdout || '').trim());
    const d = parsed?.data;
    if (d) {
      if (Array.isArray(d.items)) dataInfo = { items: d.items.length, total_bytes: d.total_bytes ?? null };
      if (Array.isArray(d.entries)) dataInfo = { entries: d.entries.length };
      if (Array.isArray(d.tasks)) dataInfo = { tasks: d.tasks.length };
    }
  } catch { /* keep raw info below */ }
  samples.sort((a, b) => a - b);
  const entry = {
    suite: 'engine-cli',
    label,
    argv,
    iterations,
    ms_min: round(samples[0]),
    ms_median: round(samples[Math.floor(samples.length / 2)]),
    ms_max: round(samples[samples.length - 1]),
    exit_code: last.status,
    ok: parsed?.ok ?? null,
    stdout_bytes: Buffer.byteLength(last.stdout || ''),
    stderr: (last.stderr || '').trim().slice(0, 300) || undefined,
    ...dataInfo,
  };
  results.push(entry);
  console.log(`[engine] ${label.padEnd(46)} median ${String(entry.ms_median).padStart(9)} ms  (n=${iterations}, ok=${entry.ok})`);
  return entry;
}

const round = (n) => Math.round(n * 100) / 100;

// --- synthetic fixtures -----------------------------------------------------
// Deterministic pseudo-random sizes so runs are comparable across machines.
function makeFixture(name, fileCount, dirs, maxFileKB) {
  const root = path.join(benchRoot, name);
  let seed = 123456789;
  const rnd = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff;
  const t0 = performance.now();
  let bytes = 0;
  const perDir = Math.ceil(fileCount / dirs);
  for (let d = 0; d < dirs; d++) {
    const dir = path.join(root, `dir-${String(d).padStart(3, '0')}`);
    fs.mkdirSync(dir, { recursive: true });
    for (let f = 0; f < perDir && d * perDir + f < fileCount; f++) {
      const size = Math.max(256, Math.floor(rnd() * maxFileKB * 1024));
      fs.writeFileSync(path.join(dir, `file-${String(f).padStart(5, '0')}.bin`), Buffer.alloc(size, 0x61));
      bytes += size;
    }
  }
  const setupMs = performance.now() - t0;
  console.log(`[fixture] ${name}: ${fileCount} files in ${dirs} dirs, ${(bytes / 1048576).toFixed(1)} MiB, created in ${Math.round(setupMs)} ms`);
  return { root, fileCount, bytes, setupMs: round(setupMs) };
}

function makeDeepFixture(name, depth, filesPerLevel) {
  const root = path.join(benchRoot, name);
  let dir = root;
  let bytes = 0;
  const t0 = performance.now();
  for (let i = 0; i < depth; i++) {
    dir = path.join(dir, `level-${i}`);
    fs.mkdirSync(dir, { recursive: true });
    for (let f = 0; f < filesPerLevel; f++) {
      const size = 2048 + f * 512;
      fs.writeFileSync(path.join(dir, `f-${f}.bin`), Buffer.alloc(size, 0x62));
      bytes += size;
    }
  }
  return { root, fileCount: depth * filesPerLevel, bytes, setupMs: round(performance.now() - t0) };
}

try {
  // --- fixed commands (the exact argv Scour's commands.rs uses) --------------
  runEngine(['version'], { iterations: 5 });
  runEngine(['status'], { iterations: 3 });
  runEngine(['clean', '--dry-run'], { iterations: quick ? 2 : 3, label: 'clean --dry-run (whole-PC junk scan)' });
  runEngine(['optimize', '--dry-run'], { iterations: 3 });

  // --- analyze scaling on synthetic fixtures ---------------------------------
  const fixtures = [
    makeFixture('analyze-small', 100, 4, 8),
    makeFixture('analyze-medium', 2000, 20, 16),
  ];
  if (!quick) fixtures.push(makeFixture('analyze-large', 10000, 50, 32));
  const deep = makeDeepFixture('analyze-deep', 40, 10);
  fixtures.push(deep);

  for (const fx of fixtures) {
    const e = runEngine(['analyze', fx.root], { iterations: quick ? 2 : 3, label: `analyze ${path.basename(fx.root)} (${fx.fileCount} files)` });
    e.fixture = { files: fx.fileCount, bytes: fx.bytes, setup_ms: fx.setupMs };
  }

  // --- real-world targets -----------------------------------------------------
  const profile = process.env.USERPROFILE || os.homedir();
  const downloads = path.join(profile, 'Downloads');
  if (fs.existsSync(downloads)) {
    runEngine(['analyze', downloads], { iterations: 1, label: 'analyze ~/Downloads (real)' });
  }
  if (wholePc && !quick) {
    runEngine(['analyze', profile], { iterations: 1, label: 'analyze user profile (real)' });
    const sysDrive = process.env.SystemDrive || 'C:';
    runEngine(['analyze', sysDrive + '\\'], { iterations: 1, label: `analyze ${sysDrive}\\ (whole drive, real)` });
  }
} finally {
  fs.rmSync(benchRoot, { recursive: true, force: true });
}

const payload = { suite: 'engine-cli', engine: engineExe, quick, results };
if (outFile) fs.writeFileSync(outFile, JSON.stringify(payload, null, 2));
console.log(`[engine] ${results.length} measurements${outFile ? ` -> ${outFile}` : ''}`);