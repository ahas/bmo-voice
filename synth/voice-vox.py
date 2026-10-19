#!/usr/bin/env python
"""Speaks a line in an agent's own trained voice, its Korean in Korean and its English in English.

The voice is a LoRA fine-tune of VoxCPM2 on the agent's own recorded lines, and what picks a run's
weights is the checkpoint directory named by the caller — never a table this file reads for
itself, because the row was chosen by the caller and a mouth that went looking for its own
settings could speak in a voice nobody asked for.

It works three ways; the first two speak through the same `Mouth`, and the third asks the second:

- **Once** (the default): speak one line on this machine, write a wav, and hand that wav to
  `voice-encode.py`, in a process of its own, for the OGG/Opus encode.
- **As a server** (`--serve`): the voice service on each node that lends its card (docs/voice-plan.md,
  「The voice servers on the voice nodes」), listening on the node's loopback. It answers `POST /speak`
  with the spoken line as a wav and `GET /health` with what it holds. The card is shared under the
  node's shared GPU lock (`Card`): the model stays on the card between lines until another occupant
  asks for it, and waits in page-locked host memory while it is off. Encoding stays on the caller's
  side, where the encoder already lives.
- **Asking** (`--ask`): the server's own client, run on the node beside it, which is how a call from
  the operations machine reaches a server nothing outside the node can reach. It posts the request file it is
  given and removes it, writes the wav the server answered, and prints one line of JSON: the status,
  the server's reason where it refused, and the answer's account of the line. With no request it
  asks `/health` and prints what the server holds.

It runs on the voice environment's interpreter (docs/voice-plan.md, 「One voice environment」), the
one `synth/voice-encode.py` runs on too, unless the caller names another. The OGG/Opus encode exists
once, in that file, which has the tests that hold it, and this file is only a mouth.

The peak is matched to the encoder's ceiling before the wav leaves. The model returns a waveform
peaking far below full scale, and the encoder only ever lowers a peak rather than lifting one — so a
line handed over as it came back would arrive in the room roughly sixteen decibels quieter than full
scale allows, for no reason anybody chose.

A voice may carry a delivery instruction — the calm, steady, restrained speech its training run
settled on — and this file is the one that knows how VoxCPM2 reads one: in brackets, before the
words, never spoken aloud. The caller hands the instruction over as plain words and the mouth puts
it where the model looks for it, so the model's own convention stays in the file that loads it.

usage: <venv>/bin/python synth/voice-vox.py --text '<words>' --out <path.ogg>
       --checkpoint <dir> --reference <ref.wav> [--cfg 1.5] [--steps 16] [--style '<instruction>']
       [--encoder-python <path>] [--wav <path.wav>] [--seed 0] [--sentences]
       [--prompt <ko|en> <clip.wav> '<words>']...
       <venv>/bin/python synth/voice-vox.py --serve --voices <dir> --token-file <path>
       [--host 127.0.0.1] [--port 7432] [--gpu-lock ~/.gpu.lock] [--gpu-wait 60]
       <venv>/bin/python synth/voice-vox.py --ask --token-file <path> [--port 7432]
       [--request <request.json> --wav <path.wav>] [--timeout 600]
"""

import argparse
import ctypes
import fcntl
import hmac
import io
import json
import os
import random
import re
import socket
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import numpy as np
import soundfile as sf

# What the whole thing is kept under. It is the same number `synth/voice-encode.py` holds its
# ceiling at, and it is here for the same reason: a voice message that clips is worse than a quiet
# one, and one that arrives quiet for no reason is worse than either.
PEAK = 0.95

# How the line is spoken, when nobody says otherwise. Both come from the recorded selection of
# this training run rather than from the older example scripts beside it: the run's own note says
# the priorities moved to a lower, steadier delivery at these two settings, spoken under a delivery
# instruction the voice carries with it (`delivery`), and the scripts that still pass the older
# pair were written before that.
DEFAULT_CFG = 1.5
DEFAULT_STEPS = 16

# What the model is asked for beyond the two above. A voice not spoken a sentence at a time speaks
# a briefing in passes of up to `MOST_PASS` characters, so the ceiling is the generous one the
# newest example script uses; a bad case is not retried, because a retry would speak a second time
# on a card another briefing is queued for.
MAX_LEN = 1800
RETRY_BADCASE = False

# The most characters of one language spoken in one pass where the voice does not speak a sentence
# at a time (docs/voice-plan.md, 「What a long line takes of the card」). The audio decoder turns a
# whole pass into its waveform at once, and what it takes of the card grows with the pass's seconds,
# so a longer stretch is spoken in passes of whole sentences that each stay within this many.
MOST_PASS = 240

# The languages a voice may carry a continuation prompt for: the two a line's stretches are told
# apart as (`stretches`). A prompt is a clip of the voice speaking that language and the words it
# says, and both are the voices row's, handed over by the caller. Where a voice has one, every piece
# in that language is spoken as what comes next after that clip, and carries the clip's delivery and
# its clean pauses rather than rolling a delivery of its own (docs/voice-plan.md, 「What steadies the
# tone」). Where it has none, a piece in that language is spoken from the reference alone — an English
# piece never on from the Korean clip, which pulls its accent toward Korean (docs/voice-plan.md, 「How
# English is spoken」).
LANGUAGES = ("ko", "en")

# How a line is told apart into its Korean and its English (docs/voice-plan.md, 「Which sentences are
# English」). The runner reads the same rule to decide what the reading gate is given, and the case
# table `voice-language-cases.json` is checked against both.
HANGUL = re.compile(r"[가-힣ㄱ-ㅎㅏ-ㅣ]")
ROMAN = re.compile(r"[A-Za-z]")
WORD = re.compile(r"[A-Za-z']+")
LOWER = re.compile(r"[a-z]")
SENTENCE_END = re.compile(r"(?<=[.?!…])\s+|\n+")

# A voice whose row asks for it speaks a line a sentence at a time, each on from its continuation
# prompt: one pass of a long line lays a scratchy layer over the vowels that grows toward its end,
# and a sentence ends before it grows (docs/voice-plan.md, 「What keeps a long line clean」). This
# many seconds of silence stand between two sentences, and between a line's Korean and its English
# however the row speaks.
SENTENCE_PAUSE = 0.3

# How long each edge of a spoken piece is faded over, in seconds (docs/voice-plan.md, 「How a piece
# ends and meets the next」). Every sentence of the line it was measured on opens on at least 135 ms
# of the model's own quiet and, with its stop held (`HeldStop`), ends on the silence of the held
# step, so this much takes nothing from a first consonant or a last vowel the model spoke.
EDGE_FADE = 0.01

