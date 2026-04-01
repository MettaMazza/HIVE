#!/usr/bin/env python3
"""
HIVE Vector Backfill — One-time script to embed all existing memory data.
Reads .env for HIVE_EMBED_MODEL and HIVE_OLLAMA_URL, then scans the memory/
directory for timeline.jsonl, lessons.jsonl, and synaptic/nodes.jsonl files,
embeds each entry, and writes the vector index to memory/vectors/index.bin.

Usage: python3 backfill_vectors.py
"""

import os
import json
import struct
import time
import sys

try:
    import requests
except ImportError:
    print("Installing requests...")
    os.system(f"{sys.executable} -m pip install requests -q")
    import requests

try:
    import msgpack
except ImportError:
    print("Installing msgpack...")
    os.system(f"{sys.executable} -m pip install msgpack -q")
    import msgpack

# ── Load .env ─────────────────────────────────────────────────
def load_env():
    env_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), ".env")
    if os.path.exists(env_path):
        with open(env_path) as f:
            for line in f:
                line = line.strip()
                if line and not line.startswith("#") and "=" in line:
                    key, _, val = line.partition("=")
                    os.environ.setdefault(key.strip(), val.strip())

load_env()

EMBED_MODEL = os.environ.get("HIVE_EMBED_MODEL", "nomic-embed-text")
OLLAMA_URL = os.environ.get("HIVE_OLLAMA_URL", "http://localhost:11434")
MEMORY_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "memory")
VECTORS_DIR = os.path.join(MEMORY_DIR, "vectors")
INDEX_PATH = os.path.join(VECTORS_DIR, "index.bin")

print(f"🧠 HIVE Vector Backfill")
print(f"   Model:  {EMBED_MODEL}")
print(f"   Ollama: {OLLAMA_URL}")
print(f"   Memory: {MEMORY_DIR}")
print()

# ── Embedding client ─────────────────────────────────────────
def embed_single(text: str) -> list[float]:
    """Embed a single chunk (must be <= 2048 chars) via Ollama /api/embed."""
    resp = requests.post(
        f"{OLLAMA_URL}/api/embed",
        json={"model": EMBED_MODEL, "input": text},
        timeout=30,
    )
    resp.raise_for_status()
    data = resp.json()
    return data["embeddings"][0]

def chunk_text(text: str, chunk_size: int = 2048) -> list[str]:
    """Split text into chunks at word boundaries."""
    if len(text) <= chunk_size:
        return [text]
    
    chunks = []
    start = 0
    while start < len(text):
        end = min(start + chunk_size, len(text))
        if end < len(text):
            # Break at last space or newline
            break_at = text.rfind(' ', start, end)
            if break_at == -1:
                break_at = text.rfind('\n', start, end)
            if break_at > start:
                end = break_at + 1
        chunk = text[start:end].strip()
        if chunk:
            chunks.append(chunk)
        start = end
    return chunks

def embed_chunked(text: str) -> list[tuple[int, list[float]]]:
    """Embed text by chunking. Returns (chunk_index, vector) pairs."""
    chunks = chunk_text(text)
    results = []
    for i, chunk in enumerate(chunks):
        vec = embed_single(chunk)
        results.append((i, vec))
    return results

# ── Load existing index ──────────────────────────────────────
def load_existing_index() -> tuple[list, set]:
    """Load existing index.bin (msgpack format matching Rust VectorEntry)."""
    if os.path.exists(INDEX_PATH):
        with open(INDEX_PATH, "rb") as f:
            raw = f.read()
        try:
            entries = msgpack.unpackb(raw, raw=False)
            known_ids = set()
            for entry in entries:
                # Rust struct serialized as array: [id, source, text_preview, vector, timestamp]
                if isinstance(entry, (list, tuple)):
                    known_ids.add(entry[0])
                elif isinstance(entry, dict):
                    known_ids.add(entry.get("id", ""))
            print(f"   Loaded {len(entries)} existing entries from index.bin")
            return entries, known_ids
        except Exception as e:
            print(f"   ⚠️ Failed to parse index.bin: {e}. Starting fresh.")
    return [], set()

def save_index(entries: list):
    """Save index as msgpack (matching Rust rmp_serde VectorEntry serialization)."""
    os.makedirs(VECTORS_DIR, exist_ok=True)
    raw = msgpack.packb(entries, use_bin_type=True)
    with open(INDEX_PATH, "wb") as f:
        f.write(raw)
    print(f"\n✅ Saved {len(entries)} entries to {INDEX_PATH} ({len(raw)} bytes)")

