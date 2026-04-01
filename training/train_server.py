#!/usr/bin/env python3
"""
HIVE Training Server — Host-side HTTP wrapper for train_teacher.py

Runs on the HOST machine (not inside Docker) so training has full access
to Metal/MLX/GPU acceleration and the local model cache (512GB M3 Ultra).

Docker calls this via http://host.docker.internal:8491/train
Same pattern as Ollama (inference) and Flux (images).

Usage:
    python3 training/train_server.py              # Start on port 8491
    python3 training/train_server.py --port 8492   # Custom port
"""

import http.server
import json
import os
import subprocess
import sys
import threading
from urllib.parse import urlparse

PORT = int(os.environ.get("HIVE_TRAINING_PORT", "8491"))

# Lock to prevent concurrent training runs
training_lock = threading.Lock()


class TrainingHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        print(f"[TRAIN SERVER] {format % args}", flush=True)

    def do_GET(self):
        """Health check endpoint."""
        if self.path == "/health":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({
                "status": "ready",
                "locked": training_lock.locked(),
            }).encode())
            return

        self.send_response(404)
        self.end_headers()

    def do_POST(self):
        if self.path != "/train":
            self.send_response(404)
            self.end_headers()
            return

        # Read request body
        content_length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_length).decode() if content_length > 0 else "{}"

        try:
            params = json.loads(body)
        except json.JSONDecodeError:
            params = {}

        # Prevent concurrent training
        if not training_lock.acquire(blocking=False):
            self.send_response(409)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({
                "status": "busy",
                "error": "Training already in progress",
            }).encode())
            return

        try:
            result = run_training(params)
            status_code = 200 if result["exit_code"] == 0 else 500

            self.send_response(status_code)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(result).encode())
        finally:
            training_lock.release()

    def run_training(params):
        """NOT a method - standalone function called by do_POST."""
        pass  # Defined below


def run_training(params: dict) -> dict:
    """Execute train_teacher.py with the given parameters on the HOST."""
    # Use sys.executable so the server spawns train_teacher with the exact same
    # python binary that launched the server (which should be python3.12 for MLX MoE)
    python_bin = os.environ.get("HIVE_PYTHON_BIN", sys.executable)

    # Build command from params (mirrors the Rust subprocess call)
    cmd = [python_bin, "training/train_teacher.py"]

    if params.get("micro", True):
        cmd.append("--micro")
    if params.get("stack", True):
        cmd.append("--stack")
    if params.get("backend"):
        cmd.extend(["--backend", str(params["backend"])])
    if params.get("examples"):
        cmd.extend(["--examples", str(params["examples"])])
    if params.get("lr"):
        cmd.extend(["--lr", str(params["lr"])])
    if params.get("epochs"):
        cmd.extend(["--epochs", str(params["epochs"])])
    if params.get("max_seq_len"):
        cmd.extend(["--max-seq-len", str(params["max_seq_len"])])

    print(f"[TRAIN SERVER] Executing: {' '.join(cmd)}", flush=True)

    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=3600,  # 1 hour max
            cwd=os.path.dirname(os.path.dirname(os.path.abspath(__file__))),  # HIVE root
        )

        print(f"[TRAIN SERVER] Exit code: {result.returncode}", flush=True)
        if result.stdout:
            print(f"[TRAIN SERVER] stdout: {result.stdout[-500:]}", flush=True)
        if result.stderr:
            print(f"[TRAIN SERVER] stderr: {result.stderr[-500:]}", flush=True)

        return {
            "status": "success" if result.returncode == 0 else "failed",
            "exit_code": result.returncode,
            "stdout": result.stdout[-2000:] if result.stdout else "",
            "stderr": result.stderr[-2000:] if result.stderr else "",
        }
    except subprocess.TimeoutExpired:
        return {
            "status": "timeout",
            "exit_code": -1,
            "stdout": "",
            "stderr": "Training timed out after 3600s",
        }
    except Exception as e:
        return {
            "status": "error",
            "exit_code": -1,
            "stdout": "",
            "stderr": str(e),
        }


# Patch the method to use the module-level function
TrainingHandler.run_training = staticmethod(run_training)


def main():
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=PORT)
    args = parser.parse_args()

    server = http.server.HTTPServer(("0.0.0.0", args.port), TrainingHandler)
    print(f"[TRAIN SERVER] 🧠 Training server listening on http://0.0.0.0:{args.port}", flush=True)
    print(f"[TRAIN SERVER] POST /train  — Run training", flush=True)
    print(f"[TRAIN SERVER] GET  /health — Health check", flush=True)

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n[TRAIN SERVER] Shutting down.", flush=True)
        server.server_close()


if __name__ == "__main__":
    main()
