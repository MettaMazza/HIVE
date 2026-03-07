#!/usr/bin/env python3
"""
HIVE Teacher Module — Smoke Test
Validates a newly trained model before hot-swapping it live.
Runs 5 known-good prompts and checks for common failure modes.

Usage:
    python3 training/smoke_test.py apis-v3-20260307
"""

import json
import sys
import requests

OLLAMA_ENDPOINT = "http://localhost:11434/api/chat"

# Known-good prompts that should produce clean, conversational responses
SMOKE_PROMPTS = [
    "Good afternoon, please introduce yourself.",
    "What tools do you have access to?",
    "Hello, how are you?",
    "Can you tell me a bit about what you can do?",
    "What is 2 + 2?",
]

# Patterns that indicate a FAILED response
FAILURE_PATTERNS = [
    "<system_codebase_read>",
    "<native_codebase_list>",
    "native_codebase_read",
    "```json\n{",
    "\"tasks\":",
    "\"drone_type\":",
    "I'm just an AI",
    "As an AI language model",
    "I cannot help",
    "tokio::spawn",
    "5-Tier Memory",
    "async workers",
]


def test_prompt(model: str, prompt: str) -> tuple:
    """Send a prompt to the model and check the response. Returns (passed, response)."""
    try:
        payload = {
            "model": model,
            "messages": [
                {"role": "system", "content": "You are Apis, a helpful AI assistant."},
                {"role": "user", "content": prompt},
            ],
            "stream": False,
        }
        resp = requests.post(OLLAMA_ENDPOINT, json=payload, timeout=60)
        resp.raise_for_status()

        data = resp.json()
        content = data.get("message", {}).get("content", "")

        if not content or len(content.strip()) < 10:
            return False, f"[EMPTY RESPONSE] '{content}'"

        for pattern in FAILURE_PATTERNS:
            if pattern.lower() in content.lower():
                return False, f"[FAILURE PATTERN: '{pattern}'] {content[:200]}"

        return True, content[:200]

    except Exception as e:
        return False, f"[REQUEST ERROR] {e}"


def main():
    if len(sys.argv) < 2:
        print("Usage: python3 training/smoke_test.py <model_name>")
        sys.exit(1)

    model = sys.argv[1]
    print(f"[SMOKE TEST] Testing model: {model}")
    print("=" * 60)

    passed = 0
    failed = 0

    for prompt in SMOKE_PROMPTS:
        ok, response = test_prompt(model, prompt)
        status = "✅ PASS" if ok else "❌ FAIL"
        print(f"\n{status}: \"{prompt}\"")
        print(f"  Response: {response}")

        if ok:
            passed += 1
        else:
            failed += 1

    print("\n" + "=" * 60)
    print(f"[SMOKE TEST] Results: {passed}/{len(SMOKE_PROMPTS)} passed")

    if failed > 0:
        print(f"[SMOKE TEST] ❌ FAILED — {failed} prompts produced bad responses")
        print("[SMOKE TEST] Model will NOT be hot-swapped.")
        sys.exit(1)
    else:
        print("[SMOKE TEST] ✅ ALL PASSED — Safe to hot-swap.")
        sys.exit(0)


if __name__ == "__main__":
    main()
