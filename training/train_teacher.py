#!/usr/bin/env python3
"""
HIVE Teacher Module — MLX Training Pipeline
Runs mixed ORPO + SFT micro-training on golden examples and preference pairs.
Designed for Apple Silicon (M3 Ultra) via mlx-lm-lora.

Usage:
    python3 training/train_teacher.py             # Full training
    python3 training/train_teacher.py --dry-run    # Parse validation only
    python3 training/train_teacher.py --micro      # Micro sleep training (1-2 examples)
    python3 training/train_teacher.py --micro --stack  # Stack on previous adapter
"""

import argparse
import json
import os
import sys
import shutil
from datetime import datetime, timedelta
from pathlib import Path
from collections import Counter

# ─── Configuration ───────────────────────────────────────────────────────────

BASE_MODEL = "mlx-community/Qwen3.5-35B-A3B-4bit"
MAX_SEQ_LEN = 16384
LORA_R = 8
LORA_ALPHA = 8
LEARNING_RATE = 2e-5
NUM_EPOCHS = 2
ORPO_BETA = 0.1
QUANTIZATION = "q8_0"
MAX_GOLDEN = 50        # Cap golden examples per session
MAX_PAIRS = 30         # Cap preference pairs per session

TEACHER_DIR = Path("./memory/teacher")
GOLDEN_PATH = TEACHER_DIR / "golden_buffer.jsonl"
PREFERENCE_PATH = TEACHER_DIR / "preference_buffer.jsonl"
ARCHIVE_DIR = TEACHER_DIR / "archive"
MANIFEST_PATH = TEACHER_DIR / "manifest.json"
OUTPUT_DIR = Path("./training/output")

# ─── Data Loading ────────────────────────────────────────────────────────────

def load_jsonl(path: Path, max_items: int = None) -> list:
    """Load JSONL file, return list of dicts."""
    if not path.exists():
        return []
    items = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line:
                try:
                    items.append(json.loads(line))
                except json.JSONDecodeError:
                    continue
    if max_items:
        items = items[-max_items:]  # Take most recent
    return items


def load_manifest() -> dict:
    """Load or create manifest."""
    if MANIFEST_PATH.exists():
        with open(MANIFEST_PATH) as f:
            return json.load(f)
    return {
        "current": BASE_MODEL,
        "base": BASE_MODEL,
        "history": [],
        "retention": 5
    }


def save_manifest(manifest: dict):
    """Save manifest to disk."""
    with open(MANIFEST_PATH, "w") as f:
        json.dump(manifest, f, indent=2)

# ─── Data Formatting ─────────────────────────────────────────────────────────

def format_golden_for_sft(examples: list) -> list:
    """Convert golden examples to Qwen3.5 chat format for SFT."""
    formatted = []
    for ex in examples:
        system_content = strip_system_prompt(ex.get("system_prompt", ""))
        swarm_ctx = ex.get("swarm_ctx", "")
        user_msg = ex.get("user_msg", "")
        if swarm_ctx:
            user_msg += f"\n\n[INTERNAL EXECUTION LOOP]\n{swarm_ctx}"

        formatted.append({
            "messages": [
                {"role": "system", "content": system_content},
                {"role": "user", "content": user_msg},
                {"role": "assistant", "content": ex["response"]},
            ]
        })
    return formatted


def strip_system_prompt(prompt: str, max_chars: int = 2000) -> str:
    """Extract the identity/persona section from the full kernel prompt.
    
    The full prompt is ~110K chars: HUD + kernel laws + tool defs + identity.
    For training, we only want the identity block so the model learns
    HOW Apis communicates, not the system architecture.
    
    Extracts from 'You are Apis' to the next '###' heading.
    """
    # Find the identity section
    start = prompt.find("You are Apis")
    if start == -1:
        # Fallback: try other identity markers
        for marker in ["# Identity", "## Identity", "# Persona"]:
            start = prompt.find(marker)
            if start != -1:
                break
    
    if start == -1:
        return "You are Apis, the intelligent core of the HIVE Engine."
    
    # Find the end: next section header after identity starts
    remainder = prompt[start:]
    end_markers = ["### Self-Supervised", "### Capabilities and Limits",
                   "### Available Tools", "## Tool Definitions"]
    
    end = len(remainder)
    for marker in end_markers:
        idx = remainder.find(marker)
        if idx > 0 and idx < end:
            end = idx
    
    identity = remainder[:end].rstrip()
    
    # Hard cap
    if len(identity) > max_chars:
        identity = identity[:max_chars].rstrip()
    
    return identity