# Source type enum matching Rust: Timeline=0, Synaptic=1, Lesson=2
# In msgpack with rmp-serde, enums serialize as {"variant_name": null} or as a string
# For rmp_serde with internally tagged enums, it serializes as a map.
# But for unit variants, rmp_serde serializes as the variant name string.
SOURCE_TIMELINE = "Timeline"
SOURCE_SYNAPTIC = "Synaptic"
SOURCE_LESSON = "Lesson"

def make_entry(entry_id: str, source: str, preview: str, vector: list, timestamp: str) -> list:
    """Create an entry matching Rust VectorEntry rmp_serde serialization.
    rmp_serde serializes structs as arrays by default."""
    return [entry_id, source, preview, vector, timestamp]

# ── Scan and embed ───────────────────────────────────────────
def find_files(directory: str, filename: str) -> list[str]:
    """Recursively find files with a specific name."""
    results = []
    for root, dirs, files in os.walk(directory):
        if filename in files:
            results.append(os.path.join(root, filename))
    return results

def backfill_timelines(entries: list, known_ids: set) -> int:
    """Embed all timeline.jsonl entries."""
    indexed = 0
    timeline_files = find_files(MEMORY_DIR, "timeline.jsonl")
    print(f"\n📜 Timeline files found: {len(timeline_files)}")
    
    for tl_path in timeline_files:
        rel_path = os.path.relpath(os.path.dirname(tl_path), MEMORY_DIR)
        scope_key = rel_path.replace(os.sep, ":")
        
        with open(tl_path) as f:
            lines = f.readlines()
        
        for line_idx, line in enumerate(lines):
            entry_id = f"timeline:{scope_key}:{line_idx}"
            if entry_id in known_ids:
                continue
            
            try:
                event = json.loads(line.strip())
            except json.JSONDecodeError:
                continue
            
            author = event.get("author_name", "unknown")
            content = event.get("content", "")
            timestamp = event.get("timestamp", "")
            
            if not content or content.startswith("***"):
                continue
            
            text = f"{author}: {content}"
            
            try:
                chunks = embed_chunked(text)
                for chunk_idx, vec in chunks:
                    chunk_entry_id = f"{entry_id}:c{chunk_idx}" if len(chunks) > 1 else entry_id
                    if chunk_entry_id in known_ids:
                        continue
                    chunk_preview = chunk_text(text)[chunk_idx][:200] if chunk_idx < len(chunk_text(text)) else text[:200]
                    entries.append(make_entry(chunk_entry_id, SOURCE_TIMELINE, chunk_preview, vec, timestamp))
                    known_ids.add(chunk_entry_id)
                    indexed += 1
                if indexed % 10 == 0:
                    print(f"   📜 {indexed} timeline chunks embedded...", end="\r")
            except Exception as e:
                print(f"\n   ❌ Embed failed at {entry_id}: {e}")
                continue  # Skip bad entries, don't stop
    
    print(f"   📜 {indexed} timeline chunks embedded.          ")
    return indexed

def backfill_synaptic(entries: list, known_ids: set) -> int:
    """Embed all synaptic/nodes.jsonl entries."""
    indexed = 0
    nodes_path = os.path.join(MEMORY_DIR, "synaptic", "nodes.jsonl")
    
    if not os.path.exists(nodes_path):
        print(f"\n🧬 No synaptic nodes found.")
        return 0
    
    print(f"\n🧬 Synaptic nodes file: {nodes_path}")
    
    with open(nodes_path) as f:
        lines = f.readlines()
    
    for line in lines:
        try:
            node = json.loads(line.strip())
        except json.JSONDecodeError:
            continue
        
        concept = node.get("concept", "")
        if not concept:
            continue
        
        entry_id = f"synaptic:{concept.lower()}"
        if entry_id in known_ids:
            continue
        
        data_items = node.get("data", [])
        if isinstance(data_items, list):
            data_str = "; ".join(str(d) for d in data_items)
        else:
            data_str = str(data_items)
        
        text = f"{concept}: {data_str}"
        preview = text[:200]
        timestamp = node.get("updated_at", "")
        
        try:
            chunks = embed_chunked(text)
            for chunk_idx, vec in chunks:
                chunk_entry_id = f"{entry_id}:c{chunk_idx}" if len(chunks) > 1 else entry_id
                if chunk_entry_id in known_ids:
                    continue
                chunk_preview = chunk_text(text)[chunk_idx][:200] if chunk_idx < len(chunk_text(text)) else text[:200]
                entries.append(make_entry(chunk_entry_id, SOURCE_SYNAPTIC, chunk_preview, vec, timestamp))
                known_ids.add(chunk_entry_id)
                indexed += 1
                if indexed % 10 == 0:
                    print(f"   🧬 {indexed} synaptic chunks embedded...", end="\r")
        except Exception as e:
            print(f"\n   ❌ Embed failed at {entry_id}: {e}")
            continue
    
    print(f"   🧬 {indexed} synaptic chunks embedded.          ")
    return indexed

