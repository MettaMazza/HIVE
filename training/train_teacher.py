#!/usr/bin/env python3
"""
HIVE Teacher Module — Multi-Backend Training Pipeline
Runs mixed ORPO + SFT micro-training on golden examples and preference pairs.

Backends:
  - MLX (default on macOS/Apple Silicon) via mlx-lm-lora
  - Torch (Linux/Windows/fallback) via HuggingFace Transformers + PEFT

Usage:
    python3 training/train_teacher.py                        # Auto-detect backend
    python3 training/train_teacher.py --dry-run               # Parse validation only
    python3 training/train_teacher.py --micro --stack          # Micro sleep training
    python3 training/train_teacher.py --backend torch          # Force torch backend
    python3 training/train_teacher.py --backend mlx            # Force MLX backend
"""

import argparse
import json
import os
import sys
import shutil
from datetime import datetime
from pathlib import Path
from collections import Counter

# ─── Configuration ───────────────────────────────────────────────────────────

# Training uses the 122B model as the teacher — the bigger brain trains the smaller.
# The 35B runs inference via Ollama. The 122B runs training via MLX LoRA.
MLX_BASE_MODEL = "mlx-community/Qwen3.5-122B-A10B-4bit"
TORCH_BASE_MODEL = "Qwen/Qwen3.5-35B-A3B"  # Torch fallback (Linux/Windows)
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

# ─── Backend Detection ───────────────────────────────────────────────────────

def detect_backend(requested: str = "auto") -> str:
    """Auto-detect training backend: mlx (macOS default) > torch (Linux/Windows).
    
    Returns: 'mlx' or 'torch'
    """
    backend = os.environ.get("HIVE_TRAINING_BACKEND", requested)
    if backend in ("mlx", "torch"):
        print(f"[TEACHER] Backend forced: {backend}")
        return backend

    # macOS with Apple Silicon → MLX is ALWAYS the default
    if sys.platform == "darwin":
        try:
            import mlx  # noqa: F401
            print("[TEACHER] Backend auto-detected: mlx (macOS/Apple Silicon)")
            return "mlx"
        except ImportError:
            print("[TEACHER] ⚠️ macOS detected but MLX not installed — falling back to torch", file=sys.stderr)

    # Linux/Windows → torch (auto-detects CUDA/CPU at runtime)
    try:
        import torch  # noqa: F401
        device = "cuda" if torch.cuda.is_available() else "cpu"
        print(f"[TEACHER] Backend auto-detected: torch (device={device})")
        return "torch"
    except ImportError:
        pass

    print("[TEACHER] ❌ No training backend available.", file=sys.stderr)
    print("[TEACHER] Install mlx-lm-lora (macOS) or transformers+peft+trl (Linux/Windows).", file=sys.stderr)
    sys.exit(1)

# ─── Data Loading ────────────────────────────────────────────────────────────

def load_jsonl(path: Path, max_items: int = None) -> list:
    """Load JSONL file, return list of dicts."""
    if not path.exists():
        return []
    items = []
    with open(path) as f:
        for i, line in enumerate(f):
            line = line.strip()
            if line:
                try:
                    items.append(json.loads(line))
                except json.JSONDecodeError as e:
                    print(f"[TEACHER] ⚠️ Skipping malformed JSONL line {i+1} in {path}: {e}", file=sys.stderr)
                    continue
    if max_items:
        items = items[-max_items:]  # Take most recent
    return items


def load_manifest() -> dict:
    """Load or create manifest."""
    if MANIFEST_PATH.exists():
        try:
            with open(MANIFEST_PATH) as f:
                return json.load(f)
        except (json.JSONDecodeError, IOError) as e:
            print(f"[TEACHER] ⚠️ Failed to load manifest: {e}. Using defaults.", file=sys.stderr)
    return {
        "current": MLX_BASE_MODEL,
        "base": MLX_BASE_MODEL,
        "history": [],
        "retention": 5
    }


def save_manifest(manifest: dict):
    """Save manifest to disk."""
    try:
        with open(MANIFEST_PATH, "w") as f:
            json.dump(manifest, f, indent=2)
    except IOError as e:
        print(f"[TEACHER] ❌ Failed to save manifest: {e}", file=sys.stderr)
        sys.exit(1)

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
    """Extract the identity/persona section from the full kernel prompt."""
    start = prompt.find("You are Apis")
    if start == -1:
        for marker in ["# Identity", "## Identity", "# Persona"]:
            start = prompt.find(marker)
            if start != -1:
                break

    if start == -1:
        return "You are Apis, the intelligent core of the HIVE Engine."

    remainder = prompt[start:]
    end_markers = ["### Self-Supervised", "### Capabilities and Limits",
                   "### Available Tools", "## Tool Definitions"]

    end = len(remainder)
    for marker in end_markers:
        idx = remainder.find(marker)
        if idx > 0 and idx < end:
            end = idx

    identity = remainder[:end].rstrip()

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

