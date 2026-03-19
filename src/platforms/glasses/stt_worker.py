"""
HIVE Glasses STT Worker — Speech-to-Text via Google Speech Recognition.

Usage: python stt_worker.py <wav_file>
Output: TRANSCRIPTION:<text> on success

Uses the `speech_recognition` library with Google's free API.
Falls back gracefully if audio is unintelligible or the API is unreachable.
"""
import sys
import speech_recognition as sr


def transcribe(wav_path: str) -> str:
    recognizer = sr.Recognizer()
    
    # Adjust for ambient noise sensitivity
    recognizer.energy_threshold = 300
    recognizer.dynamic_energy_threshold = True
    
    try:
        with sr.AudioFile(wav_path) as source:
            audio = recognizer.record(source)
        
        text = recognizer.recognize_google(audio)
        return text.strip()
    
    except sr.UnknownValueError:
        # Audio was unintelligible
        return ""
    except sr.RequestError as e:
        print(f"ERROR: Google STT API error: {e}", file=sys.stderr)
        sys.exit(1)
    except Exception as e:
        print(f"ERROR: STT failed: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python stt_worker.py <wav_file>", file=sys.stderr)
        sys.exit(1)
    
    wav_path = sys.argv[1]
    result = transcribe(wav_path)
    
    if result:
        print(f"TRANSCRIPTION:{result}")
    else:
        print("TRANSCRIPTION:")