# Which of the stop head's two answers is which: the package reads its vote as the argmax of the two.
GO, STOP = 0, 1

# The line a freshly loaded model speaks and throws away (`Mouth.warm`), and the ceiling that keeps
# it to a second or two of the card.
WARM_LINE = "준비됐어."
WARM_LEN = 100

# The server's own limits. A line longer than this is not a briefing anybody asked for, and a body
# is read whole into memory, so it is bounded before it is read.
MOST_TEXT = 4000
MOST_BODY = 64 * 1024
# A delivery instruction is a sentence, not a script: anything longer is a mistake in a row.
MOST_STYLE = 300

# What loading the model takes of the card: the server's process peaks at about 6.9 GiB with the
# model loaded and a line spoken on an RTX 3060 Ti (7.1 GB held on an RTX 4070), and an 8 GB card has about
# 7.4 GiB free beside another service's idle CUDA context. A card with less free than this cannot take
# a load, so the health says so and a caller with another node to choose takes that one.
LOAD_BYTES = int(7.2 * 1024**3)

# What putting the kept weights on the card takes of it for one line, once the server's own CUDA
# context is already there: a line takes the server to 6,908 MiB on an RTX 3060 Ti, its own 194 MiB
# context among them, and reserves at most about 6.6 GB on an RTX 4070. An 8 GB card beside two other
# services' idle contexts has 6.98 GiB free, which a line fits in, so the bar stands below it.
UP_BYTES = int(6.8 * 1024**3)

# The node's shared GPU lock: every service that puts work on the node's card holds this one file
# under flock for as long as its work is there, so the voice, the gate model and the song pipeline
# take the card in turn rather than all at once. A node where nothing else keeps the convention
# holds the file all the same, and the voice takes it unopposed.
GPU_LOCK = "~/.gpu.lock"

# How long a line waits for other work to let go of the card, in seconds. The caller has already
# waited for a node whose health said the card was free, so this covers only another service taking
# it in the moment between; work that holds it longer is told to the caller as the card taken.
GPU_WAIT = 60.0


def card_free_bytes() -> int | None:
    """The card's free memory as the driver counts it, every process on it included, or None where
    the driver cannot be asked. It is asked of nvidia-smi rather than torch, so asking opens no CUDA
    context the server would then hold on a card it shares."""
    try:
        ran = subprocess.run(
            ["nvidia-smi", "--id=0", "--query-gpu=memory.free", "--format=csv,noheader,nounits"],
            capture_output=True, text=True, timeout=5,
        )
        return int(ran.stdout.strip().splitlines()[0]) * 1024 * 1024 if ran.returncode == 0 else None
    except (OSError, subprocess.TimeoutExpired, ValueError, IndexError):
        return None


def delivery(style) -> str:
    """The instruction as it may be read, or a refusal saying why it may not.

    A bracket inside it would close the instruction early and have the rest spoken aloud as part of
    the line, so an instruction carrying one is refused rather than spoken wrong.
    """
    if style is None:
        return ""

    if not isinstance(style, str) or len(style) > MOST_STYLE or "(" in style or ")" in style:
        raise ValueError(f"a style is plain words of at most {MOST_STYLE} characters, without brackets")

    return style.strip()


def styled(style: str, text: str) -> str:
    """The instruction as VoxCPM2 reads it: in brackets before the first words of the text it is
    given. That is the piece where the piece is spoken from the reference alone, and the prompt's own
    words where it is spoken on from a prompt: the model reads the prompt's words and the piece as one
    text, so an instruction put between the two is read aloud as part of the line."""
    return f"({style}){text}" if style else text


def continuation(given, place) -> dict[str, tuple[Path, str]]:
    """The voice's continuation prompts by language, each as the clip and its words, out of what the
    caller handed over: per language a `clip`, made a path by `place`, and the `words` it says. A
    language the voice has none for is not in it.

    A clip without its words, words without their clip, or a clip `place` finds no file for is a
    prompt half there, and is refused rather than spoken without: a clip read against the wrong
    words, or none, speaks the line wrong.
    """
    if given is None:
        return {}

    if not isinstance(given, dict):
        raise ValueError("prompts are a clip and its words by language")

    prompts: dict[str, tuple[Path, str]] = {}

    for language, prompt in given.items():
        if language not in LANGUAGES:
            raise ValueError(f"a continuation prompt is in one of {', '.join(LANGUAGES)}, not {language}")

        clip = prompt.get("clip") if isinstance(prompt, dict) else None
        words = prompt.get("words") if isinstance(prompt, dict) else None
        path = place(clip) if isinstance(clip, str) and clip != "" else None
        said = words.strip() if isinstance(words, str) else ""

        if path is None or not path.exists() or said == "":
            raise ValueError(f"the {language} continuation prompt is a clip that is there and the words it says, both")

        prompts[language] = (path, said)

    return prompts


def sentence_spans(text: str) -> list[tuple[int, int]]:
    """Where each sentence of the line starts and ends, its surrounding spaces left out.

    A sentence ends at a full stop, a question or exclamation mark or an ellipsis before a space, and
    at every line break.
    """
    spans, start = [], 0

    for gap in [*SENTENCE_END.finditer(text), None]:
        end = len(text) if gap is None else gap.start()
        piece = text[start:end]

        if piece.strip():
            lead = len(piece) - len(piece.lstrip())
            spans.append((start + lead, start + len(piece.rstrip())))

        start = end if gap is None else gap.end()

    return spans


def sentences(text: str) -> list[str]:
    """The line as the sentences a voice speaks it in when it speaks one at a time.

    Two short sentences are not joined, because a piece of two already carries the growth a sentence
    alone ends before; a long one is never cut, because a cut inside it would end its first half on
    a sentence's closing fall.
    """
    return [text[start:end] for start, end in sentence_spans(text)]


def passes(text: str) -> list[str]:
    """The line as the passes a voice speaks it in when it does not speak a sentence at a time: the
    whole line where it is within `MOST_PASS` characters, and otherwise its sentences gathered in
    order into passes each as long as it can be within them, cut out of the line as they stand.

    A sentence longer than `MOST_PASS` is a pass of its own and is never cut, for the same reason
    `sentences` never cuts one.
    """
    if len(text) <= MOST_PASS:
        return [text]

    spans: list[tuple[int, int]] = []

    for start, end in sentence_spans(text):
        if spans and end - spans[-1][0] <= MOST_PASS:
            spans[-1] = (spans[-1][0], end)
        else:
            spans.append((start, end))

    return [text[start:end] for start, end in spans]


