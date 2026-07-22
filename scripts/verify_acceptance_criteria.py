#!/usr/bin/env python3
"""Doc 30 TASK-QA-010: CI Gate for Acceptance Criteria and Risk Register.

Maps each of Document 31 §17's high-impact risk-register items to the real
test(s) that cover it, and runs exactly those tests (not the full nightly
suite -- this is meant to be fast enough to gate a merge). Fails loudly,
with the risk description in the failure message, if a mapped test doesn't
exist or doesn't pass -- a release candidate must never ship with a
regression in a documented high-impact risk silently unnoticed.

Usage:
    python3 scripts/verify_acceptance_criteria.py
    python3 scripts/verify_acceptance_criteria.py --list   # show the mapping, run nothing
"""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SRC_TAURI = REPO_ROOT / "src-tauri"

# Document 31 §17's risk register, each mapped to the real test(s) that
# currently cover it. `cmd` is run from `src-tauri/` unless `cwd` overrides
# it. Kept intentionally narrow (targeted `cargo test`/`npx vitest` filters,
# not the full suite) so this stays fast enough to gate a merge -- the full
# nightly suite (including the ~168s Gmail-quota test) is `benchmark.yml`'s
# job, not this one.
RISK_REGISTER = [
    {
        "risk": "False merges inflate analytics",
        "control": "Keep ambiguity unresolved until user or rules resolve it.",
        "tracked_at": "Document 12 §8.2a; TASK-DEDUP-005/010",
        "tests": [
            {
                "cmd": ["cargo", "test", "--test", "reconciliation_regression"],
                "cwd": SRC_TAURI,
            },
        ],
    },
    {
        "risk": "PDF leakage to disk",
        "control": "Process in memory only, discard bytes immediately.",
        "tracked_at": "Document 26 §7.1-7.2; TASK-STMT-008",
        "tests": [
            {
                "cmd": ["cargo", "test", "--lib", "test_raw_pdf_not_persisted_after_parse"],
                "cwd": SRC_TAURI,
            },
        ],
    },
    {
        "risk": "Licensing Backend overreach",
        "control": "Enforce a narrow API and data schema.",
        "tracked_at": "Document 17; TASK-LIC-010",
        "tests": [
            {
                "cmd": ["cargo", "test", "--test", "data_isolation_suite"],
                "cwd": SRC_TAURI,
            },
            {
                "cmd": ["npx", "vitest", "run", "tests/data_isolation.test.ts"],
                "cwd": REPO_ROOT / "licensing-backend",
                "optional": True,  # only present once licensing-backend is checked out with its own deps installed
            },
        ],
    },
    {
        "risk": "Cloud LLM use in production",
        "control": "Prohibit third-party cloud LLMs by default.",
        "tracked_at": "Document 15 Core Principle; TASK-TXN-006",
        "tests": [
            {
                "cmd": ["cargo", "test", "--test", "data_isolation_suite", "test_no_disallowed_cloud_llm_dependencies"],
                "cwd": SRC_TAURI,
            },
        ],
    },
    {
        "risk": "Duplicate Gmail scans",
        "control": "Checkpoint and deduplicate at source and canonical layers.",
        "tracked_at": "TASK-GMAIL-007/009; TASK-TXN-009",
        "tests": [
            {
                "cmd": ["cargo", "test", "--lib", "test_history_delta_polling_idempotent"],
                "cwd": SRC_TAURI,
            },
        ],
    },
    {
        "risk": "Support/debug data exposure",
        "control": "Redact and minimize telemetry.",
        "tracked_at": "Document 06 §5; TASK-AUTH-015; TASK-OPS-004/007",
        "tests": [
            {
                "cmd": ["cargo", "test", "--lib", "test_gmail_telemetry_snapshot_contains_no_free_form_content"],
                "cwd": SRC_TAURI,
            },
        ],
    },
]


def run_test(test: dict) -> tuple[bool, str]:
    cwd = test["cwd"]
    if not cwd.exists():
        if test.get("optional"):
            return True, f"skipped (optional, {cwd} not present in this checkout)"
        return False, f"working directory {cwd} does not exist"
    try:
        result = subprocess.run(
            test["cmd"],
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=600,
        )
    except FileNotFoundError as e:
        if test.get("optional"):
            return True, f"skipped (optional, command not available: {e})"
        return False, f"command not found: {e}"
    output = result.stdout + result.stderr
    if result.returncode != 0:
        tail = "\n".join(output.splitlines()[-20:])
        return False, f"exit code {result.returncode}:\n{tail}"
    # `cargo test`/`vitest` both exit 0 for "zero tests matched the filter"
    # just as readily as for "all matched tests passed" -- a typo'd or
    # renamed test name would otherwise silently report PASS here, exactly
    # the failure mode this script exists to prevent ("at least one test
    # exists per high-impact risk" requires a test to actually have run).
    passed_counts = [int(n) for n in re.findall(r"(\d+) passed", output)]
    if passed_counts and sum(passed_counts) == 0:
        return False, f"matched zero tests (filter is stale or the test was removed):\n{output.strip()[-500:]}"
    return True, "passed"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--list", action="store_true", help="print the risk-to-test mapping and exit without running anything")
    parser.add_argument("--json", action="store_true", help="print machine-readable JSON results instead of a human summary")
    args = parser.parse_args()

    if args.list:
        for entry in RISK_REGISTER:
            print(f"- {entry['risk']} ({entry['tracked_at']})")
            for test in entry["tests"]:
                print(f"    $ {' '.join(str(c) for c in test['cmd'])}  (in {test['cwd']})")
        return 0

    results = []
    any_failed = False
    for entry in RISK_REGISTER:
        entry_passed = True
        details = []
        for test in entry["tests"]:
            passed, detail = run_test(test)
            entry_passed = entry_passed and passed
            details.append({"cmd": " ".join(str(c) for c in test["cmd"]), "passed": passed, "detail": detail})
        results.append({
            "risk": entry["risk"],
            "control": entry["control"],
            "tracked_at": entry["tracked_at"],
            "passed": entry_passed,
            "tests": details,
        })
        any_failed = any_failed or not entry_passed

    if args.json:
        print(json.dumps({"results": results, "all_passed": not any_failed}, indent=2))
    else:
        for r in results:
            status = "PASS" if r["passed"] else "FAIL"
            print(f"[{status}] {r['risk']}")
            print(f"       control: {r['control']}")
            print(f"       tracked at: {r['tracked_at']}")
            if not r["passed"]:
                for t in r["tests"]:
                    if not t["passed"]:
                        print(f"       FAILED: {t['cmd']}")
                        print(f"         {t['detail']}")
        print()
        print("All risk-register items covered and passing." if not any_failed else "One or more high-impact risk-register items failed or have no passing coverage -- see above.")

    return 1 if any_failed else 0


if __name__ == "__main__":
    sys.exit(main())
