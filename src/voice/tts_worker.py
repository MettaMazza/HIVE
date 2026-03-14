import sys
import os
import soundfile as sf
import asyncio
import re
import numpy as np

# Provide a robust sanitization script that strips unpronounceable characters
def sanitize_text(text: str) -> str:
    # Remove URLs
    text = re.sub(r'http\S+', '', text)
    # Remove markdown characters
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
    # Don't strip punctuation, we need it for chunking
    return re.sub(r'\s+', ' ', text).strip()

async def generate():
    if len(sys.argv) < 3:
        print("Usage: python tts_worker.py <text> <output.wav>")
        sys.exit(1)

    text = sys.argv[1]
    output_path = sys.argv[2]
    
    clean_text = sanitize_text(text)
    if not clean_text:
        print("Text empty after sanitization.")
        sys.exit(0)

    try:
        from kokoro_onnx import Kokoro
        
        # Hardcode the model paths as per Core running inside the virtualenv
        base_dir = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        model_path = os.path.join(base_dir, "models", "kokoro-v1.0.onnx")
        voices_path = os.path.join(base_dir, "models", "voices-v1.0.bin")
        
        kokoro = Kokoro(model_path, voices_path)
        
        # Ernos 3.0 pacing match: Chunk by punctuation to simulate breaths
        # Split on commas, periods, exclamation, and question marks but keep the delimeter
        chunks = re.split(r'([,.\!\?]+)', clean_text)
        
        master_audio = []
        sample_rate = 24000 # Default kokoro sample rate
        
        for i in range(0, len(chunks), 2):
            text_chunk = chunks[i].strip()
            punct_chunk = chunks[i+1] if i+1 < len(chunks) else ""
            
            if not text_chunk:
                continue
                
            samples, sr = kokoro.create(
                text_chunk + punct_chunk,
                voice="af_heart",
                speed=1.0,
                lang="en-us"
            )
            sample_rate = sr
            master_audio.append(samples)
            
            # Inject silence based on punctuation (0.2s for comma, 0.4s for full stop)
            if ',' in punct_chunk:
                silence_frames = int(0.25 * sample_rate)
                master_audio.append(np.zeros(silence_frames, dtype=np.float32))
            elif any(p in punct_chunk for p in ['.', '!', '?']):
                silence_frames = int(0.5 * sample_rate)
                master_audio.append(np.zeros(silence_frames, dtype=np.float32))
        
        if master_audio:
            final_samples = np.concatenate(master_audio)
            sf.write(output_path, final_samples, sample_rate)
            print(f"SUCCESS:{output_path}")
        else:
            print("ERROR: No valid audio generated after chunking.")
            sys.exit(1)
        
    except ImportError:
        print("ERROR: kokoro-onnx not installed in the current environment.")
        sys.exit(1)
    except Exception as e:
        print(f"ERROR: failed to generate audio: {e}")
        sys.exit(1)

if __name__ == "__main__":
    asyncio.run(generate())
