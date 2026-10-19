#!/usr/bin/env python
"""Writes a spoken wav as OGG/Opus, the form a Telegram voice message takes.

Every spoken sound ends here, whichever machine spoke it: the trained voice's mouth
(synth/voice-vox.py) speaks a wav, on this machine's card or on a voice node's, and this file
encodes it. It loads no model.

It runs on the voice environment's interpreter, a virtualenv outside the repository
(docs/voice-plan.md, 「One voice environment」), never the machine's own python.

libsndfile writes OGG/Opus on its own, so soundfile does the encoding here and the voice needs no
ffmpeg or sox wherever it is spoken.

What it is given is what it writes: the waveform is only kept under the peak ceiling on its way
into the encoder, and nothing is laid over the voice itself (docs/voice-plan.md, 「What reaches the
room」).

usage: <venv>/bin/python synth/voice-encode.py --from-wav <in.wav> --out <path.ogg> [--wav <path.wav>]
"""

import argparse
import sys

import numpy as np
import soundfile as sf

# What the whole thing is kept under: a voice message that clips is worse than a quiet one.
PEAK = 0.95


def under_peak(audio: np.ndarray) -> np.ndarray:
    """The waveform as it came, lowered only where its peak would stand over the ceiling.

    It is the one step between the mouth and the encoder. The level is otherwise the mouth's own —
    it already matches its output to the ceiling — so a sound that arrives under it leaves exactly
    as it arrived.
    """
    peak = float(np.max(np.abs(audio))) if len(audio) else 0.0

    if peak > PEAK:
        audio = audio / peak * PEAK

    return audio.astype(np.float32)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--from-wav", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--wav")
    args = parser.parse_args()

    audio, sr = sf.read(args.from_wav, dtype="float32", always_2d=False)

    if audio.ndim > 1:
        audio = audio.mean(axis=1).astype(np.float32)

    audio = under_peak(audio)

    # OGG/Opus is what a Telegram voice message is; the WAV is only written when asked for, for
    # listening to on this machine without a player that knows Opus.
    sf.write(args.out, audio, sr, format="OGG", subtype="OPUS")

    if args.wav:
        sf.write(args.wav, audio, sr, subtype="PCM_16")

    print(f"voice-encode: {len(audio) / sr:.1f}s · {args.out}", file=sys.stderr)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
