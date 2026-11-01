#!/usr/bin/env python
"""What a listener takes down from spoken lines: Whisper large-v3-turbo on each file, in one language.

A continuation prompt is checked with it before it is installed (docs/voice-plan.md, 「How a prompt is
checked before it is installed」): the lines spoken on from the clip are taken down here, and the
judging is the caller's. Each file is written out as one line of JSON on standing output, in the
order given, as `{"file": <path>, "heard": <text>}`, so a caller reads the answers as they come.

It runs on the voice environment's interpreter (docs/voice-plan.md, 「The environments」).

usage: <venv>/bin/python synth/voice-transcribe.py <ko|en> <file>...
"""

import json
import sys

LANGUAGES = {"ko": "korean", "en": "english"}
MODEL = "openai/whisper-large-v3-turbo"


def main() -> int:
    if len(sys.argv) < 3 or sys.argv[1] not in LANGUAGES:
        print("usage: voice-transcribe.py <ko|en> <file>...", file=sys.stderr)
        return 1

    language, files = LANGUAGES[sys.argv[1]], sys.argv[2:]

    import torch
    import soundfile as sf
    import torchaudio
    from transformers import pipeline

    cuda = torch.cuda.is_available()
    asr = pipeline("automatic-speech-recognition", model=MODEL, device="cuda" if cuda else "cpu",
                   dtype=torch.float16 if cuda else torch.float32)

    for file in files:
        sound, rate = sf.read(file, dtype="float32", always_2d=False)

        if sound.ndim > 1:
            sound = sound.mean(axis=1)

        if rate != 16000:
            sound = torchaudio.functional.resample(torch.from_numpy(sound), rate, 16000).numpy()
            rate = 16000

        # Timestamps let the recogniser read past thirty seconds, which a line gone wrong can run to.
        heard = asr({"raw": sound, "sampling_rate": rate}, return_timestamps=True,
                    generate_kwargs={"language": language, "task": "transcribe"})["text"].strip()
        print(json.dumps({"file": file, "heard": heard}, ensure_ascii=False), flush=True)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
