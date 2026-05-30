#!/usr/bin/env python3
"""Audio transcription via faster-whisper.

Usage:
    python3 transcribe.py <audio_path> [model_size] [language]

Outputs JSON: {"text": "...", "language": "...", "duration": ..., "segments": [...]}
Exit code 0 on success, 1 on error (stderr has details).
"""

import json
import sys
import os
import warnings

warnings.filterwarnings("ignore")
os.environ["CT2_VERBOSE"] = "0"


def main():
    if len(sys.argv) < 2:
        print(json.dumps({"error": "Usage: transcribe.py <audio_path> [model_size] [language]"}))
        sys.exit(1)

    audio_path = sys.argv[1]
    model_size = sys.argv[2] if len(sys.argv) > 2 else "base"
    language = sys.argv[3] if len(sys.argv) > 3 else None  # None = auto-detect

    if not os.path.isfile(audio_path):
        print(json.dumps({"error": f"File not found: {audio_path}"}))
        sys.exit(1)

    try:
        from faster_whisper import WhisperModel

        model = WhisperModel(model_size, device="cpu", compute_type="int8")

        segments_gen, info = model.transcribe(
            audio_path,
            language=language,
            beam_size=5,
            vad_filter=True,
        )

        segments = []
        full_text_parts = []
        for seg in segments_gen:
            segments.append({
                "start": round(seg.start, 2),
                "end": round(seg.end, 2),
                "text": seg.text.strip(),
            })
            full_text_parts.append(seg.text.strip())

        result = {
            "text": " ".join(full_text_parts),
            "language": info.language,
            "language_probability": round(info.language_probability, 3),
            "duration": round(info.duration, 2),
            "segments": segments,
        }

        print(json.dumps(result))

    except Exception as e:
        print(json.dumps({"error": str(e)}))
        sys.exit(1)


if __name__ == "__main__":
    main()