def backfill_lessons(entries: list, known_ids: set) -> int:
    """Embed all lessons.jsonl entries."""
    indexed = 0
    lesson_files = find_files(MEMORY_DIR, "lessons.jsonl")
    print(f"\n📚 Lesson files found: {len(lesson_files)}")
    
    for lesson_path in lesson_files:
        rel_path = os.path.relpath(os.path.dirname(lesson_path), MEMORY_DIR)
        scope_key = rel_path.replace(os.sep, ":")
        
        with open(lesson_path) as f:
            lines = f.readlines()
        
        for line_idx, line in enumerate(lines):
            try:
                lesson = json.loads(line.strip())
            except json.JSONDecodeError:
                continue
            
            id_val = lesson.get("id", "")
            entry_id = f"lesson:{id_val}" if id_val else f"lesson:{scope_key}:{line_idx}"
            
            if entry_id in known_ids:
                continue
            
            text = lesson.get("text", "")
            if not text:
                continue
            
            preview = text[:200]
            timestamp = lesson.get("learned_at", "")
            
            try:
                chunks = embed_chunked(text)
                for chunk_idx, vec in chunks:
                    chunk_entry_id = f"{entry_id}:c{chunk_idx}" if len(chunks) > 1 else entry_id
                    if chunk_entry_id in known_ids:
                        continue
                    chunk_preview = chunk_text(text)[chunk_idx][:200] if chunk_idx < len(chunk_text(text)) else text[:200]
                    entries.append(make_entry(chunk_entry_id, SOURCE_LESSON, chunk_preview, vec, timestamp))
                    known_ids.add(chunk_entry_id)
                    indexed += 1
                    if indexed % 10 == 0:
                        print(f"   📚 {indexed} lesson chunks embedded...", end="\r")
            except Exception as e:
                print(f"\n   ❌ Embed failed at {entry_id}: {e}")
                continue
    
    print(f"   📚 {indexed} lesson chunks embedded.           ")
    return indexed

# ── Main ─────────────────────────────────────────────────────
if __name__ == "__main__":
    # Test embedding connectivity
    print("🔌 Testing Ollama embedding endpoint...")
    try:
        test_vec = embed_single("connection test")
        print(f"   ✅ Got {len(test_vec)}-dimensional vector from {EMBED_MODEL}")
    except Exception as e:
        print(f"   ❌ Failed: {e}")
        print(f"   Make sure Ollama is running and {EMBED_MODEL} is pulled.")
        sys.exit(1)
    
    entries, known_ids = load_existing_index()
    start = time.time()
    
    total = 0
    total += backfill_timelines(entries, known_ids)
    total += backfill_synaptic(entries, known_ids)
    total += backfill_lessons(entries, known_ids)
    
    elapsed = time.time() - start
    
    if total > 0:
        save_index(entries)
    
    print(f"\n🏁 Backfill complete: {total} new entries in {elapsed:.1f}s")
    print(f"   Total index size: {len(entries)} entries")
    
    # Stats
    t_count = sum(1 for e in entries if (isinstance(e, list) and e[1] == SOURCE_TIMELINE) or (isinstance(e, dict) and e.get("source") == SOURCE_TIMELINE))
    s_count = sum(1 for e in entries if (isinstance(e, list) and e[1] == SOURCE_SYNAPTIC) or (isinstance(e, dict) and e.get("source") == SOURCE_SYNAPTIC))
    l_count = sum(1 for e in entries if (isinstance(e, list) and e[1] == SOURCE_LESSON) or (isinstance(e, dict) and e.get("source") == SOURCE_LESSON))
    print(f"   Timeline: {t_count} | Synaptic: {s_count} | Lessons: {l_count}")
