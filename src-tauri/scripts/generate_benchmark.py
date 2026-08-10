"""Generates the synthetic benchmark corpus used to exercise transaction extraction.

Produces one standard statement plus one email per edge case for every bank in
BANKS, writing each item as a pair of files into a sibling dinero-benchmarks
directory: the document itself (.txt for statements, .html for emails) and a
matching _truth.json holding the values a correct extraction should recover.
That pairing is what lets the extraction tests score themselves automatically.

The corpus is entirely synthetic by design -- real bank statements carry
personal financial data and cannot live in a benchmark repository.
"""
import os
import json
import asyncio
from typing import List, Dict

# Indian retail banks the extraction pipeline is expected to handle. Each one
# contributes a standard statement plus one email per entry in EDGE_CASES.
BANKS = [
    "HDFC Bank", "ICICI Bank", "SBI", "Axis Bank", "Kotak Mahindra",
    "IndusInd Bank", "Yes Bank", "PNB", "Bank of Baroda", "IDFC First",
    "Standard Chartered", "Citibank", "HSBC", "RBL Bank"
]

# Transaction shapes that historically break naive parsers: a negative-direction
# refund, a foreign-currency charge, and a failed authorisation that must not be
# recorded as spend.
EDGE_CASES = ["refund", "international", "declined"]

async def generate_synthetic_data(bank: str, edge_case: str = None) -> Dict:
    """Build one synthetic document plus the ground truth an extractor should recover.

    Currently returns a fixed placeholder payload; this is the seam where a real
    generator (an LLM call, or a template renderer) would be substituted. Passing
    an edge_case switches the output from a statement to an email, since those
    scenarios arrive as bank alerts rather than in a periodic statement.
    """
    return {
        "bank": bank,
        "type": "email" if edge_case else "statement",
        "edge_case": edge_case,
        "content": f"Synthetic content for {bank} ({edge_case or 'standard'})",
        "ground_truth": {
            "merchant": "Test Merchant",
            "amount": 1000.0,
            "currency": "INR",
            "date": "2026-07-05T12:00:00Z"
        }
    }

async def main():
    """Generate the full corpus and write it out as document/ground-truth file pairs."""
    print(f"Starting synthetic benchmark generation for {len(BANKS)} banks...")

    # The corpus lives outside this repository, in a sibling checkout, so the
    # large binary fixtures never bloat the application's own git history.
    output_dir = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../../dinero-benchmarks"))
    os.makedirs(output_dir, exist_ok=True)

    # Queue every bank/edge-case combination first, then await them together --
    # generation is I/O bound, so running it concurrently is markedly faster
    # than a sequential loop.
    tasks = []
    for bank in BANKS:
        tasks.append(generate_synthetic_data(bank))
        for edge_case in EDGE_CASES:
            tasks.append(generate_synthetic_data(bank, edge_case))

    results = await asyncio.gather(*tasks)

    for idx, result in enumerate(results):
        # The index keeps names unique when one bank yields several documents.
        base_name = f"synth_{result['bank'].replace(' ', '_').lower()}_{idx}"

        # Emails are written as HTML because that is the form the ingestion
        # pipeline receives them in; statements are plain text.
        content_ext = "html" if result["type"] == "email" else "txt"
        with open(os.path.join(output_dir, f"{base_name}.{content_ext}"), "w") as f:
            f.write(result["content"])
            
        # The _truth.json sidecar is what the extraction benchmark scores
        # against; its name must stay derivable from the document's own name.
        with open(os.path.join(output_dir, f"{base_name}_truth.json"), "w") as f:
            json.dump(result["ground_truth"], f, indent=2)

    print(f"Generated {len(results)} items in {output_dir}")
    print("Please use Git LFS to track the generated PDF/HTML files in the dinero-benchmarks repository.")

if __name__ == "__main__":
    asyncio.run(main())
