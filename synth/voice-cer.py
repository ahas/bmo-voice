#!/usr/bin/env python
"""Measures how much of a spoken line a listener can take down from a voice.

The number is a character error rate: a line is spoken, a recogniser writes down what it hears,
and what it wrote is compared with the line that was asked for. A voice whose consonants have
fallen apart scores worse than one whose have not, which is how two voices — two checkpoints of
the trained voice — are put side by side.

Each line is drawn more than once, so that a single unlucky roll of the model's dice cannot
carry the figure. The sentences are read from a file and stand in kinds — numbers, crowded
consonants, an ordinary briefing line — because a voice that is clean on one kind is not thereby
clean on another, which is the whole reason this is measured over a set rather than over one line.

Beside it, each clip's ending: whether its last syllable died away or was cut off while it still
sounded (`ending_db`). The recogniser cannot say so — it completes a clipped last word from the
words before it — so the sound itself is measured.

The draws come from the trained voice on the voice server of the node `--node` names, drawn
through `scripts/voice-node.ts speak` the way a briefing reaches it, and are transcribed in this
process as they were spoken (docs/voice-plan.md, 「How intelligible the voice is」).

Beside them, for a line asked in English, how surely the recogniser hears English in the clip
(`english`): its own language guess, the share it gives English among every language it knows. A
voice speaking English with another language's accent is heard as English less surely, so it
tells the accent apart where the error rate alone does not.

usage: <venv>/bin/python synth/voice-cer.py --node <name> --checkpoint <voice folder>
       [--sentences <file.json>] [--language korean|english] [--draws 2] [--work <dir>]
       [--json <out.json>] [--style '<the voice row's delivery instruction>']
"""

import argparse
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

# What a Korean recogniser writes where a Korean voice said a number: the digits, and the unit
# as its own abbreviation. The line that was spoken has neither, because the reading gate hands
# the synthesiser a line already written the way it is said. So both sides are brought into the
# spoken spelling before they are compared — otherwise a perfect transcription of a percentage said in
# words scores as half wrong for having been written "87%", and the orthography would drown
# the thing being measured.
UNITS = [
    ("밀리초", ["ms", "밀리세컨드"]),
    ("기가바이트", ["GB", "gb"]),
    ("메가바이트", ["MB", "mb"]),
    ("퍼센트", ["%"]),
]
DIGITS = "영일이삼사오육칠팔구"
PLACES = [(10000, "만"), (1000, "천"), (100, "백"), (10, "십")]


def sino(number: int) -> str:
    """A number as Korean says it, which is how a spoken line spells it out."""
    if number == 0:
        return "영"

    said = ""

    for size, name in PLACES:
        count, number = divmod(number, size)

        if count:
            # 일십 and 일백 are not said; 이천 and 삼백 are.
            said += ("" if count == 1 else sino(count)) + name

    return said + (DIGITS[number] if number else "")


def spoken(text: str) -> str:
    """The line in the spelling a voice uses: units by name, numbers as words."""
    for name, written in UNITS:
        for form in written:
            text = text.replace(form, name)

    return re.sub(r"\d+", lambda hit: sino(int(hit.group())), text)


def bare(text: str) -> str:
    """What is left to compare: the letters and nothing else, in one case."""
    return re.sub(r"[^가-힣a-z]", "", spoken(text).lower())


def cer(asked: str, heard: str) -> float:
    """Levenshtein distance over the bare letters, against the length of what was asked."""
    reference, hypothesis = bare(asked), bare(heard)
    previous = list(range(len(hypothesis) + 1))

    for index, letter in enumerate(reference, 1):
        current = [index]

        for place, other in enumerate(hypothesis, 1):
            current.append(min(previous[place] + 1, current[place - 1] + 1, previous[place - 1] + (letter != other)))

        previous = current

    return previous[-1] / max(1, len(reference))


# How a clip's last syllable is told heard to its end from cut off while it still sounds. A voice
# that ends dies away: over its last 30 ms before it falls for good under -40 dB of its loudest it is
# already some 25 dB down. A cut one stops at full voice. A clip whose voice is still within this
# many dB of its loudest there ends cut (docs/voice-plan.md, 「How a line ends」).
QUIET_DB = -40.0
CUT_DB = -20.0


def ending_db(sound, rate: int) -> float:
    """The voice's level over its last 30 ms before it falls for good under `QUIET_DB`, in dB
    against the clip's loudest 10 ms. Near 0 for a voice cut off at full strength; far below
    `CUT_DB` for one that died away."""
    import numpy as np

    frame = rate // 100
    count = len(sound) // frame

    if count == 0:
        return float("-inf")

    levels = np.sqrt(np.mean(np.asarray(sound[: count * frame], dtype=np.float64).reshape(count, frame) ** 2, axis=1))
    loudest = float(levels.max())

    if loudest == 0.0:
        return float("-inf")

    sounding = np.nonzero(levels >= loudest * 10 ** (QUIET_DB / 20))[0]
    last = int(sounding[-1])

    return float(20 * np.log10(levels[max(0, last - 2): last + 1].max() / loudest))


