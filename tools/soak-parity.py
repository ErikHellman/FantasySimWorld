#!/usr/bin/env python3
"""Statistical parity between the TypeScript and Rust sims.

Runs `bun tools/soak.ts DAYS SEED --json` and the Rust `soak` binary for each
seed, averages the end-of-run statistics over the seeds and prints the
relative difference of each. The migration plan's gate is ±25%.

    python3 tools/soak-parity.py [days] [seeds]      (defaults: 30 days, 10 seeds)
"""
import json
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DAYS = int(sys.argv[1]) if len(sys.argv) > 1 else 30
SEEDS = range(1, 1 + (int(sys.argv[2]) if len(sys.argv) > 2 else 10))
TOLERANCE = 0.25
# Counts too small to compare meaningfully (a handful of rare events) are reported but not gated.
MIN_GATED = 20

subprocess.run(["cargo", "build", "--release", "-q", "-p", "fsw-sim", "--bin", "soak"], cwd=ROOT / "server", check=True)


def stats(report):
    out = {"alive": report["alive"], "dead": report["dead"], "arrivals": report["arrivals"]}
    for kind, count in report["events"].items():
        out[f"events.{kind}"] = count
    for kind, count in report["chronicleKinds"].items():
        out[f"chronicle.{kind}"] = count
    chars = report["characters"]
    for key in ("hp", "hunger", "energy", "social", "memories"):
        out[f"mean.{key}"] = sum(c[key] for c in chars) / max(1, len(chars))
    return out


totals = {"ts": defaultdict(float), "rs": defaultdict(float)}
for seed in SEEDS:
    ts = json.loads(subprocess.run(["bun", "tools/soak.ts", str(DAYS), str(seed), "--json"], cwd=ROOT, capture_output=True, text=True, check=True).stdout)
    rs = json.loads(subprocess.run([str(ROOT / "server/target/release/soak"), str(DAYS), str(seed), "--json"], capture_output=True, text=True, check=True).stdout)
    for side, report in (("ts", ts), ("rs", rs)):
        for key, value in stats(report).items():
            totals[side][key] += value / len(SEEDS)
    print(f"seed {seed}: ts {ts['elapsedSeconds']} s, rs {rs['elapsedSeconds']} s, alive {ts['alive']}/{rs['alive']}, dead {ts['dead']}/{rs['dead']}", flush=True)

failures = []
print(f"\n{'statistic':<36}{'ts':>10}{'rs':>10}{'diff':>9}")
for key in sorted(set(totals["ts"]) | set(totals["rs"])):
    a, b = totals["ts"][key], totals["rs"][key]
    diff = (b - a) / a if a else (0.0 if not b else float("inf"))
    gated = max(a, b) >= MIN_GATED or key.startswith("mean.") or key in ("alive",)
    flag = ""
    if gated and abs(diff) > TOLERANCE:
        flag = "  <-- outside ±25%"
        failures.append(key)
    print(f"{key:<36}{a:>10.1f}{b:>10.1f}{diff:>+9.1%}{flag}")
print("\nparity:", "FAIL " + ", ".join(failures) if failures else f"ok (all gated statistics within ±{TOLERANCE:.0%})")
sys.exit(1 if failures else 0)
