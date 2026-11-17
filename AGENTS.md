# Repository Instructions

## Performance & Benchmarks

- The performance baseline lives in [`BENCHMARK.md`](BENCHMARK.md).
- Run the automated benchmark suite with:
  ```powershell
  powershell -ExecutionPolicy Bypass -File scripts/bench/run-bench.ps1
  ```
- Any performance-related change must keep all metrics at or better than the baseline in `BENCHMARK.md`. When a change measurably improves the numbers, update `BENCHMARK.md` with the new baseline and record the change in the Optimization Log.