def english_share(asr, sound, rate: int) -> float:
    """The share the recogniser's language guess gives English for the clip: its first decoding step
    after the start of a transcript, read over its language tokens alone."""
    import torch

    model = asr.model
    features = asr.feature_extractor(sound, sampling_rate=rate, return_tensors="pt").input_features
    features = features.to(model.device, dtype=model.dtype)
    start = asr.tokenizer.convert_tokens_to_ids("<|startoftranscript|>")
    languages = model.generation_config.lang_to_id

    with torch.no_grad():
        logits = model(input_features=features,
                       decoder_input_ids=torch.tensor([[start]], device=model.device)).logits[0, -1]

    ids = list(languages.values())
    shares = torch.softmax(logits[ids].float(), dim=-1)

    return float(shares[ids.index(languages["<|en|>"])])


def speak(args: argparse.Namespace, text: str, wav: Path, seed: int) -> None:
    """One draw of the trained voice from the voice server, through the node's own shell."""
    line = [
        "bun", "scripts/voice-node.ts", "speak", "--node", args.node,
        "--checkpoint", args.checkpoint, "--language", {"korean": "ko", "english": "en"}[args.language],
        "--style", args.style, "--seed", str(seed), "--text", text, "--wav", str(wav),
    ]
    done = subprocess.run(line, capture_output=True, text=True)

    if done.returncode != 0:
        raise SystemExit(f"voice-cer: the node ended {done.returncode}\n{done.stderr[-2000:]}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sentences", default="synth/voice-cer-sentences.json")
    parser.add_argument("--asr", default="openai/whisper-large-v3-turbo")
    parser.add_argument("--draws", type=int, default=2)
    parser.add_argument("--work", default="")
    parser.add_argument("--json", dest="record")
    # The trained voice is drawn from a voice server: the node it stands on by its name in the nodes
    # table, and the checkpoint and reference as names under its voices root.
    parser.add_argument("--node", required=True)
    parser.add_argument("--checkpoint", required=True)
    # The language the sentences are said in, as the recogniser names it.
    parser.add_argument("--language", choices=["korean", "english"], default="korean")
    # The delivery instruction the voice row carries, so a clip is drawn the way a briefing speaks it.
    parser.add_argument("--style", default="")
    args = parser.parse_args()

    sentences = json.loads(Path(args.sentences).read_text(encoding="utf-8"))
    room = Path(args.work or tempfile.mkdtemp(prefix="voice-cer-"))
    room.mkdir(parents=True, exist_ok=True)

    clips: list[dict] = []

    for sentence in sentences:
        for draw in range(1, args.draws + 1):
            wav = room / f"{sentence['id']}-{draw}.wav"
            print(f"voice-cer: spoken {wav.name}", file=sys.stderr, flush=True)
            speak(args, sentence["text"], wav, draw)
            clips.append({
                "id": sentence["id"], "kind": sentence["kind"], "draw": draw,
                "asked": sentence["text"], "wav": str(wav),
            })

    print(f"voice-cer: {len(clips)} clips to take down", file=sys.stderr, flush=True)

    import torch
    import soundfile as sf
    import torchaudio
    from transformers import pipeline

    asr = pipeline("automatic-speech-recognition", model=args.asr,
                   device="cuda" if torch.cuda.is_available() else "cpu",
                   dtype=torch.float16 if torch.cuda.is_available() else torch.float32)

    for index, clip in enumerate(clips, 1):
        sound, rate = sf.read(clip["wav"], dtype="float32", always_2d=False)

        if sound.ndim > 1:
            sound = sound.mean(axis=1)

        # A silent clip has no ending, and is written as none rather than as an infinity JSON cannot hold.
        ending = ending_db(sound, rate)
        clip["endingDb"] = None if ending == float("-inf") else round(ending, 1)

        if rate != 16000:
            sound = torchaudio.functional.resample(torch.from_numpy(sound), rate, 16000).numpy()
            rate = 16000

        heard = asr({"raw": sound, "sampling_rate": rate},
                    generate_kwargs={"language": args.language, "task": "transcribe"})["text"].strip()
        clip["heard"] = heard
        clip["cer"] = round(cer(clip["asked"], heard), 3)

        if args.language == "english":
            clip["english"] = round(english_share(asr, sound, rate), 3)

        print(f"voice-cer: {index}/{len(clips)} {clip['id']}-{clip['draw']} "
              f"CER {clip['cer']:.3f} · ending {clip['endingDb']} dB · English {clip.get('english')} · {heard}",
              file=sys.stderr, flush=True)

    def mean(of: list[dict]) -> float:
        return round(sum(clip["cer"] for clip in of) / max(1, len(of)), 3)

    kinds = sorted({sentence["kind"] for sentence in sentences})
    summary = {
        "clips": len(clips), "sentences": len(sentences), "draws": args.draws, "asr": args.asr,
        "voice": f"{args.checkpoint} on {args.node}",
        "style": args.style,
        "overall": mean(clips),
        "byKind": {kind: mean([c for c in clips if c["kind"] == kind]) for kind in kinds},
        "worst": max([c["cer"] for c in clips], default=0.0),
        "language": args.language,
        # How surely English was heard, on average, in a line asked in English.
        "english": round(sum(c["english"] for c in clips) / len(clips), 3) if clips and "english" in clips[0] else None,
        # How many clips end on a last syllable cut off while it still sounds.
        "cutEndings": sum(clip["endingDb"] is not None and clip["endingDb"] > CUT_DB for clip in clips),
    }

    print(json.dumps({"summary": summary, "clips": clips}, ensure_ascii=False, indent=2))

    if args.record:
        Path(args.record).write_text(
            json.dumps({"summary": summary, "clips": clips}, ensure_ascii=False, indent=2),
            encoding="utf-8",
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
