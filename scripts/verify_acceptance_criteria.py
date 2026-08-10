#!/usr/bin/env python3
"""CI gate that ties each high-impact product risk to the test proving it is controlled.

RISK_REGISTER below is the single source of truth: every entry names a risk, the
control that mitigates it, and the exact test command that demonstrates the
control still works. This script runs only those mapped commands rather than the
whole test suite, so it stays fast enough to gate a merge.

A risk is reported as failing in three distinct situations, all treated as
equally serious: the test command exits non-zero, the command cannot be found or
its working directory is missing, or the command runs successfully but matches
zero tests. That last case is the subtle one -- a renamed or deleted test would
otherwise leave a filter silently matching nothing and reporting green, which
would mean an uncovered risk masquerading as a covered one.

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

# The risk-to-test mapping this whole gate operates on. Adding a high-impact
# risk means adding an entry here together with the test that proves its control
# holds; entries may carry "optional": True when their tooling is not guaranteed
# to exist in every checkout.
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
                "optional": True,
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
    """Execute one mapped test command and report whether its risk stays covered.

    Returns a (passed, detail) pair where detail is a human-readable explanation
    used verbatim in the failure output. Entries marked "optional" degrade to a
    skip instead of a failure when their tooling or directory is absent, which
    lets the gate run in partial checkouts.
    """
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
        # Only the tail is surfaced: full cargo output is far too long to be
        # useful in a CI summary, and the failure itself is always at the end.
        tail = "\n".join(output.splitlines()[-20:])
        return False, f"exit code {result.returncode}:\n{tail}"

    # A clean exit is not sufficient. A test filter that matches nothing also
    # exits zero, so parse the reported pass counts and treat "ran zero tests"
    # as a failure -- otherwise a renamed test would silently drop coverage.
    passed_counts = [int(n) for n in re.findall(r"(\d+) passed", output)]
    if passed_counts and sum(passed_counts) == 0:
        return False, f"matched zero tests (filter is stale or the test was removed):\n{output.strip()[-500:]}"
    return True, "passed"

def main() -> int:
    """Run every mapped test and return a shell exit code: 0 all covered, 1 otherwise.

    Supports three output modes -- a printed human summary by default, machine
    readable JSON via --json, and a written JSON file via --output, which the
    in-app Release Readiness view reads to display go/no-go status.
    """
    parser = argparse.ArgumentParser()
    parser.add_argument("--list", action="store_true", help="print the risk-to-test mapping and exit without running anything")
    parser.add_argument("--json", action="store_true", help="print machine-readable JSON results instead of a human summary")
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        help="also write the JSON results to this path; the in-app Release "
        "Readiness debug view reads this file, if present, for go/no-go status",
    )
    args = parser.parse_args()

    # --list is a dry run: print the mapping so a reviewer can audit which test
    # backs which risk, without paying for a full test execution.
    if args.list:
        for entry in RISK_REGISTER:
            print(f"- {entry['risk']} ({entry['tracked_at']})")
            for test in entry["tests"]:
                print(f"    $ {' '.join(str(c) for c in test['cmd'])}  (in {test['cwd']})")
        return 0

    # Every risk is evaluated even after one fails, so a single run reports the
    # complete picture rather than stopping at the first problem.
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

    # The file is written regardless of --json so the readiness view can be fed
    # from the same run that prints the human summary.
    output_payload = {"results": results, "all_passed": not any_failed}
    if args.output:
        args.output.write_text(json.dumps(output_payload, indent=2))

    if args.json:
        print(json.dumps(output_payload, indent=2))
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