def format_pairs_for_orpo(pairs: list) -> list:
    """Convert preference pairs to ORPO format (chosen/rejected)."""
    formatted = []
    for pair in pairs:
        formatted.append({
            "prompt": pair["prompt"],
            "chosen": pair["chosen"],
            "rejected": pair["rejected"],
        })
    return formatted


def balance_by_category(pairs: list, max_per_category: int = 10) -> list:
    """Resample preference pairs to ensure diversity across failure categories."""
    by_category = {}
    for pair in pairs:
        cat = pair.get("failure_category", "unknown")
        if cat not in by_category:
            by_category[cat] = []
        by_category[cat].append(pair)

    balanced = []
    for cat, items in by_category.items():
        balanced.extend(items[:max_per_category])

    return balanced

# ─── Archive ─────────────────────────────────────────────────────────────────

def archive_processed(golden_count: int, pair_count: int):
    """Move processed buffer files to archive with timestamp."""
    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    ARCHIVE_DIR.mkdir(parents=True, exist_ok=True)

    if GOLDEN_PATH.exists() and golden_count > 0:
        shutil.move(str(GOLDEN_PATH), str(ARCHIVE_DIR / f"golden_{ts}.jsonl"))

    if PREFERENCE_PATH.exists() and pair_count > 0:
        shutil.move(str(PREFERENCE_PATH), str(ARCHIVE_DIR / f"preference_{ts}.jsonl"))


def get_next_version(manifest: dict) -> str:
    """Generate next version string."""
    version_num = len(manifest.get("history", [])) + 1
    date_str = datetime.now().strftime("%Y%m%d")
    return f"apis-v{version_num}-{date_str}"

# ─── Main Training Entry ─────────────────────────────────────────────────────

def parse_args():
    parser = argparse.ArgumentParser(description="HIVE Teacher Training Pipeline")
    parser.add_argument("--dry-run", action="store_true", help="Parse validation only")
    parser.add_argument("--micro", action="store_true", help="Micro sleep training (1-2 examples, 1 epoch)")
    parser.add_argument("--stack", action="store_true", help="Train on previous adapter instead of base model")
    parser.add_argument("--examples", type=int, default=None, help="Max examples for micro mode")
    parser.add_argument("--lr", type=float, default=None, help="Override learning rate")
    parser.add_argument("--epochs", type=int, default=None, help="Override epoch count")
    parser.add_argument("--max-seq-len", type=int, default=None, help="Override max sequence length")
    return parser.parse_args()


