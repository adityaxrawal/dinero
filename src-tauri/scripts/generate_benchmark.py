import os
import json
import asyncio
from typing import List, Dict

# Note: This is a placeholder for the benchmark generation script.
# In a real execution, it would use the OpenAI API (`gpt-4o`) to generate synthetic
# statements and emails for 14 Indian banks.

BANKS = [
    "HDFC Bank", "ICICI Bank", "SBI", "Axis Bank", "Kotak Mahindra",
    "IndusInd Bank", "Yes Bank", "PNB", "Bank of Baroda", "IDFC First",
    "Standard Chartered", "Citibank", "HSBC", "RBL Bank"
]

EDGE_CASES = ["refund", "international", "declined"]

async def generate_synthetic_data(bank: str, edge_case: str = None) -> Dict:
    """Simulates an API call to gpt-4o to generate a synthetic statement/email."""
    # This would prompt the LLM to generate a realistic PDF layout (or HTML email)
    # and a corresponding JSON ground truth file.
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
    print(f"Starting synthetic benchmark generation for {len(BANKS)} banks...")
    output_dir = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../../dinero-benchmarks"))
    os.makedirs(output_dir, exist_ok=True)
    
    tasks = []
    # Generate standard statements and edge cases
    for bank in BANKS:
        tasks.append(generate_synthetic_data(bank))
        for edge_case in EDGE_CASES:
            tasks.append(generate_synthetic_data(bank, edge_case))
            
    results = await asyncio.gather(*tasks)
    
    # Save the synthetic corpus
    for idx, result in enumerate(results):
        base_name = f"synth_{result['bank'].replace(' ', '_').lower()}_{idx}"
        
        # Save content (simulating PDF/HTML output)
        content_ext = "html" if result["type"] == "email" else "txt"
        with open(os.path.join(output_dir, f"{base_name}.{content_ext}"), "w") as f:
            f.write(result["content"])
            
        # Save ground truth
        with open(os.path.join(output_dir, f"{base_name}_truth.json"), "w") as f:
            json.dump(result["ground_truth"], f, indent=2)

    print(f"Generated {len(results)} items in {output_dir}")
    print("Please use Git LFS to track the generated PDF/HTML files in the dinero-benchmarks repository.")

if __name__ == "__main__":
    asyncio.run(main())