# ─── MLX Training Backend ───────────────────────────────────────────────────

def train_mlx(sft_data: list, train_from: str, resume_adapter: str,
              lr: float, epochs: int, max_seq_len: int):
    """Train via MLX LoRA (Apple Silicon native). Existing, proven path."""
    sft_cmd = (
        f"python3 -m mlx_lm.lora "
        f"--model {train_from} "
        f"--data {OUTPUT_DIR} "
        f"--train "
        f"--num-layers {LORA_R} "
        f"--learning-rate {lr} "
        f"--iters {epochs} "
        f"--batch-size 1 "
        f"--max-seq-length {max_seq_len} "
        f"--adapter-path {OUTPUT_DIR}/adapters"
    )
    if resume_adapter:
        sft_cmd += f" --resume-adapter-file {resume_adapter}/adapters.safetensors"

    print(f"[TEACHER] Running MLX: {sft_cmd}")
    exit_code = os.system(sft_cmd)
    if exit_code != 0:
        print(f"[TEACHER] ❌ MLX SFT training failed with exit code {exit_code}", file=sys.stderr)
        sys.exit(1)

# ─── Torch Training Backend ─────────────────────────────────────────────────

def train_torch(sft_data: list, train_from: str, resume_adapter: str,
                lr: float, epochs: int, max_seq_len: int):
    """Train via HuggingFace Transformers + PEFT (Linux/Windows/CPU)."""
    try:
        import torch
        from transformers import AutoModelForCausalLM, AutoTokenizer, TrainingArguments
        from peft import LoraConfig, get_peft_model, PeftModel
        from trl import SFTTrainer, SFTConfig
        from datasets import Dataset
    except ImportError as e:
        print(f"[TEACHER] ❌ Torch backend missing dependencies: {e}", file=sys.stderr)
        print("[TEACHER] Install: pip install -r training/requirements_torch.txt", file=sys.stderr)
        sys.exit(1)

    # Device detection
    if torch.cuda.is_available():
        device = "cuda"
        dtype = torch.bfloat16
        print(f"[TEACHER] Torch device: CUDA ({torch.cuda.get_device_name(0)})")
    elif hasattr(torch.backends, 'mps') and torch.backends.mps.is_available():
        device = "mps"
        dtype = torch.float32
        print("[TEACHER] Torch device: MPS (Apple Silicon via PyTorch)")
    else:
        device = "cpu"
        dtype = torch.float32
        print("[TEACHER] Torch device: CPU (training will be slow)")

    model_id = os.environ.get("HIVE_TRAINING_MODEL_TORCH", train_from)
    print(f"[TEACHER] Loading model: {model_id} (dtype={dtype})")

    tokenizer = AutoTokenizer.from_pretrained(model_id, trust_remote_code=True)
    if tokenizer.pad_token is None:
        tokenizer.pad_token = tokenizer.eos_token

    model = AutoModelForCausalLM.from_pretrained(
        model_id,
        torch_dtype=dtype,
        device_map=device if device == "cuda" else None,
        trust_remote_code=True,
    )
    if device == "mps":
        model = model.to("mps")

    # Resume from previous adapter if stacking
    if resume_adapter:
        adapter_path = Path(resume_adapter) / "adapters"
        if adapter_path.exists():
            print(f"[TEACHER] Loading previous adapter for stacking: {adapter_path}")
            model = PeftModel.from_pretrained(model, str(adapter_path))
            model = model.merge_and_unload()

    # Apply LoRA
    lora_config = LoraConfig(
        r=LORA_R,
        lora_alpha=LORA_ALPHA,
        target_modules=["q_proj", "v_proj", "k_proj", "o_proj"],
        lora_dropout=0.05,
        bias="none",
        task_type="CAUSAL_LM",
    )
    model = get_peft_model(model, lora_config)
    model.print_trainable_parameters()

    # Prepare dataset
    sft_path = OUTPUT_DIR / "train.jsonl"
    dataset = Dataset.from_json(str(sft_path))

    # Training config
    adapter_output = str(OUTPUT_DIR / "adapters")
    training_args = SFTConfig(
        output_dir=adapter_output,
        num_train_epochs=epochs,
        per_device_train_batch_size=1,
        learning_rate=lr,
        max_seq_length=max_seq_len,
        logging_steps=1,
        save_strategy="no",
        fp16=(device == "cuda" and dtype == torch.float16),
        bf16=(device == "cuda" and dtype == torch.bfloat16),
        report_to="none",
    )

    trainer = SFTTrainer(
        model=model,
        train_dataset=dataset,
        processing_class=tokenizer,
        args=training_args,
    )

    print("[TEACHER] Starting Torch training...")
    trainer.train()

    # Save adapter
    print(f"[TEACHER] Saving adapter to {adapter_output}")
    model.save_pretrained(adapter_output)
    tokenizer.save_pretrained(adapter_output)

    print("[TEACHER] ✅ Torch training complete")

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
    parser.add_argument("--backend", choices=["auto", "mlx", "torch"], default="auto",
                       help="Training backend (default: auto-detect)")
    return parser.parse_args()


