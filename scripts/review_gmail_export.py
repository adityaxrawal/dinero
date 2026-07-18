#!/usr/bin/env python3
"""Hand-label ground truth against the classify binary's predictions.

Usage:
    review.py mark <email_id> --correct-label {transaction,non_transaction,statement} [--note "..."]
    review.py report
"""
import argparse
import json
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
OUTPUT_DIR = REPO_ROOT / "gmail_export" / "segregated_emails"
REVIEW_LOG = REPO_ROOT / "gmail_export" / "review_log.jsonl"
LABELS = ["transaction", "non_transaction", "statement"]


def find_segregated_record(email_id: str) -> tuple[str, dict] | tuple[None, None]:
    for label in LABELS:
        path = OUTPUT_DIR / label / f"{email_id}.json"
        if path.exists():
            return label, json.loads(path.read_text())
    return None, None


def cmd_mark(args: argparse.Namespace) -> None:
    label, record = find_segregated_record(args.email_id)
    if record is None:
        sys.exit(
            f"email_id {args.email_id!r} not found in any of "
            f"{[str(OUTPUT_DIR / l) for l in LABELS]}"
        )
    entry = {
        "email_id": args.email_id,
        "predicted_label": record["pipeline"]["predicted_label"],
        "correct_label": args.correct_label,
        "note": args.note,
        "marked_at": datetime.now(timezone.utc).isoformat(),
    }
    with REVIEW_LOG.open("a", encoding="utf-8") as f:
        f.write(json.dumps(entry, ensure_ascii=False) + "\n")
    print(f"Marked {args.email_id}: predicted={entry['predicted_label']} correct={args.correct_label}")


def cmd_report(_args: argparse.Namespace) -> None:
    if not REVIEW_LOG.exists():
        print("No review_log.jsonl yet — run `review.py mark` first.")
        return

    entries = [
        json.loads(line)
        for line in REVIEW_LOG.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]

    matrix: dict[str, dict[str, int]] = defaultdict(lambda: defaultdict(int))
    mismatches_by_reason: dict[str, list[dict]] = defaultdict(list)

    for e in entries:
        matrix[e["predicted_label"]][e["correct_label"]] += 1
        if e["predicted_label"] != e["correct_label"]:
            _, record = find_segregated_record(e["email_id"])
            pipeline = record["pipeline"] if record else {}
            reason = pipeline.get("rejection_reason") or pipeline.get("gate2_result") or "unknown"
            mismatches_by_reason[reason].append(e)

    print("Confusion matrix (rows = predicted, columns = correct):")
    all_labels = sorted({*matrix.keys(), *(k for v in matrix.values() for k in v)})
    header = "predicted \\ correct".ljust(20) + "".join(l.ljust(18) for l in all_labels)
    print(header)
    for predicted in all_labels:
        row = predicted.ljust(20)
        for correct in all_labels:
            row += str(matrix[predicted][correct]).ljust(18)
        print(row)

    print(f"\nTotal marked: {len(entries)}")
    total_mismatches = sum(len(v) for v in mismatches_by_reason.values())
    print(f"Total mismatches: {total_mismatches}\n")

    if mismatches_by_reason:
        print("Mismatches grouped by rejection_reason/gate2_result:")
        for reason, items in sorted(mismatches_by_reason.items(), key=lambda kv: -len(kv[1])):
            print(f"  {reason}: {len(items)}")
            for item in items:
                print(
                    f"    {item['email_id']}: predicted={item['predicted_label']} "
                    f"correct={item['correct_label']} note={item.get('note')!r}"
                )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    mark_parser = sub.add_parser("mark", help="Record ground truth for one email")
    mark_parser.add_argument("email_id")
    mark_parser.add_argument("--correct-label", required=True, choices=LABELS)
    mark_parser.add_argument("--note", default=None)
    mark_parser.set_defaults(func=cmd_mark)

    report_parser = sub.add_parser("report", help="Print confusion matrix + mismatch breakdown")
    report_parser.set_defaults(func=cmd_report)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