def main():
    args = parse_args()
    dry_run = args.dry_run

    # Apply micro-mode overrides
    global LEARNING_RATE, NUM_EPOCHS, MAX_SEQ_LEN, MAX_GOLDEN, MAX_PAIRS
    if args.micro:
        LEARNING_RATE = args.lr or 1e-5
        NUM_EPOCHS = args.epochs or 1
        MAX_SEQ_LEN = args.max_seq_len or 8192
        MAX_GOLDEN = args.examples or 2
        MAX_PAIRS = args.examples or 2
    else:
        if args.lr:
            LEARNING_RATE = args.lr
        if args.epochs:
            NUM_EPOCHS = args.epochs
        if args.max_seq_len:
            MAX_SEQ_LEN = args.max_seq_len

    mode_label = "MICRO SLEEP" if args.micro else ("DRY RUN" if dry_run else "FULL TRAINING")

    print("=" * 60)
    print("[TEACHER] HIVE Self-Supervised Training Pipeline")
    print(f"[TEACHER] Mode: {mode_label}")
    if args.micro:
        print(f"[TEACHER] Micro config: lr={LEARNING_RATE}, epochs={NUM_EPOCHS}, max_examples={MAX_GOLDEN}, seq_len={MAX_SEQ_LEN}")
        if args.stack:
            print(f"[TEACHER] Stacking on previous adapter (cumulative)")
    print("=" * 60)

    # 1. Load data
    golden = load_jsonl(GOLDEN_PATH, MAX_GOLDEN)
    pairs = load_jsonl(PREFERENCE_PATH, MAX_PAIRS)

    print(f"[TEACHER] Golden examples: {len(golden)}")
    print(f"[TEACHER] Preference pairs: {len(pairs)}")

    if len(golden) == 0 and len(pairs) == 0:
        print("[TEACHER] No training data available. Exiting.")
        return

    # 2. Category distribution
    if pairs:
        categories = Counter(p.get("failure_category", "unknown") for p in pairs)
        print(f"[TEACHER] Failure categories: {dict(categories)}")
        pairs = balance_by_category(pairs)
        print(f"[TEACHER] After balancing: {len(pairs)} pairs")

    # 3. Format data
    sft_data = format_golden_for_sft(golden)
    orpo_data = format_pairs_for_orpo(pairs)

    print(f"[TEACHER] SFT examples: {len(sft_data)}")
    print(f"[TEACHER] ORPO pairs: {len(orpo_data)}")

    if dry_run:
        print("[TEACHER] Dry run complete. Data parsed successfully.")
        if sft_data:
            print(f"[TEACHER] Sample SFT: {json.dumps(sft_data[0], indent=2)[:500]}")
        if orpo_data:
            print(f"[TEACHER] Sample ORPO: {json.dumps(orpo_data[0], indent=2)[:500]}")
        return

    # 4. Write training datasets
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    sft_path = OUTPUT_DIR / "train.jsonl"
    orpo_path = OUTPUT_DIR / "orpo_train.jsonl"

    with open(sft_path, "w") as f:
        for item in sft_data:
            f.write(json.dumps(item) + "\n")

    with open(orpo_path, "w") as f:
        for item in orpo_data:
            f.write(json.dumps(item) + "\n")

    print(f"[TEACHER] Datasets written to {OUTPUT_DIR}")

    # 5. Run MLX LoRA training
    manifest = load_manifest()
    new_version = get_next_version(manifest)
    parent = manifest["current"]

    # In stack mode, use the current (latest trained) model, not the base
    train_from = parent if args.stack else manifest.get("base", BASE_MODEL)

    print(f"[TEACHER] Training {new_version} (from: {train_from}, parent: {parent})")
    print(f"[TEACHER] Config: lr={LEARNING_RATE}, epochs={NUM_EPOCHS}, r={LORA_R}, seq_len={MAX_SEQ_LEN}")

    # MLX LoRA SFT training
    if sft_data:
        sft_cmd = (
            f"python3.12 -m mlx_lm.lora "
            f"--model {train_from} "
            f"--data {OUTPUT_DIR} "
            f"--train "
            f"--num-layers {LORA_R} "
            f"--learning-rate {LEARNING_RATE} "
            f"--iters {NUM_EPOCHS} "
            f"--batch-size 1 "
            f"--max-seq-length {MAX_SEQ_LEN} "
            f"--adapter-path {OUTPUT_DIR}/adapters"
        )
        print(f"[TEACHER] Running: {sft_cmd}")
        exit_code = os.system(sft_cmd)
        if exit_code != 0:
            print(f"[TEACHER] SFT training failed with exit code {exit_code}")
            sys.exit(1)

    # 6. Version the adapter for cumulative stacking
    # Qwen 3.5 MoE (hybrid SSM/attention) can't be converted to GGUF via any
    # available tool. Instead, we version adapters and use --resume-adapter-file
    # for cumulative stacking. The Ollama base model stays the same, while
    # the adapters compound over sleep cycles.
    versioned_adapter_dir = OUTPUT_DIR / "adapters" / new_version
    versioned_adapter_dir.mkdir(parents=True, exist_ok=True)
    
    # Copy current adapter to versioned location
    import shutil
    for f_name in ["adapters.safetensors", "adapter_config.json"]:
        src = OUTPUT_DIR / "adapters" / f_name
        if src.exists():
            shutil.copy2(str(src), str(versioned_adapter_dir / f_name))
    
    print(f"[TEACHER] 💾 Adapter saved: {versioned_adapter_dir}")

    # 7. Update manifest (track adapter path, not Ollama model)
    manifest["history"].append({
        "version": new_version,
        "date": datetime.now().isoformat(),
        "golden_count": len(golden),
        "pair_count": len(pairs),
        "parent": parent,
        "adapter_path": str(versioned_adapter_dir),
    })
    manifest["current"] = new_version
    manifest["latest_adapter"] = str(OUTPUT_DIR / "adapters")
    save_manifest(manifest)

    # 8. Archive processed data
    archive_processed(len(golden), len(pairs))

    # 9. Cleanup old adapters (keep last N)
    retention = manifest.get("retention", 5)
    if len(manifest["history"]) > retention:
        old_versions = manifest["history"][:-retention]
        for old in old_versions:
            old_adapter = Path(old.get("adapter_path", ""))
            if old_adapter.exists():
                shutil.rmtree(str(old_adapter), ignore_errors=True)
                print(f"[TEACHER] Pruned old adapter: {old_adapter}")
        manifest["history"] = manifest["history"][-retention:]
        save_manifest(manifest)

    print(f"[TEACHER] ✅ Training complete: {new_version}")
    print(f"[TEACHER] Golden: {len(golden)} | Pairs: {len(pairs)}")


if __name__ == "__main__":
    main()