def said_language(sentence: str) -> str | None:
    """Korean ("ko") for a sentence holding a Hangul syllable, English ("en") for one holding none
    with two words or more that have a lower-case letter, and None for one that says nothing by
    itself — a greeting like "Yay, hello!", a model name, a figure."""
    if HANGUL.search(sentence):
        return "ko"

    if sum(1 for word in WORD.findall(sentence) if LOWER.search(word)) >= 2:
        return "en"

    return None


def stretches(text: str) -> list[tuple[str, str]]:
    """The line as its stretches of one language each, in order, as (language, words).

    A sentence that says nothing by itself takes the language of the next one that does, or of the
    last one before it where none follows; a line where none does is English if it has a roman
    letter and Korean otherwise. Neighbouring sentences of one language are one stretch, cut out of
    the line as they stand.
    """
    spans = sentence_spans(text)
    said = [said_language(text[start:end]) for start, end in spans]
    fallback = "en" if ROMAN.search(text) else "ko"
    languages = [
        known
        or next((after for after in said[at + 1:] if after is not None), None)
        or next((before for before in reversed(said[:at]) if before is not None), fallback)
        for at, known in enumerate(said)
    ]
    out: list[tuple[str, int, int]] = []

    for language, (start, end) in zip(languages, spans):
        if out and out[-1][0] == language:
            out[-1] = (language, out[-1][1], end)
        else:
            out.append((language, start, end))

    return [(language, text[start:end]) for language, start, end in out]


