#!/usr/bin/env python
"""What a listener takes down from spoken lines: Whisper large-v3-turbo on each file, in one language.

A continuation prompt is checked with it before it is installed (docs/voice-plan.md, 「How a prompt is
checked before it is installed」): the lines spoken on from the clip are taken down here, and the
judging is the caller's. Each file is written out as one line of JSON on standing output, in the
order given, as `{"file": <path>, "heard": <text>}`, so a caller reads the answers as they come.

Whisper runs on the card only while the card lock is free for it, and on the processor otherwise or once the card
runs out of memory (docs/voice-plan.md, 「Where a transcription runs」).

It runs on the voice environment's interpreter (docs/voice-plan.md, 「The environments」).

usage: <venv>/bin/python synth/voice-transcribe.py <ko|en> <file>...
"""

import fcntl
import gc
import json
import os
import sys
from pathlib import Path

LANGUAGES = {"ko": "korean", "en": "english"}
MODEL = "openai/whisper-large-v3-turbo"
# The rate Whisper hears at; a file at another is resampled to it.
RATE = 16000
# The node's card lock, shared by every service that puts work on the card (docs/voice-plan.md, 「How it holds the
# card」).
CARD_LOCK = "~/.gpu.lock"


def take_card(path: Path) -> int | None:
    """The card lock, taken without waiting: its handle, or None where another occupant holds it or it cannot be
    opened."""
    try:
        handle = os.open(path, os.O_RDWR | os.O_CREAT, 0o644)
    except OSError:
        return None

    try:
        fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        os.close(handle)
        return None

    return handle


class Hearer:
    """Whisper on the card while the card lock is the transcriber's, and on the processor otherwise."""

    def __init__(self, torch, pipeline, language: str) -> None:
        self.torch = torch
        self.pipeline = pipeline
        self.language = language
        self.card = take_card(Path(CARD_LOCK).expanduser()) if torch.cuda.is_available() else None
        self.asr = None

        if self.card is not None:
            try:
                self.asr = self.load("cuda", torch.float16)
            except torch.cuda.OutOfMemoryError:
                pass

        if self.asr is None:
            self.leave_card()
            self.asr = self.load("cpu", torch.float32)

    def __enter__(self) -> "Hearer":
        return self

    def __exit__(self, *_) -> None:
        self.leave_card()

    def load(self, device: str, dtype):
        return self.pipeline("automatic-speech-recognition", model=MODEL, device=device, dtype=dtype)

    def hear(self, sound) -> str:
        """One file's words, heard again on the processor where the card runs out of memory under it."""
        try:
            return self.take_down(sound)
        except self.torch.cuda.OutOfMemoryError:
            if self.card is None:
                raise

        # Outside the handler, so the traceback no longer holds the tensors the card is to be emptied of.
        self.leave_card()
        self.asr = self.load("cpu", self.torch.float32)
        return self.take_down(sound)

    def take_down(self, sound) -> str:
        # Timestamps let the recogniser read past thirty seconds, which a line gone wrong can run to.
        return self.asr({"raw": sound, "sampling_rate": RATE}, return_timestamps=True,
                        generate_kwargs={"language": self.language, "task": "transcribe"})["text"].strip()

    def leave_card(self) -> None:
        """The model off the card and the allocator's cache emptied, then the lock given back."""
        if self.card is None:
            return

        self.asr = None
        gc.collect()
        self.torch.cuda.empty_cache()
        os.close(self.card)
        self.card = None


def main() -> int:
    if len(sys.argv) < 3 or sys.argv[1] not in LANGUAGES:
        print("usage: voice-transcribe.py <ko|en> <file>...", file=sys.stderr)
        return 1

    language, files = LANGUAGES[sys.argv[1]], sys.argv[2:]

    import torch
    import soundfile as sf
    import torchaudio
    from transformers import pipeline

    with Hearer(torch, pipeline, language) as hearer:
        for file in files:
            sound, rate = sf.read(file, dtype="float32", always_2d=False)

            if sound.ndim > 1:
                sound = sound.mean(axis=1)

            if rate != RATE:
                sound = torchaudio.functional.resample(torch.from_numpy(sound), rate, RATE).numpy()

            print(json.dumps({"file": file, "heard": hearer.hear(sound)}, ensure_ascii=False), flush=True)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
