#!/usr/bin/env python3
"""
Flux Image Generation Server — runs on HOST alongside Ollama.
Docker calls this via http://host.docker.internal:8490/generate

Usage:
    python3 src/computer/flux_server.py

Listens on 0.0.0.0:8490
"""
import sys
import os
import json
import time
import torch
import warnings
import threading
from http.server import HTTPServer, BaseHTTPRequestHandler

warnings.filterwarnings("ignore")

# Lazy-load the pipeline on first request
_pipeline = None
_pipeline_lock = threading.Lock()

def get_pipeline():
    global _pipeline
    if _pipeline is None:
        with _pipeline_lock:
            if _pipeline is None:
                from diffusers import FluxPipeline
                device = "mps" if torch.backends.mps.is_available() else "cpu"
                dtype = torch.bfloat16
                print(f"[FLUX SERVER] Loading FLUX.1-dev on {device}...", flush=True)
                _pipeline = FluxPipeline.from_pretrained(
                    "black-forest-labs/FLUX.1-dev",
                    torch_dtype=dtype
                )
                _pipeline.to(device)
                print(f"[FLUX SERVER] ✅ Pipeline ready on {device}", flush=True)
    return _pipeline

class FluxHandler(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path != "/generate":
            self.send_error(404)
            return

        content_len = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_len)
        try:
            req = json.loads(body)
        except json.JSONDecodeError:
            self.send_error(400, "Invalid JSON")
            return

        prompt = req.get("prompt", "")
        output_path = req.get("output_path", f"/tmp/flux-{int(time.time()*1000)}.png")
        width = int(req.get("width", 1024))
        height = int(req.get("height", 1024))

        print(f"[FLUX SERVER] Generating: '{prompt[:60]}...' ({width}x{height})", flush=True)

        try:
            pipe = get_pipeline()
            image = pipe(
                prompt,
                guidance_scale=3.5,
                num_inference_steps=50,
                width=width,
                height=height,
                max_sequence_length=512,
                generator=torch.Generator("cpu").manual_seed(0)
            ).images[0]

            os.makedirs(os.path.dirname(output_path) or ".", exist_ok=True)
            image.save(output_path)
            print(f"[FLUX SERVER] ✅ Saved to {output_path}", flush=True)

            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"status": "ok", "path": output_path}).encode())
        except Exception as e:
            print(f"[FLUX SERVER] ❌ Error: {e}", flush=True)
            self.send_response(500)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"status": "error", "error": str(e)}).encode())

    def log_message(self, format, *args):
        pass  # Suppress default HTTP logging

def main():
    port = int(os.environ.get("FLUX_PORT", "8490"))
    server = HTTPServer(("0.0.0.0", port), FluxHandler)
    print(f"[FLUX SERVER] 🎨 Listening on http://0.0.0.0:{port}/generate", flush=True)
    print(f"[FLUX SERVER] Pipeline will load on first request.", flush=True)
    server.serve_forever()

if __name__ == "__main__":
    main()
