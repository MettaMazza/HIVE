"""Streaming TTS worker for Kokoro ONNX.

Writes raw 16-bit PCM chunks (24kHz mono) directly to stdout as they are
generated, so the Rust caller can pipe them to the WebSocket in real-time.

Protocol:
  stdout → raw int16 little-endian PCM bytes (no header, no framing)
  stderr → status/error messages (read by Rust for logging)

Usage: python tts_stream_worker.py "<text>"
"""
import sys
import os
import re
import struct
import numpy as np

def sanitize_text(text: str) -> str:
    text = re.sub(r'http\S+', '', text)
    text = re.sub(r'[\#\*\_\(\)\[\]]', '', text)
    emoji_pattern = re.compile(
        "["
        "\U0001F600-\U0001FAFF"
        "\U00002702-\U000027B0"
        "\U000024C2-\U0000257F"
        "\U0000FE00-\U0000FE0F"
        "\U0000200D"
        "\U00002600-\U000026FF"
        "\U00002300-\U000023FF"
        "\U00002B50-\U00002B55"
        "\U000023CF-\U000023F3"
        "\U0000203C-\U00003299"
        "]+",
        flags=re.UNICODE
    )
    text = emoji_pattern.sub('', text)
    return re.sub(r'\s+', ' ', text).strip()


def main():
    if len(sys.argv) < 2:
        print("Usage: python tts_stream_worker.py <text>", file=sys.stderr)
        sys.exit(1)

    text = sys.argv[1]
    import asyncio
    
    async def run_tts():
        clean_text = sanitize_text(text)
        if not clean_text:
            print("Text empty after sanitization.", file=sys.stderr)
            sys.exit(0)

        try:
            from kokoro_onnx import Kokoro

            base_dir = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
            model_path = os.path.join(base_dir, "models", "kokoro-v1.0.onnx")
            voices_path = os.path.join(base_dir, "models", "voices-v1.0.bin")

            kokoro = Kokoro(model_path, voices_path)

            stream = kokoro.create_stream(
                clean_text,
                voice="af_heart",
                speed=1.0,
                lang="en-us"
            )

            stdout_bin = sys.stdout.buffer

            async for samples, sample_rate in stream:
                clamped = np.clip(samples, -1.0, 1.0)
                pcm_int16 = (clamped * 32767).astype(np.int16)
                stdout_bin.write(pcm_int16.tobytes())
                stdout_bin.flush()

            print(f"STREAM_DONE", file=sys.stderr)

        except ImportError:
            print("ERROR: kokoro-onnx not installed.", file=sys.stderr)
            sys.exit(1)
        except Exception as e:
            print(f"ERROR: {e}", file=sys.stderr)
            sys.exit(1)

    asyncio.run(run_tts())

if __name__ == "__main__":
    main()
