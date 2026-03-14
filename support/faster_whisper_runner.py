#!/usr/bin/env python3

import argparse
import json
import sys


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--file", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--device", default="auto")
    parser.add_argument("--compute-type", default="auto")
    parser.add_argument("--language")
    parser.add_argument("--vad-filter", action="store_true")
    parser.add_argument("--initial-prompt")
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    try:
        from faster_whisper import WhisperModel
    except Exception as exc:
        print(json.dumps({"error": f"failed to import faster_whisper: {exc}"}))
        return 1

    model_kwargs = {}
    if args.device and args.device != "auto":
        model_kwargs["device"] = args.device
    if args.compute_type and args.compute_type != "auto":
        model_kwargs["compute_type"] = args.compute_type

    transcribe_kwargs = {
        "vad_filter": bool(args.vad_filter),
    }
    if args.language and args.language.lower() != "auto":
        transcribe_kwargs["language"] = args.language
    if args.initial_prompt:
        transcribe_kwargs["initial_prompt"] = args.initial_prompt

    try:
        model = WhisperModel(args.model, **model_kwargs)
        segments, info = model.transcribe(args.file, **transcribe_kwargs)
        segment_list = list(segments)
        text = " ".join(segment.text.strip() for segment in segment_list if segment.text.strip())
        print(
            json.dumps(
                {
                    "text": text.strip(),
                    "language": getattr(info, "language", None),
                }
            )
        )
        return 0
    except Exception as exc:
        print(json.dumps({"error": str(exc)}))
        return 1


if __name__ == "__main__":
    sys.exit(main())