def main():
    args = parse_args()
    dry_run = args.dry_run

    # Detect backend
    backend = detect_backend(args.backend)

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
    print(f"[TEACHER] Mode: {mode_label} | Backend: {backend}")
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
        print("[TEACHER] ❌ No training data available. Exiting.", file=sys.stderr)
        sys.exit(1)

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
        print("[TEACHER] ✅ Dry run complete. Data parsed successfully.")
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

    # 5. Resolve model and adapter for training
    manifest = load_manifest()
    new_version = get_next_version(manifest)
    parent = manifest["current"]

    # Select base model per backend
    if backend == "mlx":
        train_from = manifest.get("base", MLX_BASE_MODEL)
    else:
        train_from = os.environ.get("HIVE_TRAINING_MODEL_TORCH", TORCH_BASE_MODEL)

    # Find the latest adapter for stacking
    resume_adapter = None
    if args.stack and manifest.get("latest_adapter"):
        adapter_dir = Path(manifest["latest_adapter"])
        adapter_file = adapter_dir / "adapters.safetensors"
        if adapter_file.exists():
            resume_adapter = str(adapter_dir)
            print(f"[TEACHER] Stacking on adapter: {resume_adapter}")
        else:
            print(f"[TEACHER] ⚠️ No previous adapter found at {adapter_dir}, training from scratch")

    print(f"[TEACHER] Training {new_version} (model: {train_from}, parent: {parent})")
    print(f"[TEACHER] Config: lr={LEARNING_RATE}, epochs={NUM_EPOCHS}, r={LORA_R}, seq_len={MAX_SEQ_LEN}")

    # 6. Run training via selected backend
    if sft_data:
        if backend == "mlx":
            train_mlx(sft_data, train_from, resume_adapter,
                      LEARNING_RATE, NUM_EPOCHS, MAX_SEQ_LEN)
        else:
            train_torch(sft_data, train_from, resume_adapter,
                        LEARNING_RATE, NUM_EPOCHS, MAX_SEQ_LEN)

    # 7. Version the adapter for cumulative stacking
    versioned_adapter_dir = OUTPUT_DIR / "adapters" / new_version
    versioned_adapter_dir.mkdir(parents=True, exist_ok=True)

    for f_name in ["adapters.safetensors", "adapter_config.json"]:
        src = OUTPUT_DIR / "adapters" / f_name
        if src.exists():
            shutil.copy2(str(src), str(versioned_adapter_dir / f_name))

    # Torch saves differently — also copy PEFT files
    for f_name in ["adapter_model.safetensors", "adapter_config.json"]:
        src = OUTPUT_DIR / "adapters" / f_name
        if src.exists():
            shutil.copy2(str(src), str(versioned_adapter_dir / f_name))

    print(f"[TEACHER] 💾 Adapter saved: {versioned_adapter_dir}")

    # 8. Update manifest
    manifest["history"].append({
        "version": new_version,
        "date": datetime.now().isoformat(),
        "golden_count": len(golden),
        "pair_count": len(pairs),
        "parent": parent,
        "adapter_path": str(versioned_adapter_dir),
        "backend": backend,
    })
    manifest["current"] = new_version
    manifest["latest_adapter"] = str(OUTPUT_DIR / "adapters")
    save_manifest(manifest)

    # 9. Archive processed data
    archive_processed(len(golden), len(pairs))

    # 10. Cleanup old adapters (keep last N)
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

    print(f"[TEACHER] ✅ Training complete: {new_version} (backend: {backend})")
    print(f"[TEACHER] Golden: {len(golden)} | Pairs: {len(pairs)}")


if __name__ == "__main__":
    main()