def faded(audio: np.ndarray, rate: int) -> np.ndarray:
    """The piece with both edges faded over `EDGE_FADE` on a raised-cosine curve, so neither steps
    between the model's noise floor and the digital silence beside it. A piece shorter than two fades
    is faded over half its length at each end."""
    audio = np.array(audio, dtype=np.float32)
    span = min(int(EDGE_FADE * rate), len(audio) // 2)

    if span == 0:
        return audio

    rise = (0.5 - 0.5 * np.cos(np.pi * (np.arange(span) + 0.5) / span)).astype(np.float32)
    audio[:span] *= rise
    audio[-span:] *= rise[::-1]

    return audio


class HeldStop:
    """The model's stop held for one step, as a forward hook on its stop head.

    VoxCPM2 ends a piece on the first step its stop head votes to stop, and a vote cast while the voice
    still sounds would end the piece on it. Armed, the hook answers that first vote with go, so the
    model speaks one step more, then answers stop on the step after whatever the head says, so the
    piece does not run on into silence the model would otherwise go on generating. A last syllable the
    model itself cuts short is not this hook's to mend: it is cut and gone silent before the vote
    (docs/voice-plan.md, 「How a line ends」). Every step before the vote is left as
    the model decided it, so the delivery a seed names is unchanged. Unarmed it passes every vote
    through, which is how the throwaway line after a load is spoken.

    The hook is the one place the package lets that vote be read and answered: it decides inside its
    own generation loop and hands back only the finished waveform.
    """

    def __init__(self) -> None:
        # None while unarmed, False while waiting for the first vote, True for the held step.
        self.voted: bool | None = None

    def arm(self) -> None:
        """Hold the next piece's stop; asked before every call that speaks a piece."""
        self.voted = False

    def __call__(self, _module, _inputs, logits):
        if self.voted is None:
            return logits

        answer = logits.clone()

        if self.voted:
            self.voted = None
            answer[..., STOP] = answer[..., GO] + 1

            return answer

        if int(logits.argmax(dim=-1)[0]) != STOP:
            return logits

        self.voted = True
        answer[..., STOP] = answer[..., GO] - 1

        return answer


def local_base(named: str) -> str:
    """The base model a checkpoint names, as it stands on this machine.

    A checkpoint's config names the snapshot by the path it had in the Hugging Face cache of the
    account that trained it. A machine whose home is elsewhere holds the same repository and revision
    in its own cache, and is asked for that instead of a path it never had.
    """
    if Path(named).is_dir():
        return named

    found = re.search(r"models--([^/]+)--([^/]+)/snapshots/([0-9a-f]+)", named)

    if found is None:
        return named

    from huggingface_hub import snapshot_download

    return snapshot_download(f"{found[1]}/{found[2]}", revision=found[3], local_files_only=True)


class Mouth:
    """VoxCPM2 with one LoRA on it, loaded on first use and swapped rather than reloaded.

    The base model is what costs the load; the LoRA weights are a few megabytes. So a call for
    another checkpoint trained on the same base with the same LoRA shape only swaps the weights,
    which is what lets two checkpoints be heard side by side without loading the base twice.

    Loaded, the model can be put down (`down`): every tensor it holds moved into page-locked host
    memory and the card given back, and put up again (`up`) for the next line at the speed of the
    bus rather than of the disk. The model is the same objects throughout; only where their storage
    stands changes.

    torch and voxcpm are imported inside rather than at the top of the file because they are what
    makes starting slow, and neither the encode hand-off nor the server's own start needs them.
    """

    def __init__(self) -> None:
        self.model = None
        self.shape: str | None = None
        self.checkpoint: Path | None = None
        self.stop = HeldStop()
        # Every tensor the model holds, with its host copy, by the tensor's identity; the tensor itself
        # is kept beside its copy, so the identity is never reused while the entry stands.
        self.kept: dict[int, tuple] = {}
        self.on_card = False

    def ready(self, checkpoint: Path) -> float:
        """The model holding these weights; answers the seconds it took to get there."""
        import torch
        from voxcpm import VoxCPM
        from voxcpm.model.voxcpm2 import LoRAConfig

        started = time.monotonic()
        lora = json.loads((checkpoint / "lora_config.json").read_text(encoding="utf-8"))
        shape = json.dumps([lora["base_model"], lora["lora_config"]], sort_keys=True)

        if self.model is None or self.shape != shape:
            self.release()
            self.model = VoxCPM.from_pretrained(
                local_base(lora["base_model"]),
                load_denoiser=False,
                optimize=False,
                device="cuda" if torch.cuda.is_available() else "cpu",
                lora_config=LoRAConfig(**lora["lora_config"]),
                lora_weights_path=str(checkpoint),
            )
            self.shape = shape
            self.on_card = True
            self.stop = HeldStop()
            self.model.tts_model.stop_head.register_forward_hook(self.stop)
            self.warm()
        elif self.checkpoint != checkpoint:
            self.model.load_lora(str(checkpoint))

        self.checkpoint = checkpoint

        return time.monotonic() - started

    def warm(self) -> None:
        """One throwaway line spoken on a freshly loaded model before any line anybody asked for.

        The first line a loaded model speaks does not follow its seed: the same line, seed and
        settings come out as another melody and another length on the first call than on every call
        after it, while every call after the first gives back the same samples. A voice's seed is
        meant to name the one delivery chosen by ear, so a line is never the first one spoken.
        """
        import torch

        with torch.inference_mode():
            self.model.generate(
                text=WARM_LINE,
                cfg_value=DEFAULT_CFG,
                inference_timesteps=DEFAULT_STEPS,
                max_len=WARM_LEN,
                retry_badcase=False,
            )

    def speak(
        self, checkpoint: Path, reference: Path, text: str, cfg: float, steps: int, seed: int, style: str = "",
        prompts: dict[str, tuple[Path, str]] | None = None, by_sentence: bool = False,
    ):
        """The line, in the voice those weights carry and the delivery it is asked for, peak-matched,
        at the model's own rate — its stretches of one language (`stretches`) spoken apart, each on
        from the voice's continuation prompt in that language where it has one, and each a sentence at
        a time (`sentences`) where asked, in passes the card holds otherwise (`passes`). Each piece
        runs one step past the model's stop (`HeldStop`) and is faded at both edges (`faded`), and a
        piece the card runs out of memory for is spoken once more as its sentences (`spoken`).

        Every sentence is spoken on the same seed, so its delivery follows from its own words and the
        same line gives back the same samples every time. What makes the sentences one voice is the
        prompt: each carries the clip's delivery, where from the reference alone each would roll a
        delivery of its own and the line would change tone at every pause.

        The seed is set on the generators rather than passed to the call. The installed package's
        `generate` takes no `seed` — only the training checkout's copy does — so asking for one by
        name is a TypeError on the interpreter this actually runs on, and seeding the generators is
        the same determinism by the road that exists.
        """
        self.ready(checkpoint)
        prompts = prompts or {}
        rate = self.model.tts_model.sample_rate
        pause = np.zeros(int(SENTENCE_PAUSE * rate), dtype=np.float32)
        joined: list[np.ndarray] = []
        pieces = [
            (language, piece)
            for language, stretch in stretches(text)
            for piece in (sentences(stretch) if by_sentence else passes(stretch))
        ]

        for language, piece in pieces:
            for wav in self.spoken(piece, lambda words: self.piece(words, reference, cfg, steps, seed, style, prompts.get(language))):
                if joined:
                    joined.append(pause)

                joined.append(faded(wav, rate))

        audio = np.concatenate(joined)
        peak = float(np.max(np.abs(audio))) or 1.0

        return (audio / peak * PEAK).astype(np.float32), rate

    def spoken(self, piece: str, say) -> list:
        """The piece as `say` speaks it, as one wav; or, where the card runs out of memory for it, its
        sentences spoken one by one once the allocator's cache is emptied. A second shortfall is the
        line's failure (docs/voice-plan.md, 「What a long line takes of the card」)."""
        import gc

        import torch

        try:
            return [say(piece)]
        except RuntimeError as failure:
            # The audio decoder's layers are TorchScript, which raises the allocator's shortfall again as
            # a plain RuntimeError carrying only its message, so a shortfall is known by that message.
            # The cache is emptied only once this block has ended: until then the failure's frames hold
            # every tensor the pass had made.
            if not isinstance(failure, torch.cuda.OutOfMemoryError) and "CUDA out of memory" not in str(failure):
                raise

        print("voice-vox: the card ran out of memory for a piece, so it is spoken again by its sentences", file=sys.stderr, flush=True)
        gc.collect()
        torch.cuda.empty_cache()

        return [say(sentence) for sentence in sentences(piece)]

    def piece(
        self, words: str, reference: Path, cfg: float, steps: int, seed: int, style: str, prompt: tuple[Path, str] | None,
    ):
        """One call to the model on the line's seed, on from the continuation prompt where there is one."""
        import torch

        continuing = {} if prompt is None else {"prompt_wav_path": str(prompt[0]), "prompt_text": styled(style, prompt[1])}
        random.seed(seed)
        np.random.seed(seed)
        torch.manual_seed(seed)
        self.stop.arm()

        with torch.inference_mode():
            return self.model.generate(
                text=styled(style, words) if prompt is None else words,
                reference_wav_path=str(reference),
                cfg_value=cfg,
                inference_timesteps=steps,
                max_len=MAX_LEN,
                retry_badcase=RETRY_BADCASE,
                **continuing,
            )

    def held(self):
        """Every tensor the loaded model holds, each once: its parameters, its buffers, and the
        tensors its modules keep as plain attributes on objects of their own — the language models'
        key-value caches are such."""
        import torch

        seen: set[int] = set()

        for module in self.model.tts_model.modules():
            owned = [*module._parameters.values(), *module._buffers.values()]

            for value in vars(module).values():
                if isinstance(value, torch.Tensor):
                    owned.append(value)
                elif not isinstance(value, (torch.nn.Module, dict, list, tuple, str)) and hasattr(value, "__dict__"):
                    owned.extend(inner for inner in vars(value).values() if isinstance(inner, torch.Tensor))

            for tensor in owned:
                if tensor is not None and id(tensor) not in seen:
                    seen.add(id(tensor))
                    yield tensor

    def down(self) -> None:
        """The model put in page-locked host memory and the card given back.

        The first time a tensor is put down its host copy is made from the card; after that the copy
        already holds what the card held, since speaking changes no weight and a LoRA swap is made
        while the model is down, straight into the copies. The key-value caches are scratch every
        line writes before it reads.

        Some tensors a line makes anew: the audio decoder's weight-normed layers compute their
        weight afresh on every call and hang it on the layer in place of the last one. So a tensor
        the model no longer holds is let go with its copy, and one it holds for the first time is
        copied like any other. Whatever the allocator still caches — a load that failed half way
        included — is emptied however the model stood, and the C heap is trimmed: a load leaves about
        1.5 GB there that the process would otherwise hold for as long as it runs.
        """
        import torch

        if self.model is not None and self.on_card:
            held = list(self.held())
            holding = {id(tensor) for tensor in held}
            self.kept = {key: kept for key, kept in self.kept.items() if key in holding}

            for tensor in held:
                if not tensor.is_cuda:
                    continue

                kept = self.kept.get(id(tensor))

                if kept is None:
                    kept = (tensor, torch.empty_like(tensor, device="cpu", pin_memory=True).copy_(tensor))
                    self.kept[id(tensor)] = kept

                tensor.data = kept[1]

        self.on_card = False

        if torch.cuda.is_available():
            torch.cuda.empty_cache()

        try:
            ctypes.CDLL("libc.so.6").malloc_trim(0)
        except (OSError, AttributeError):
            pass  # not glibc: nothing to trim this way

    def up(self) -> float:
        """The model put back on the card from its host copies; answers the seconds it took."""
        import torch

        started = time.monotonic()

        if self.model is not None and not self.on_card:
            for tensor, host in self.kept.values():
                tensor.data = host.to("cuda", non_blocking=True)

            torch.cuda.synchronize()
            self.on_card = True

        return time.monotonic() - started

    def kept_bytes(self) -> int:
        """What the host copies hold."""
        return sum(host.numel() * host.element_size() for _, host in self.kept.values())

    def release(self) -> None:
        """The card given back and the host copies with it: the model dropped and the allocator's
        cache emptied.

        A load that failed half way has no model to drop but has left what it allocated in the
        cache, so the cache is emptied whether or not a model was held.
        """
        import gc

        import torch

        self.model = None
        self.shape = None
        self.checkpoint = None
        self.kept = {}
        self.on_card = False

        gc.collect()

        if torch.cuda.is_available():
            torch.cuda.empty_cache()


def encode(python: str, wav: Path, args: argparse.Namespace) -> int:
    """The spoken wav encoded as OGG/Opus by the file that owns the encoder, found beside this one,
    so the mouth speaks the same from the checkout and from the voice tool's placed folder.

    It is a plain spawn rather than the command line the callers build, and deliberately so: the
    lock in front of the card is already held by this process, and a second `flock` on the same
    file from inside it would wait on a lock this very run is holding.
    """
    line = [python, str(Path(__file__).resolve().parent / "voice-encode.py"), "--from-wav", str(wav), "--out", args.out]

    if args.wav:
        line += ["--wav", args.wav]

    return subprocess.run(line).returncode


def inside(root: Path, name: str) -> Path | None:
    """A name the caller sent, as a path under the voices root, or None for one that leaves it."""
    if not isinstance(name, str) or name == "" or name.startswith("/"):
        return None

    path = (root / name).resolve()

    return path if path.is_relative_to(root.resolve()) else None


class Service:
    """What the server holds: the mouth, the one lock in front of its own line, the lines waiting behind
    it, and the node's shared GPU lock in front of the card.

    The card is taken for a run of lines rather than for each: after a line, the next one waiting keeps
    the card and the model on it while the node's queue says it still stands first, and otherwise the
    model goes down and the card is let go at once (docs/card.md, 「One queue of requests」). With no line
    waiting the turn rests with the model on the card, until the arbiter says another wants the card
    (watch_card), and asks to come back once it has given way (docs/card.md, 「Resting」)."""

    def __init__(self, voices: Path, token: str, card: "Card", card_wait: float, room_every_s: float = 0.5) -> None:
        self.voices = voices
        self.token = token
        self.card = card
        self.card_wait = card_wait
        # How often a card just taken is asked whether its memory takes the model yet.
        self.room_every_s = room_every_s
        self.mouth = Mouth()
        self.lock = threading.Lock()
        self.busy = False
        # When each line waiting behind the one being spoken arrived, guarded apart from the line's lock.
        self.waiting: list[float] = []
        self.waiting_lock = threading.Lock()
        # Woken when the card starts to rest or to wait to come back, which the watcher then asks after.
        self.watch_wake = threading.Event()

    def health(self) -> dict:
        import torch

        free = card_free_bytes()
        elsewhere = self.card.elsewhere()

        return {
            "loaded": None if self.mouth.checkpoint is None else self.mouth.checkpoint.name,
            "busy": self.busy,
            "onCard": self.mouth.on_card,
            "device": torch.cuda.get_device_name(0) if torch.cuda.is_available() else "cpu",
            "vramBytes": torch.cuda.memory_allocated() if torch.cuda.is_available() else 0,
            "keptBytes": self.mouth.kept_bytes(),
            "torch": torch.__version__,
            "cardFreeBytes": free,
            # Whether other work holds the node's shared GPU lock this moment.
            "cardTaken": elsewhere,
            # Whether this server holds the card itself, between lines that carry on or resting on it.
            "holding": self.card.holding,
            "resting": self.card.resting,
            # Whether a line could be spoken now: the card this server's own, or nobody else on it and
            # room on it for the weights (fits).
            "room": self.card.holding or (not elsewhere and self.fits(free)),
        }

    def fits(self, free: int | None) -> bool:
        """Whether the card's free memory takes the model: the kept weights put up where it is loaded, a
        whole load where it is not. A card the driver cannot be asked about is given the benefit of the
        doubt."""
        return free is None or free >= (UP_BYTES if self.mouth.model is not None else LOAD_BYTES)

    def room(self, until: float) -> bool:
        """The card just taken, waited on until its memory takes the model, `until` at the latest. A turn
        is handed on the moment the holder before gives it back, and a holder can give it back while its
        process still stands on the card, so the turn alone never says the memory is free."""
        while not self.fits(card_free_bytes()):
            if time.monotonic() >= until:
                return False
            time.sleep(self.room_every_s)
        return True

    def speak(self, request: dict) -> tuple[bytes, dict]:
        checkpoint = inside(self.voices, request.get("checkpoint"))
        reference = inside(self.voices, request.get("reference"))
        text = request.get("text")

        if checkpoint is None or reference is None or not isinstance(text, str) or not text.strip():
            raise ValueError("a request names a checkpoint and a reference under the voices root, and a text")

        if len(text) > MOST_TEXT:
            raise ValueError(f"a text is at most {MOST_TEXT} characters")

        style = delivery(request.get("style"))
        by_sentence = request.get("sentences", False)

        if not isinstance(by_sentence, bool):
            raise ValueError("sentences is true or false")

        for what, path in (("checkpoint", checkpoint / "lora_weights.safetensors"), ("reference", reference)):
            if not path.exists():
                raise FileNotFoundError(f"the {what} is not on this node: {path.relative_to(self.voices.resolve())}")

        prompts = continuation(request.get("prompts"), lambda name: inside(self.voices, name))
        arrived = time.monotonic()

        with self.waiting_lock:
            self.waiting.append(arrived)

        try:
            self.lock.acquire()
        finally:
            with self.waiting_lock:
                self.waiting.remove(arrived)

        try:
            self.busy = True
            if self.card.resting and not self.card.resume(time.monotonic() - arrived):
                # Another stands first for the card: the rest ends in giving way, and the line asks in line.
                self.let_go()
            fresh, until = not self.card.holding, time.monotonic() + self.card_wait
            self.card.take(self.card_wait, time.monotonic() - arrived)

            if fresh and not self.room(until):
                self.let_go()
                raise CardTaken(f"the card's memory stayed taken for the whole of {self.card_wait:.0f}s")

            try:
                # A LoRA swap happens in the host copies, before the model goes up, so the next line is
                # put up from them: a model still on the card from the line before is put down first
                # where this line names other weights.
                if self.mouth.on_card and self.mouth.checkpoint != checkpoint:
                    self.settle()
                loaded = self.mouth.ready(checkpoint)
                loaded += self.mouth.up()
                started = time.monotonic()
                audio, rate = self.mouth.speak(
                    checkpoint, reference, text.strip(),
                    float(request.get("cfg", DEFAULT_CFG)), int(request.get("steps", DEFAULT_STEPS)),
                    int(request.get("seed", 0)), style, prompts, by_sentence,
                )
                spoke = time.monotonic() - started
            except BaseException:
                self.let_go()
                raise

            self.after_line()
        finally:
            self.busy = False
            self.lock.release()

        body = io.BytesIO()
        sf.write(body, audio, rate, format="WAV", subtype="PCM_16")

        return body.getvalue(), {
            "X-Voice-Seconds": f"{len(audio) / rate:.2f}",
            "X-Voice-Load-Seconds": f"{loaded:.2f}",
            "X-Voice-Speak-Seconds": f"{spoke:.2f}",
            "X-Voice-Checkpoint": checkpoint.name,
        }

    def after_line(self) -> None:
        """The card after a line that was spoken: kept for the next line waiting while it still stands first
        for the card, let go where another stands first, and with no line waiting rested on, or given way
        to whoever already wants it and asked back, or let go where the arbiter keeps no rests."""
        with self.waiting_lock:
            oldest = min(self.waiting) if self.waiting else None

        if oldest is not None:
            if not self.card.keeps(time.monotonic() - oldest):
                self.let_go()
            return

        said = self.card.rest()

        if said == "rests":
            self.watch_wake.set()
            return

        self.let_go()

        if said == "gives way":
            self.card.come_back()
            self.watch_wake.set()

    def watch_card(self, again_s: float = 0.2) -> None:
        """The card watched for the server's life, on a thread of its own (watch_step), asked again after a
        pause where an answer came back at once."""
        while True:
            asked_at = time.monotonic()

            try:
                asked = self.watch_step()
            except OSError:
                # An arbiter that does not answer is asked again after a pause; the lock still guards the card.
                asked = True
            except Exception as failure:
                print(f"voice-vox: watching the card failed: {type(failure).__name__}: {failure}", file=sys.stderr, flush=True)
                asked = True

            if not asked:
                self.watch_wake.wait(1.0)
                self.watch_wake.clear()
            elif time.monotonic() - asked_at < 0.05:
                time.sleep(again_s)

    def watch_step(self) -> bool:
        """One ask after the resting card's ticket, its answer held open until another wants the card, or,
        once the card was given way, after the ticket that only rests until it is granted; False where there
        is neither to ask after. What the answer calls for is done under the line's lock, and only where no
        line is in hand and the card still stands as it did when asked: a line that came meanwhile has the
        card its own way."""
        card = self.card
        ticket = None if card.turn is None else card.turn.ticket
        resting, returning = card.resting, card.returning

        if ticket is None or not (resting or returning):
            return False

        answered = card.turn.watch(ticket)

        with self.lock:
            with self.waiting_lock:
                in_hand = bool(self.waiting)

            if in_hand or card.turn.ticket != ticket or (card.resting, card.returning) != (resting, returning):
                return True

            if resting and (answered is None or not answered.turn or answered.give_way_for is not None):
                # Asked for by another, or the arbiter no longer knows the turn: down, and back once free.
                self.let_go()
                card.come_back()
            elif returning and answered is None:
                card.come_back()
            elif returning and answered.turn and card.take_returned():
                if not self.fits(card_free_bytes()):
                    # The card handed back still holds another's memory: nothing goes up, and the next
                    # line takes the card its own way.
                    self.let_go()
                    return True
                try:
                    self.mouth.up()
                except Exception as failure:
                    print(f"voice-vox: putting the model back up failed, so the card was let go: {type(failure).__name__}: {failure}", file=sys.stderr, flush=True)
                    self.let_go()

        return True

    def let_go(self) -> None:
        """The model put down and the card let go, at once."""
        try:
            self.settle()
        finally:
            self.card.let_go()

    def settle(self) -> None:
        """The card given back before its lock is, however the line ended: the model put down into
        its host copies, or, where even that fails, let go whole."""
        try:
            self.mouth.down()
        except Exception as failure:
            print(f"voice-vox: putting the model down failed, so it was let go: {type(failure).__name__}: {failure}", file=sys.stderr, flush=True)
            self.mouth.release()


class CardTaken(Exception):
    """Other work held the node's shared GPU lock for the whole of a line's wait."""


def card_turn():
    """The voice's ticket at the node's card arbiter, through the turn client the arbiter installs on the node and
    the unit puts on the import path, under the service name the unit declares; None where the client is not there,
    and the lock is taken as it comes."""
    try:
        from turn import TurnClient
    except ImportError:
        return None
    return TurnClient(os.environ.get("BMO_CARD_SERVICE", "") or "voice")


class Card:
    """The node's shared GPU lock (`GPU_LOCK`): one file every service putting work on the card holds
    under flock while its work is there.

    The lock is let go by closing the file, which the kernel also does for a process that dies, so a
    server that falls over never leaves the card locked behind it.
    """

    def __init__(self, path: Path, turn=None) -> None:
        self.path = path
        # Where the node's card arbiter keeps a hold queue, a line waits its turn there before the lock.
        self.turn = turn
        # The open lock while this server holds the card, from one line to the next that carries on.
        self.handle: int | None = None
        # The turn resting with the model on the card, nothing left to speak (docs/card.md, 「Resting」);
        # and, having given way, a ticket that only rests waiting to be granted.
        self.resting = False
        self.returning = False

    def elsewhere(self) -> bool:
        """Whether another process holds the lock now.

        It is read out of the kernel's table of locks rather than found by taking the lock, so asking
        never stands in the way of another service taking the card that very moment.
        """
        try:
            stat = self.path.stat()
            table = Path("/proc/locks").read_text(encoding="utf-8").splitlines()
        except OSError:
            return False

        where = f"{os.major(stat.st_dev):02x}:{os.minor(stat.st_dev):02x}:{stat.st_ino}"
        mine = str(os.getpid())

        # A held lock reads `1: FLOCK  ADVISORY  WRITE <pid> <dev>:<inode> 0 EOF`; a process waiting
        # for one reads `1: -> FLOCK …` and holds nothing.
        return any(
            len(fields) >= 6 and fields[1] == "FLOCK" and fields[5] == where and fields[4] != mine
            for fields in (line.split() for line in table)
        )

    @property
    def holding(self) -> bool:
        return self.handle is not None

    def take(self, wait: float, waited: float = 0.0) -> None:
        """The lock taken once other work lets go of it within `wait` seconds, for a line that has waited
        `waited` already, and kept until `let_go`; taking it again while held does nothing.

        Where the node keeps a hold queue, the line waits its turn in it, the arbiter holding its answer
        open until the card is handed on, and stands there by the line's arrival. Elsewhere the lock is
        asked for without blocking, again and again, because a blocking flock cannot be given a deadline.
        """
        if self.handle is not None:
            return

        if self.returning:
            # The ticket that only rests goes, and the line stands in line by its own arrival.
            self.returning = False
            self.turn.give_back()

        if self.turn is not None:
            from turn import hold

            try:
                handle = hold(self.turn, self.path, until=time.monotonic() + wait, waited_s=waited)
            except PermissionError as refused:
                raise CardTaken(str(refused)) from None
            if handle is None:
                raise CardTaken(f"the card was not the voice's turn within {wait:.0f}s")
            self.handle = handle
            return

        handle = os.open(self.path, os.O_RDWR | os.O_CREAT, 0o644)
        deadline = time.monotonic() + wait

        while True:
            try:
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    os.close(handle)
                    raise CardTaken(f"other work held the card for the whole of {wait:.0f}s") from None

                time.sleep(0.2)

        self.handle = handle

    def keeps(self, next_waited: float) -> bool:
        """Whether the card carries on to the next line waiting here, which has waited `next_waited`: where
        the node keeps a hold queue, only while that line still stands first in it (docs/card.md, 「One
        queue of requests」); where it keeps none, nobody else queues for the card, so it does."""
        return self.turn is None or self.turn.keep(next_waited)

    def rest(self) -> str:
        """The turn told no line is left: `rests` where the arbiter keeps it so, the lock and the model
        kept; `gives way` where somebody already wants the card; `lets go` where the arbiter keeps no
        rests, or none answers."""
        answered = None if self.turn is None or self.handle is None else self.turn.rest()

        if answered is None or not answered.turn or not answered.resting:
            return "lets go"

        if answered.give_way_for is not None:
            return "gives way"

        self.resting = True

        return "rests"

    def resume(self, next_waited: float) -> bool:
        """A line come while the turn rests: whether the arbiter says it carries on with the card as it
        stands, which ends the rest."""
        kept = self.keeps(next_waited)

        if kept:
            self.resting = False

        return kept

    def come_back(self) -> None:
        """Having given way, a ticket asked that only rests, granted once nobody else wants the card."""
        if self.turn is None or self.handle is not None:
            return

        try:
            self.returning = self.turn.ask(rest=True) is not None
        except PermissionError:
            self.returning = False

    def take_returned(self) -> bool:
        """The lock taken for a ticket that only rests, now granted; the turn rests from the start. False
        where somebody took the lock in the moment between."""
        if self.handle is None:
            handle = os.open(self.path, os.O_RDWR | os.O_CREAT, 0o644)

            try:
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                os.close(handle)
                return False

            self.handle = handle

        self.returning = False
        self.resting = True

        return True

    def let_go(self) -> None:
        """The lock let go and the turn given back, at once."""
        self.resting = False

        if self.handle is None:
            return

        os.close(self.handle)
        self.handle = None

        if self.turn is not None:
            self.turn.give_back()


def handler(service: Service):
    class Handler(BaseHTTPRequestHandler):
        # Kept-alive connections, as a client pools them: under HTTP/1.0 the server closes each one
        # after its answer, and a client that sends its next call down that pooled socket is reset.
        protocol_version = "HTTP/1.1"

        def log_message(self, format: str, *values) -> None:
            # A line of the agent's speech is never logged; the path and the status are enough.
            print(f"voice-vox: {self.command} {self.path} {values[1] if len(values) > 1 else ''}", file=sys.stderr, flush=True)

        def answer(self, status: int, body: bytes, kind: str, headers: dict | None = None) -> None:
            self.send_response(status)
            self.send_header("Content-Type", kind)
            self.send_header("Content-Length", str(len(body)))

            for name, value in (headers or {}).items():
                self.send_header(name, value)

            self.end_headers()
            self.wfile.write(body)

        def refuse(self, status: int, why: str) -> None:
            self.answer(status, json.dumps({"error": why}).encode(), "application/json")

        def allowed(self) -> bool:
            given = self.headers.get("Authorization", "")

            if hmac.compare_digest(given.encode(), f"Bearer {service.token}".encode()):
                return True

            self.refuse(401, "a call carries the voice token")

            return False

        def do_GET(self) -> None:
            if self.path != "/health":
                return self.refuse(404, "GET /health or POST /speak")

            if self.allowed():
                self.answer(200, json.dumps(service.health()).encode(), "application/json")

        def do_POST(self) -> None:
            if self.path != "/speak":
                return self.refuse(404, "GET /health or POST /speak")

            size = int(self.headers.get("Content-Length") or 0)

            if size <= 0 or size > MOST_BODY:
                # The unread body would stand in front of the next request on this connection.
                self.close_connection = True
                return self.refuse(400, f"a request body is JSON of at most {MOST_BODY} bytes")

            # Read whole before anything is answered, a refusal included: a reply sent while the
            # caller is still writing its body resets the connection, and the caller would hear
            # "unreachable" where the truth is "refused".
            raw = self.rfile.read(size)

            if not self.allowed():
                return

            try:
                request = json.loads(raw)
            except ValueError:
                return self.refuse(400, "a request body is JSON")

            try:
                wav, headers = service.speak(request if isinstance(request, dict) else {})
            except (ValueError, FileNotFoundError) as refusal:
                return self.refuse(400, str(refusal))
            except CardTaken as taken:
                # Never told when to come back: the caller chooses a node again at once (docs/voice-plan.md).
                return self.refuse(503, str(taken))
            except Exception as failure:  # the model's own failure, said without the line
                print(f"voice-vox: speaking failed: {type(failure).__name__}: {failure}", file=sys.stderr, flush=True)
                return self.refuse(500, f"speaking failed: {type(failure).__name__}")

            self.answer(200, wav, "audio/wav", headers)

    return Handler


def serve(args: argparse.Namespace) -> int:
    token_file = Path(args.token_file)

    if not token_file.is_file() or token_file.stat().st_mode & 0o777 != 0o600:
        print(f"voice-vox: the token file must exist and be 0600: {token_file}", file=sys.stderr)
        return 1

    token = token_file.read_text(encoding="utf-8").strip()

    if token == "":
        print(f"voice-vox: the token file is empty: {token_file}", file=sys.stderr)
        return 1

    card = Card(Path(args.gpu_lock).expanduser(), card_turn())
    service = Service(Path(args.voices), token, card, args.gpu_wait)
    server = ThreadingHTTPServer((args.host, args.port), handler(service))
    threading.Thread(target=service.watch_card, daemon=True).start()
    print(f"voice-vox: serving on {args.host}:{args.port} · voices {args.voices} · card lock {card.path}", file=sys.stderr, flush=True)
    server.serve_forever()

    return 0


def ask(args: argparse.Namespace) -> int:
    """One call to the server on this machine's loopback, its end printed as one line of JSON.

    Every way the call can end is printed rather than raised, so the caller on the other side of the
    node's shell reads what happened from that line alone: a status where the server answered,
    `unreachable`, `timeout` or `token` where it never did.
    """
    import urllib.error
    import urllib.request

    said: dict = {"status": None, "error": None}
    body = None

    if args.request:
        request_file = Path(args.request)
        body = request_file.read_bytes()
        # The line is on this machine only as long as it takes to be read.
        request_file.unlink()

    try:
        token = Path(args.token_file).read_text(encoding="utf-8").strip()
    except OSError as missing:
        said["error"] = f"token: the node's token file cannot be read ({type(missing).__name__})"
        print(json.dumps(said), flush=True)
        return 0

    url = f"http://127.0.0.1:{args.port}/{'speak' if body is not None else 'health'}"
    headers = {"Authorization": f"Bearer {token}"}

    if body is not None:
        headers["Content-Type"] = "application/json"

    try:
        with urllib.request.urlopen(urllib.request.Request(url, data=body, headers=headers), timeout=args.timeout) as answer:
            said["status"] = answer.status
            data = answer.read()

            if body is None:
                said["health"] = json.loads(data)
            else:
                Path(args.wav).write_bytes(data)
                said["seconds"] = float(answer.headers.get("X-Voice-Seconds") or 0)
                said["loadSeconds"] = float(answer.headers.get("X-Voice-Load-Seconds") or 0)
                said["speakSeconds"] = float(answer.headers.get("X-Voice-Speak-Seconds") or 0)
                said["checkpoint"] = answer.headers.get("X-Voice-Checkpoint") or ""
    except urllib.error.HTTPError as refusal:
        said["status"] = refusal.code

        try:
            said["error"] = str(json.loads(refusal.read()).get("error", ""))[:200] or None
        except ValueError:
            said["error"] = None
    except (TimeoutError, socket.timeout):
        said["error"] = "timeout: the server did not answer in time"
    except urllib.error.URLError as unreached:
        waited = isinstance(unreached.reason, (TimeoutError, socket.timeout))
        said["error"] = "timeout: the server did not answer in time" if waited else f"unreachable: {unreached.reason}"
    except ConnectionError as dropped:
        said["error"] = f"unreachable: {type(dropped).__name__}"

    print(json.dumps(said), flush=True)

    return 0


def once(args: argparse.Namespace) -> int:
    if args.text_file:
        text = open(args.text_file, encoding="utf-8").read().strip()
    elif args.text:
        text = args.text.strip()
    else:
        text = sys.stdin.read().strip()

    if not text:
        print("voice-vox: nothing to say", file=sys.stderr)
        return 1

    if not args.out or not args.checkpoint or not args.reference:
        print("voice-vox: speaking once needs --out, --checkpoint and --reference", file=sys.stderr)
        return 1

    checkpoint = Path(args.checkpoint)
    reference = Path(args.reference)

    # Said before a model is loaded rather than after: half a minute of loading to find out that
    # the weights are not there is half a minute the room spends waiting for nothing.
    for what, path in (("checkpoint", checkpoint / "lora_weights.safetensors"), ("reference", reference)):
        if not path.exists():
            print(f"voice-vox: the {what} is not on this machine: {path}", file=sys.stderr)
            return 1

    try:
        style = delivery(args.style)
        prompts = continuation(
            {language: {"clip": clip, "words": words} for language, clip, words in args.prompt or []}, Path,
        )
    except ValueError as refusal:
        print(f"voice-vox: {refusal}", file=sys.stderr)
        return 1

    audio, rate = Mouth().speak(checkpoint, reference, text, args.cfg, args.steps, args.seed, style, prompts, args.sentences)

    with tempfile.TemporaryDirectory(prefix="bmo-voice-vox-") as room:
        spoken = Path(room) / "spoken.wav"
        sf.write(spoken, audio, rate, subtype="PCM_16")
        print(f"voice-vox: {len(audio) / rate:.1f}s · {rate} Hz · {checkpoint.name}", file=sys.stderr)

        return encode(args.encoder_python, spoken, args)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--text")
    parser.add_argument("--text-file")
    parser.add_argument("--out")
    parser.add_argument("--wav")
    parser.add_argument("--checkpoint")
    parser.add_argument("--reference")
    parser.add_argument("--cfg", type=float, default=DEFAULT_CFG)
    parser.add_argument("--steps", type=int, default=DEFAULT_STEPS)
    parser.add_argument("--style")
    parser.add_argument("--encoder-python", default=sys.executable)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--sentences", action="store_true")
    parser.add_argument("--prompt", nargs=3, action="append", metavar=("LANGUAGE", "CLIP", "WORDS"))
    parser.add_argument("--serve", action="store_true")
    parser.add_argument("--voices")
    parser.add_argument("--token-file")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--ask", action="store_true")
    parser.add_argument("--request")
    parser.add_argument("--timeout", type=float, default=600.0)
    parser.add_argument("--port", type=int, default=7432)
    parser.add_argument("--gpu-lock", default=GPU_LOCK)
    parser.add_argument("--gpu-wait", type=float, default=GPU_WAIT)
    args = parser.parse_args()

    if args.serve:
        if not args.voices or not args.token_file:
            print("voice-vox: serving needs --voices and --token-file", file=sys.stderr)
            return 1

        return serve(args)

    if args.ask:
        if not args.token_file or (args.request and not args.wav):
            print("voice-vox: asking needs --token-file, and --wav beside --request", file=sys.stderr)
            return 1

        return ask(args)

    return once(args)


if __name__ == "__main__":
    raise SystemExit(main())
