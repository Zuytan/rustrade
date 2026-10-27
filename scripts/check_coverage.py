#!/usr/bin/env python3
"""Enforce line-coverage minimums on safety-critical modules.

Usage: python3 scripts/check_coverage.py coverage/tarpaulin-report.json

Reads a cargo-tarpaulin JSON report and fails (exit 1) when a listed module falls
below its minimum. The UI (src/interfaces) and binaries (src/bin) are not gated.

Minimums sit a few points below the coverage measured when they were introduced
(0.99.32): raise them as coverage improves, never lower them to make a change pass.
"""

import json
import sys
from collections import defaultdict

# Path prefix (relative to the repository root) -> minimum line coverage in percent.
MINIMUMS = {
    "src/domain": 80.0,
    "src/domain/risk": 80.0,
    "src/domain/trading": 65.0,
    "src/application/risk_management": 72.0,
    "src/application/agents": 58.0,
    "src/application/agents/executor.rs": 80.0,
    "src/config": 78.0,
}


def relative_path(parts):
    path = "/".join(parts)
    # Last occurrence: the checkout itself may live under a directory named "src".
    index = path.rfind("/src/")
    return path[index + 1 :] if index >= 0 else path


def main(report_path):
    with open(report_path, encoding="utf-8") as f:
        report = json.load(f)

    totals = defaultdict(lambda: [0, 0])
    for file in report["files"]:
        rel = relative_path(file["path"])
        for prefix in MINIMUMS:
            if rel == prefix or rel.startswith(prefix + "/"):
                totals[prefix][0] += file["covered"]
                totals[prefix][1] += file["coverable"]

    failures = []
    for prefix, minimum in sorted(MINIMUMS.items()):
        covered, coverable = totals[prefix]
        if coverable == 0:
            failures.append(f"{prefix}: no coverable lines found (path renamed?)")
            continue
        pct = 100.0 * covered / coverable
        status = "ok" if pct >= minimum else "FAIL"
        print(f"{status:4} {pct:6.1f}% (min {minimum:5.1f}%)  {covered}/{coverable}  {prefix}")
        if pct < minimum:
            failures.append(f"{prefix}: {pct:.1f}% < {minimum:.1f}%")

    overall = 100.0 * report["covered"] / report["coverable"] if report["coverable"] else 0.0
    print(f"     {overall:6.1f}% overall (not gated)")

    if failures:
        print("\nCoverage minimums not met:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    sys.exit(main(sys.argv[1]))
