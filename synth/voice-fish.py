#!/usr/bin/env python
"""Speaks a line in an agent's own trained voice: Fish Audio S2-Pro with the voice's adapter folded into its weights.

The voice is a LoRA fine-tune of S2-Pro's slow autoregressive model on the agent's own recorded lines, spoken on the
delivery clip of the line's language where its folder holds one (docs/voice-plan.md, 「The voices」). What picks a run's weights is the voice folder named by the
caller, never a table this file reads for itself: the row was chosen by the caller, and a mouth that went looking for
its own settings could speak in a voice nobody asked for.

It works three ways; the first two speak through the same `Mouth`, and the third asks the second:

- **Once** (the default): speak one line on this machine's card, write a wav, and hand that wav to
  `voice-encode.py`, in a process of its own, for the OGG/Opus encode.
- **As a server** (`--serve`): the voice server on each voice node (docs/voice-plan.md, 「The voice nodes」),
  listening on the node's loopback. It answers `POST /speak` with the spoken line as a wav and `GET /health` with
  what it holds. The card is shared under the node's shared GPU lock (`Card`): the model stays on the card between
  lines until another occupant asks for it, is let go whole as it gives way, and is loaded from the disk again by the
  next line. Encoding stays on the caller's side, where the encoder lives.
- **Asking** (`--ask`): the server's own client, run on the node beside it, which is how a call from the operations
  machine reaches a server nothing outside the node can reach. It posts the request file it is given and removes it,
  writes the wav the server answered, and prints one line of JSON: the status, the server's reason where it refused,
  and the answer's account of the line. With no request it asks `/health` and prints what the server holds.

It runs on the engine environment's interpreter, the `fish-speech` checkout's own (docs/voice-plan.md, 「The
environments」), which it imports the model from; how the model is made to fit a 12 GB card is this file's own
(docs/voice-plan.md, 「How the model is loaded」), so the checkout is used as it stands.

The peak is matched to the encoder's ceiling before the wav leaves, since the encoder only ever lowers a peak.

usage: <engine venv>/bin/python synth/voice-fish.py --text '<words>' --out <path.ogg> --checkpoint <voice folder>
       [--base <s2-pro folder>] [--style '<instruction>'] [--encoder-python <path>] [--wav <path.wav>] [--seed 0]
       [--sentences]
       <engine venv>/bin/python synth/voice-fish.py --serve --voices <dir> --base <s2-pro folder> --token-file <path>
       [--host 127.0.0.1] [--port 7432] [--gpu-lock ~/.gpu.lock]
       <engine venv>/bin/python synth/voice-fish.py --ask --token-file <path> [--port 7432]
       [--request <request.json> --wav <path.wav>]
"""

import argparse
import csv
import ctypes
import fcntl
import hmac
import io
import json
import os
import random
import re
import select
import socket
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
import traceback
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import numpy as np
import soundfile as sf

# The card's memory in segments that grow and shrink, so what one pass frees is used again whole by the next
# (docs/voice-plan.md, 「How the model is loaded」). It is read once, as torch first touches the card, so it is set
# before torch is imported anywhere in this process.
os.environ.setdefault("PYTORCH_CUDA_ALLOC_CONF", "expandable_segments:True")

# What the whole thing is kept under. It is the same number `synth/voice-encode.py` holds its ceiling at.
PEAK = 0.95

# The rate a line leaves at: the one OGG/Opus is written at. The codec speaks at 44.1 kHz, so the mouth resamples
# once, the one change it makes to the sound (docs/voice-plan.md, 「What the sound carries」).
RATE = 48000

# The engine a voice folder must name in its `voice.toml` to be spoken here: the policy's (`policy/voice.toml`).
ENGINE = "fish-s2-pro"
# The file a voice's delivery clip stands in, in the voice's folder, and the words it says in its `voice.toml`
# (docs/voice-plan.md, 「The voices」); a clip of a line's language stands as `reference.<language>.wav`, its words as
# `reference_words_<language>`.
REFERENCE_CLIP = "reference.wav"
REFERENCE_WORDS = "reference_words"
# A line's language as a request names it: a language code of two or three lower-case letters.
LANGUAGE = re.compile(r"[a-z]{2,3}")
# A first capture longer than this, in seconds, compiled the step: a capture alone takes under one.
RECOMPILED_S = 5.0

# The base weights on this machine, as `scripts/voice-node.ts setup` places them (docs/voice-plan.md, 「Where the
# weights live」), where the caller names none.
BASE = "~/bmo-voice/s2-pro"

# The context the model is built with (docs/voice-plan.md, 「How the model is loaded」): the model's generator keeps
# 2,048 tokens of it for what one piece speaks and takes the prompt in what is left.
CONTEXT = 3072
MOST_NEW_TOKENS = 2048

# The most characters of one piece where the voice does not speak a sentence at a time (docs/voice-plan.md, 「What a
# long line takes of the card」): a longer line is spoken in passes of whole sentences that each stay within this.
MOST_PASS = 240

# The most characters of any piece (docs/voice-plan.md, 「What a long line takes of the card」): what the codec can make
# into sound in one pass beside the weights on a 12 GB card. A longer sentence is cut at its clauses.
MOST_PIECE = 340

# The most frames a piece's codec pass is made with the key-value caches and the frame step's graph still on the card
# beside it (docs/voice-plan.md, 「What a long line takes of the card」); a longer piece lets them go first.
BESIDE_CACHES = 600

# How a line is cut into sentences: at a full stop, a question or exclamation mark or an ellipsis before a space,
# and at every line break.
SENTENCE_END = re.compile(r"(?<=[.?!…])\s+|\n+")

# How a sentence is cut into clauses: after a comma, a semicolon, a colon or a dash before a space.
CLAUSE_END = re.compile(r"(?<=[,;:—–，、；：])\s+|(?<=\s-)\s+")

# This many seconds of silence stand between two pieces of a line.
SENTENCE_PAUSE = 0.3

# The loudness every piece of a line is brought to before the line is joined (docs/voice-plan.md, 「What the sound
# carries」): its RMS over its voiced frames, those within `VOICED_DB` of its loudest frame, in dBFS.
PIECE_RMS_DB = -20.0
VOICED_DB = 40.0
LEVEL_FRAME = 1024

# What says a piece stopped before its last syllable fell away (docs/voice-plan.md, 「What a long line takes of the
# card」): its last `CUT_TAIL_S` still within `CUT_DB` of its loudest frame, and less than `CUT_DECAY_S` after its last
# frame within `LOUD_DB` of the loudest.
CUT_TAIL_S = 0.03
CUT_DB = 30.0
CUT_DECAY_S = 0.10
LOUD_DB = 20.0

# How long each edge of a spoken piece is faded over, in seconds (docs/voice-plan.md, 「The voices」).
EDGE_FADE = 0.01

# The server's own limit: a body is read whole into memory, so it is bounded before it is read. A line is spoken
# whole however many characters it holds (docs/voice-plan.md, 「What a long line takes of the card」), and this holds
# the longest spoken words the engine takes in, four bytes to a character and more, with room to spare.
MOST_BODY = 1024 * 1024
# A delivery instruction is a sentence, not a script: anything longer is a mistake in a row.
MOST_STYLE = 300
# The most characters of a refusal the asker hands on: room for a failure's kind, message and frame.
MOST_ERROR = 600

# What loading the model takes of the card: the server's process peaks at about 11,100 MiB while a line is spoken on
# an RTX 4070, its own CUDA context and the frame step's graph among them, so a card with less free than this cannot
# take a load; the health says so (docs/voice-plan.md, 「What it answers」).
LOAD_BYTES = int(11.0 * 1024**3)

# The node's shared GPU lock: every service that puts work on the node's card holds this one file under flock for
# as long as its work is there, so the services take the card in turn rather than all at once.
GPU_LOCK = "~/.gpu.lock"


def card_free_bytes() -> int | None:
    """The card's free memory as the driver counts it, every process on it included, or None where
    the driver cannot be asked. It is asked of nvidia-smi rather than torch, so asking opens no CUDA
    context the server would then hold on a card it shares."""
    try:
        ran = subprocess.run(
            ["nvidia-smi", "--id=0", "--query-gpu=memory.free", "--format=csv,noheader,nounits"],
            capture_output=True, text=True,
        )
        return int(ran.stdout.strip().splitlines()[0]) * 1024 * 1024 if ran.returncode == 0 else None
    except (OSError, ValueError, IndexError):
        return None


def own_card_bytes() -> int:
    """What this server's own process holds on the card as the driver counts it: its CUDA context, and the model
    where it stands. It is the server's to use, and the context stays on the card after the model is let go, so the
    room the model needs counts it as free. Nothing where the driver cannot be asked."""
    try:
        ran = subprocess.run(
            ["nvidia-smi", "--id=0", "--query-compute-apps=pid,used_memory", "--format=csv,noheader,nounits"],
            capture_output=True, text=True,
        )
    except OSError:
        return 0

    if ran.returncode != 0:
        return 0

    mine = str(os.getpid())
    held = 0

    for row in csv.reader(io.StringIO(ran.stdout), skipinitialspace=True):
        if len(row) >= 2 and row[0].strip() == mine:
            try:
                held += int(row[1].strip()) * 1024 * 1024
            except ValueError:
                continue

    return held


def card_room_bytes() -> int | None:
    """The memory on the card the model may take: what the driver counts free, and what this server holds itself."""
    free = card_free_bytes()

    return None if free is None else free + own_card_bytes()


def gib(count: int | None) -> str:
    return "an unknown amount" if count is None else f"{count / 1024**3:.2f} GiB"


class Abandoned(Exception):
    """The line's asker has gone: nobody waits for its sound, so the server drops it and answers nothing."""


class LineStage:
    """Where one line stands on the server, written into the call's stage file each time it changes, for the engine to
    read (docs/voice-plan.md, 「How a line crosses」): one line of JSON, its kind, its count of steps and an English
    sentence, never the line's words. A line asked with no stage file is told nowhere."""

    def __init__(self, path: Path | None = None) -> None:
        self.path = path
        self.step = 0

    def tell(self, stage: str, said: str) -> None:
        self.step += 1

        if self.path is None:
            return

        written = self.path.with_name(self.path.name + ".new")

        try:
            written.write_text(json.dumps({"stage": stage, "step": self.step, "said": said}) + "\n", encoding="utf-8")
            os.replace(written, self.path)
        except OSError as failure:
            print(f"voice-fish: a line's stage could not be written: {failure}", file=sys.stderr, flush=True)

    def piece(self, at: int, of: int) -> None:
        self.tell("speak", f"speaks piece {at} of {of}")


def failed_at(failure: BaseException) -> str:
    """A failure as one line: its kind, its message, and the last frame of this file's own code it passed through,
    which says where in the mouth it was met; the frames below it are the model's and stand in the node's journal."""
    frames = [frame for frame in traceback.extract_tb(failure.__traceback__) if frame.filename == __file__]
    where = f" (in {frames[-1].name}, line {frames[-1].lineno} of {Path(__file__).name})" if frames else ""

    return f"{type(failure).__name__}: {failure}{where}"


def delivery(style) -> str:
    """The instruction as it may be read, or a refusal saying why it may not.

    A bracket inside it would close the instruction early and have the rest spoken aloud as part of the line, so an
    instruction carrying one is refused rather than spoken wrong.
    """
    if style is None:
        return ""

    if not isinstance(style, str) or len(style) > MOST_STYLE or "[" in style or "]" in style:
        raise ValueError(f"a style is plain words of at most {MOST_STYLE} characters, without brackets")

    return style.strip()


def styled(style: str, text: str) -> str:
    """The instruction as S2-Pro reads one: a tag in brackets before the words, never spoken aloud."""
    return f"[{style}] {text}" if style else text


def refill(frame, loaded) -> None:
    """A loaded model's weights put into a kept frame of the same model: every parameter, every buffer and every
    tensor a module keeps as a plain attribute handed over by name, none copied. A frame whose modules, parameters or
    buffers are not the loaded model's by name is refused."""
    import torch

    kept, new = dict(frame.named_modules()), dict(loaded.named_modules())

    if kept.keys() != new.keys():
        raise RuntimeError("the kept frame's modules are not the loaded model's")

    for listed in ("named_parameters", "named_buffers"):
        into, held = dict(getattr(frame, listed)()), dict(getattr(loaded, listed)())

        if into.keys() != held.keys():
            raise RuntimeError(f"the kept frame's {listed.removeprefix('named_')} are not the loaded model's")

        for name, tensor in into.items():
            tensor.data = held[name].data

    for name, module in new.items():
        for attribute, value in vars(module).items():
            if isinstance(value, torch.Tensor):
                setattr(kept[name], attribute, value)

    frame._cache_setup_done = False


def delivery_of(folder: Path, said: dict, language: str | None) -> tuple[Path, str] | None:
    """The delivery clip a line in `language` is spoken on, as its file and the words it says: the voice's clip of
    that language, `reference.<language>.wav` with `voice.toml`'s `reference_words_<language>`, where both stand, and
    else the voice's own, `reference.wav` with `reference_words`; none where neither stands whole."""
    own = (folder / REFERENCE_CLIP, said.get(REFERENCE_WORDS))
    chosen = [own] if language is None else [(folder / f"reference.{language}.wav", said.get(f"{REFERENCE_WORDS}_{language}")), own]

    for clip, words in chosen:
        if isinstance(words, str) and words.strip() and clip.exists():
            return clip, words.strip()

    return None


def declared(folder: Path) -> dict:
    """A voice folder's `voice.toml`, refused where the folder is not a voice of this engine or lacks its adapter."""
    declaration = folder / "voice.toml"

    if not declaration.exists():
        raise FileNotFoundError(f"the voice is not on this node, or is not a {ENGINE} voice: {folder.name}")

    said = tomllib.loads(declaration.read_text(encoding="utf-8"))

    if said.get("engine") != ENGINE:
        raise ValueError(f"{folder.name} is a {said.get('engine')} voice, and this server speaks {ENGINE} voices only")

    if not (folder / "lora.pt").exists():
        raise FileNotFoundError(f"the adapter of {folder.name} is not on this node")

    return said


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


def gathered(spans: list[tuple[int, int]], most: int) -> list[tuple[int, int]]:
    """The spans gathered in order into runs each as long as it can be within `most` characters, from the start of its
    first span to the end of its last; a span longer than `most` is a run of its own."""
    runs: list[tuple[int, int]] = []

    for start, end in spans:
        if runs and end - runs[-1][0] <= most:
            runs[-1] = (runs[-1][0], end)
        else:
            runs.append((start, end))

    return runs


def clauses(sentence: str, most: int = MOST_PIECE) -> list[str]:
    """The sentence as the parts it is spoken in: whole where it is within `most` characters, and otherwise its
    clauses (`CLAUSE_END`) gathered in order into parts each as long as it can be within them. A clause longer than
    `most` alone is cut at the last space within the limit, and one with no space in it at the limit. No word is left
    out and none is cut where a space stands to cut at."""
    if len(sentence) <= most:
        return [sentence]

    spans, start = [], 0

    for gap in [*CLAUSE_END.finditer(sentence), None]:
        end = len(sentence) if gap is None else gap.start()

        if end > start:
            spans.append((start, end))

        start = end if gap is None else gap.end()

    parts: list[str] = []

    for start, end in gathered(spans, most):
        rest = sentence[start:end]

        while len(rest) > most:
            cut = rest.rfind(" ", 1, most + 1)
            cut = most if cut <= 0 else cut
            parts.append(rest[:cut].rstrip())
            rest = rest[cut:].lstrip()

        if rest:
            parts.append(rest)

    return parts


def sentences(text: str) -> list[str]:
    """The line as the sentences a voice speaks it in when it speaks one at a time, a sentence longer than
    `MOST_PIECE` cut at its clauses (`clauses`).

    Two short sentences are not joined, because a piece of two already carries the growth a sentence
    alone ends before.
    """
    return [part for start, end in sentence_spans(text) for part in clauses(text[start:end])]


def passes(text: str) -> list[str]:
    """The line as the passes a voice speaks it in when it does not speak a sentence at a time: the
    whole line where it is within `MOST_PASS` characters, and otherwise its sentences gathered in
    order into passes each as long as it can be within them, cut out of the line as they stand.

    A sentence longer than `MOST_PASS` is a pass of its own, cut at its clauses where it is longer
    than `MOST_PIECE` (`clauses`).
    """
    if len(text) <= MOST_PASS:
        return [text]

    return [part for start, end in gathered(sentence_spans(text), MOST_PASS) for part in clauses(text[start:end])]


def resampled(audio: np.ndarray, rate: int) -> np.ndarray:
    """The line at `RATE`, by the band-limited sinc resampler torchaudio carries."""
    if rate == RATE:
        return audio

    import torch
    import torchaudio

    return torchaudio.functional.resample(torch.from_numpy(audio), rate, RATE).numpy()


def levelled(audio: np.ndarray) -> np.ndarray:
    """The piece brought to `PIECE_RMS_DB`, its RMS taken over its voiced frames only, so the pauses and the edges
    of a short sentence weigh nothing in it. A piece with no sound is left as it is."""
    frames = len(audio) // LEVEL_FRAME

    if frames == 0:
        return audio

    energy = np.sqrt(np.mean(audio[: frames * LEVEL_FRAME].reshape(frames, LEVEL_FRAME).astype(np.float64) ** 2, axis=1))
    loudest = float(energy.max())

    if loudest <= 0.0:
        return audio

    voiced = energy[energy >= loudest * 10 ** (-VOICED_DB / 20)]
    rms = float(np.sqrt(np.mean(voiced**2)))

    return (audio * (10 ** (PIECE_RMS_DB / 20) / rms)).astype(np.float32)


def stops_short(audio: np.ndarray, rate: int) -> bool:
    """Whether the piece ends while its voice is still loud: the model closed its turn before the last syllable fell
    away, which a listener hears as the line stopping mid-word and the next sentence starting."""
    frames = len(audio) // LEVEL_FRAME
    tail = audio[-int(CUT_TAIL_S * rate):]

    if frames < 2 or len(tail) == 0:
        return False

    energy = np.sqrt(np.mean(audio[: frames * LEVEL_FRAME].reshape(frames, LEVEL_FRAME).astype(np.float64) ** 2, axis=1))
    loudest = float(energy.max())

    if loudest <= 0.0:
        return False

    last_loud = int(np.where(energy >= loudest * 10 ** (-LOUD_DB / 20))[0].max())
    decay = (frames - 1 - last_loud) * LEVEL_FRAME / rate
    still = float(np.sqrt(np.mean(tail.astype(np.float64) ** 2))) >= loudest * 10 ** (-CUT_DB / 20)

    return still and decay < CUT_DECAY_S


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


class FrameStep:
    """The step that makes one frame of a piece once its prompt is read: the slow model's token and the fast model's
    codebooks, sampled as fish-speech's own decode step samples them, compiled by inductor at its fixed shapes and
    replayed as one CUDA graph (docs/voice-plan.md, 「How the model is loaded」). It stands where fish-speech's
    generator takes a decode step, and is called the way the generator calls one.

    It takes no key-value length, so the generator asks it for none: its attention reads the whole context, masked to
    what the piece has written, and its shapes never change with the piece. The graph reads the weights and the
    key-value caches where they stood when it was captured, so it is let go (`forget`) before either is let go or
    moved, and captured again by the next frame; the compiled kernels outlive every graph. The capture's own runs draw
    from the card's generator, so its state is put back after them, and random numbers are drawn by the same calls as
    uncompiled. A step that fails to compile or to be captured is
    taken uncompiled for the rest of the process, the failure said once.
    """

    def __init__(self) -> None:
        import torch
        import torch._inductor.config
        from fish_speech.models.text2semantic.inference import decode_one_token_ar
        from torch.nn.attention import SDPBackend, sdpa_kernel

        # Random numbers drawn by the same calls as uncompiled. Each kernel is launched in the fastest configuration
        # the node measured for it, which fish-speech's module asks for as it is imported; the measurements are kept
        # beside the compiled kernels, so a restart reads them back rather than measuring again.
        torch._inductor.config.fallback_random = True

        def frame(model, x, input_pos, temperature, top_p, top_k, semantic_logit_bias, previous_tokens):
            return decode_one_token_ar(
                model, x, input_pos, temperature, top_p, top_k, semantic_logit_bias, None, None, previous_tokens=previous_tokens,
            )

        def uncompiled(model, x, input_pos, temperature, top_p, top_k, semantic_logit_bias, audio_masks, audio_parts, previous_tokens):
            # The generator asks for the plain attention kernel around every step; uncompiled, the fused ones are
            # faster.
            with sdpa_kernel([SDPBackend.FLASH_ATTENTION, SDPBackend.EFFICIENT_ATTENTION, SDPBackend.CUDNN_ATTENTION, SDPBackend.MATH]):
                return decode_one_token_ar(
                    model, x, input_pos, temperature, top_p, top_k, semantic_logit_bias, audio_masks, audio_parts, previous_tokens=previous_tokens,
                )

        self.uncompiled = uncompiled
        self.compiled = torch.compile(frame, mode="default", fullgraph=True, dynamic=False)
        self.broken = False
        # How long the first capture since it was last asked took, in seconds: one that compiled is told by it.
        self.captured_s: float | None = None
        self.forget()

    def forget(self) -> None:
        """The graph let go, with the inputs it reads and the output it writes."""
        self.graph = None
        # The frame's own inputs, copied in before each replay, then the sampling's, copied where they change.
        self.inputs: list | None = None
        self.sampling: tuple | None = None
        self.top_k: int | None = None
        self.out = None

    def __call__(self, model, x, input_pos, temperature, top_p, top_k, semantic_logit_bias, audio_masks, audio_parts, previous_tokens=None):
        import torch

        if self.broken or audio_parts is not None or previous_tokens is None:
            return self.uncompiled(model, x, input_pos, temperature, top_p, top_k, semantic_logit_bias, audio_masks, audio_parts, previous_tokens)

        if self.graph is None or top_k != self.top_k:
            try:
                self.capture(model, x, input_pos, temperature, top_p, top_k, semantic_logit_bias, previous_tokens)
            except torch.cuda.OutOfMemoryError:
                # The card's room, not the compiling: the piece's own shortfall, spoken again by its clauses.
                self.forget()
                raise
            except Exception as failure:
                print(f"voice-fish: the frame step could not be compiled, so it runs uncompiled: {type(failure).__name__}: {failure}", file=sys.stderr, flush=True)
                self.broken = True
                self.forget()

                return self.uncompiled(model, x, input_pos, temperature, top_p, top_k, semantic_logit_bias, None, None, previous_tokens)

        frame_x, frame_pos, frame_previous, *sampled = self.inputs
        frame_x.copy_(x)
        frame_pos.copy_(input_pos)
        frame_previous.copy_(previous_tokens)

        # The sampling's tensors stand the same for every frame of a piece, so they are copied in only where the
        # generator hands in others; held here, none is freed and its identity taken by another.
        if self.sampling is None or any(held is not given for held, given in zip(self.sampling, (temperature, top_p, semantic_logit_bias))):
            for into, given in zip(sampled, (temperature, top_p, semantic_logit_bias)):
                into.copy_(given)
            self.sampling = (temperature, top_p, semantic_logit_bias)

        self.graph.replay()

        return self.out

    def capture(self, model, x, input_pos, temperature, top_p, top_k, semantic_logit_bias, previous_tokens) -> None:
        """The compiled frame captured as a graph on the inputs' own copies: compiled, where it is not yet, and run
        twice on a side stream first, as a capture asks, then captured; the generator's state put back after."""
        import torch

        started = time.monotonic()
        self.forget()
        inputs = [tensor.clone() for tensor in (x, input_pos, previous_tokens, temperature, top_p, semantic_logit_bias)]
        frame_x, frame_pos, frame_previous, frame_temperature, frame_top_p, frame_bias = inputs
        drawn = torch.cuda.get_rng_state()
        side = torch.cuda.Stream()
        side.wait_stream(torch.cuda.current_stream())

        with torch.cuda.stream(side):
            for _ in range(2):
                self.compiled(model, frame_x, frame_pos, frame_temperature, frame_top_p, top_k, frame_bias, frame_previous)

        torch.cuda.current_stream().wait_stream(side)
        graph = torch.cuda.CUDAGraph()

        with torch.cuda.graph(graph):
            out = self.compiled(model, frame_x, frame_pos, frame_temperature, frame_top_p, top_k, frame_bias, frame_previous)

        torch.cuda.set_rng_state(drawn)
        self.graph, self.inputs, self.out, self.top_k = graph, inputs, out, top_k
        self.sampling = (temperature, top_p, semantic_logit_bias)

        if self.captured_s is None:
            torch.cuda.synchronize()
            self.captured_s = time.monotonic() - started


class Mouth:
    """S2-Pro with one voice's adapter folded into its weights, loaded on the first line, on the first after it was let
    go, and for another voice.

    Loaded, the model stands on the card and nowhere else, and its weights are let go whole (`release`): no copy of
    them is kept in host memory, so giving the card away leaves the node's memory to the others, and the next line
    loads them again. The model's own modules are kept without their weights, with the step compiled for them, and
    the next load puts its weights into them (`refilled`), so that step is not compiled again.

    torch and fish_speech are imported inside rather than at the top of the file because they are what makes starting
    slow, and neither the encode hand-off nor the server's own start needs them.
    """

    def __init__(self, base: Path) -> None:
        self.base = base
        self.model = None
        self.codec = None
        self.checkpoint: Path | None = None
        self.sampling = {"temperature": 0.7, "top_p": 0.7}
        # The delivery clip the line is spoken on, as its words and its codes (`deliver`), None where it has none.
        self.reference: tuple | None = None
        # Each delivery clip read so far, by its file and when that was written: its codes are read once in a
        # process, on the host, and stand through every load after.
        self.clips: dict = {}
        # Why the last delivery clip could not be read, empty where it was or where the voice has none.
        self.clip_failure = ""
        # The frame step every piece is generated through (`FrameStep`), made with the first model.
        self.step: FrameStep | None = None
        # The model's modules kept without their weights while the card is elsewhere, None where none are kept.
        self.frame = None
        # Whether a frame is kept at all: off once keeping it was seen to buy nothing in this process.
        self.keeps = True
        # Whether the model that stands was put into a kept frame, until its first capture has been judged.
        self.refilled_now = False
        # What the last load took, in seconds: the weights and the codec.
        self.took = {"weights": 0.0, "codec": 0.0}

    def ready(self, checkpoint: Path) -> float:
        """The model holding this voice's weights; answers the seconds it took to get there."""
        started = time.monotonic()

        if self.model is None or self.checkpoint != checkpoint:
            said = declared(checkpoint)
            self.release()
            self.model = self.refilled(self.slow(checkpoint / "lora.pt"))
            self.took["weights"] = time.monotonic() - started
            self.codec = self.decoder()
            self.took["codec"] = time.monotonic() - started - self.took["weights"]

            if self.step is None:
                self.step = FrameStep()

            self.step.captured_s = None
            self.sampling = {"temperature": float(said.get("temperature", 0.7)), "top_p": float(said.get("top_p", 0.7))}
            self.checkpoint = checkpoint

        return time.monotonic() - started

    def refilled(self, loaded):
        """The model that speaks: the kept frame with the loaded model's weights put into it, so the step compiled for
        the frame still stands for it, or the loaded model itself where no frame is kept. A frame the weights do not
        fit is forsaken with what was compiled for it, and the loaded model speaks."""
        frame, self.frame = self.frame, None
        self.refilled_now = False

        if frame is None:
            return loaded

        try:
            refill(frame, loaded)
        except Exception as failure:
            print(f"voice-fish: the loaded weights did not go into the kept frame, so it is let go with what was compiled for it: {type(failure).__name__}: {failure}", file=sys.stderr, flush=True)
            self.forsake()

            return loaded

        self.refilled_now = True

        return frame

    def forsake(self) -> None:
        """No frame kept from here on in this process, and what was compiled let go."""
        import torch._dynamo

        self.keeps = False
        self.frame = None
        self.step = None
        torch._dynamo.reset()

    def judge_capture(self) -> None:
        """The first capture after weights went into a kept frame, judged once: one that took as long as compiling
        does means the step was compiled again, so keeping the frame buys nothing and is given up."""
        if not self.refilled_now or self.step is None or self.step.captured_s is None:
            return

        self.refilled_now = False

        if self.step.captured_s > RECOMPILED_S:
            print(f"voice-fish: the step was compiled again for a kept frame ({self.step.captured_s:.1f} s), so no frame is kept from here on", file=sys.stderr, flush=True)
            self.keeps = False

    def account(self) -> str:
        """What the last load and the line's first capture took, as the line's asker is told."""
        graph = self.step.captured_s if self.step is not None else None
        parts = [f"weights {self.took['weights']:.1f} s", f"codec {self.took['codec']:.1f} s"]

        if graph is not None:
            parts.append(f"graph {graph:.1f} s")

        parts.append("frame kept" if self.keeps else "frame not kept")
        parts.append("on its delivery clip" if self.reference is not None else "no delivery clip")

        if self.clip_failure:
            parts.append(f"the clip could not be read: {self.clip_failure}")

        return ", ".join(parts)

    def slow(self, adapter: Path):
        """The two autoregressive models with the adapter folded in, on the card in bf16.

        They are built on the meta device and the checkpoint's bf16 tensors assigned in, so the host never holds a
        float32 copy of four billion weights; the buffers that are computed rather than stored are made again. Each
        linear the adapter names gets B times A at its scale added in float32 and written back in its own dtype,
        before anything goes on the card.
        """
        import torch
        from fish_speech.models.text2semantic.llama import DualARTransformer, precompute_freqs_cis
        # Imported before the meta device is entered: the tokenizer's library cannot be imported inside it.
        from fish_speech.tokenizer import FishTokenizer  # noqa: F401

        with torch.device("meta"):
            model = DualARTransformer.from_pretrained(str(self.base), load_weights=True, max_length=CONTEXT)

        config = model.config
        model.freqs_cis = precompute_freqs_cis(config.max_seq_len, config.head_dim, config.rope_base)
        model.causal_mask = torch.tril(torch.ones(config.max_seq_len, config.max_seq_len, dtype=torch.bool))
        model.fast_freqs_cis = precompute_freqs_cis(config.num_codebooks, config.fast_head_dim, config.rope_base)
        left = [name for name, tensor in [*model.named_parameters(), *model.named_buffers()] if tensor.is_meta]

        if left:
            raise RuntimeError(f"the base weights left {len(left)} tensors unloaded, {left[0]} first")

        folded = torch.load(adapter, map_location="cpu", weights_only=True)
        scale = float(folded["scale"])

        for name in sorted({key.rsplit(".", 1)[0] for key in folded["state"]}):
            linear = model.get_submodule(name)
            a = folded["state"][f"{name}.lora_A"].float()
            b = folded["state"][f"{name}.lora_B"].float()
            linear.weight.data = (linear.weight.data.float() + scale * (b @ a)).to(linear.weight.dtype)

        model = model.to(device="cuda", dtype=torch.bfloat16).eval()
        # The generator sets its key-value caches up on its first piece, at the context above.
        model._cache_setup_done = False

        return model

    def deliver(self, checkpoint: Path, language: str | None) -> None:
        """The delivery clip the line is spoken on, as `self.reference`: the clip of its language where the voice
        has one, else the voice's own (`delivery_of`)."""
        self.reference = self.delivery_clip(checkpoint, declared(checkpoint), language)

    def delivery_clip(self, checkpoint: Path, said: dict, language: str | None = None) -> tuple | None:
        """The delivery clip of a line in `language` as S2-Pro reads a reference (`delivery_of`): the words
        `voice.toml` says it speaks and the codes the codec reads from its file, mixed to one channel at the codec's
        rate. A clip that cannot be read is told and the voice speaks without it, since a line is worth more than its
        delivery. A clip is read once in a process: its codes stand on the host through every load after, beside every
        other clip read, and one that could not be read is read again by the next line."""
        self.clip_failure = ""
        chosen = delivery_of(checkpoint, said, language)

        if chosen is None:
            return None

        clip, words = chosen
        read = (str(clip), clip.stat().st_mtime_ns, words)

        if read in self.clips:
            return self.clips[read]

        import torch

        for again in (False, True):
            try:
                import torchaudio

                audio, rate = sf.read(clip, dtype="float32", always_2d=True)
                mono = torchaudio.functional.resample(torch.from_numpy(audio.mean(axis=1))[None], rate, self.codec.sample_rate)

                with torch.inference_mode():
                    heard = mono[None].to(device="cuda", dtype=torch.bfloat16)
                    codes, _ = self.codec.encode(heard, torch.tensor([heard.shape[2]], device="cuda", dtype=torch.long))

                self.clips[read] = (words, (codes[0] if codes.ndim == 3 else codes).cpu())
                self.clip_failure = ""

                return self.clips[read]
            except Exception as failure:
                self.clip_failure = f"{type(failure).__name__}: {failure}"[:200]
                print(f"voice-fish: the delivery clip of {checkpoint.name} could not be read{'' if again else ', so it is read once more on an emptied cache'}: {self.clip_failure}", file=sys.stderr, flush=True)
                torch.cuda.empty_cache()

        return None

    def decoder(self):
        """The codec, with each of its transformers' attention mask made at that transformer's own block length
        rather than the 32,768 its code builds them at, cast to bf16 on the host and put on the card once."""
        import hydra
        import torch
        from hydra import compose, initialize_config_dir
        from hydra.utils import instantiate

        # Imported for the resolvers it registers, and for where the package's configurations stand beside it: the
        # package is a namespace one, with no file of its own to be found by.
        import fish_speech.models.dac.inference as codec_code

        hydra.core.global_hydra.GlobalHydra.instance().clear()

        with initialize_config_dir(version_base="1.3", config_dir=str(Path(codec_code.__file__).parents[2] / "configs")):
            codec = instantiate(compose(config_name="modded_dac_vq"))

        for module in codec.modules():
            block = getattr(getattr(module, "config", None), "block_size", None)

            if isinstance(block, int) and isinstance(getattr(module, "causal_mask", None), torch.Tensor):
                module.causal_mask = torch.tril(torch.ones(block, block, dtype=torch.bool))

        state = torch.load(self.base / "codec.pth", map_location="cpu", mmap=True, weights_only=True)
        state = state.get("state_dict", state)

        if any("generator" in key for key in state):
            state = {key.replace("generator.", ""): value for key, value in state.items() if "generator." in key}

        codec.load_state_dict(state, strict=False, assign=True)
        codec.eval()
        codec.to(dtype=torch.bfloat16)

        return codec.to("cuda")

    def speak(self, checkpoint: Path, text: str, seed: int, style: str = "", by_sentence: bool = False, told=None,
              language: str | None = None):
        """The line in the voice those weights carry, said in `language`, peak-matched, at `RATE` (`pieces`, joined
        0.3 seconds apart), each piece handed to `told` by its place and the count of pieces as it is begun."""
        pieces, rate = self.pieces(checkpoint, text, seed, style, by_sentence, told, language)
        pause = np.zeros(int(SENTENCE_PAUSE * rate), dtype=np.float32)
        joined: list[np.ndarray] = []

        for wav in pieces:
            if joined:
                joined.append(pause)

            joined.append(wav)

        audio = resampled(np.concatenate(joined), rate)
        peak = float(np.max(np.abs(audio))) or 1.0

        return (audio / peak * PEAK).astype(np.float32), RATE

    def pieces(self, checkpoint: Path, text: str, seed: int, style: str = "", by_sentence: bool = False, told=None,
               language: str | None = None):
        """The line's pieces at the codec's rate, each levelled (`levelled`) and faded at both edges (`faded`): a
        sentence at a time where asked (`sentences`), in passes the context holds otherwise (`passes`). Every piece
        is spoken on from the voice's delivery clip for the line's language where it has one (`deliver`), and else
        each piece after the first on from the first (`piece`); every piece on the same seed, so the same line gives
        back the same samples every time."""
        self.ready(checkpoint)
        self.deliver(checkpoint, language)
        rate = self.codec.sample_rate
        # What every piece of the line is spoken on from: the voice's delivery clip, or else the line's first piece,
        # as its words and the codes the model made for them, once it is spoken.
        carried: dict = {"first": self.reference}
        out: list[np.ndarray] = []

        parts = sentences(text) if by_sentence else passes(text)

        for at, piece in enumerate(parts, 1):
            if told is not None:
                told(at, len(parts))

            for wav in self.spoken(piece, lambda words: self.piece(styled(style, words), seed, carried)):
                out.append(faded(levelled(wav), rate))

        return out, rate

    def spoken(self, piece: str, say) -> list:
        """The piece as `say` speaks it, as one wav; or, where the card runs out of memory for it, its sentences
        spoken one by one once the allocator's cache is emptied, each cut at its clauses into parts of half
        `MOST_PIECE`. A second shortfall is the line's failure."""
        import gc

        import torch

        try:
            return [say(piece)]
        except torch.cuda.OutOfMemoryError:
            pass

        print("voice-fish: the card ran out of memory for a piece, so it is spoken again by its clauses", file=sys.stderr, flush=True)
        self.drop_caches()
        gc.collect()
        torch.cuda.empty_cache()

        return [say(part) for start, end in sentence_spans(piece) for part in clauses(piece[start:end], MOST_PIECE // 2)]

    def piece(self, words: str, seed: int, carried: dict) -> np.ndarray:
        """One piece on the line's seed, asked with what the line carries as its reference: the voice's delivery clip,
        or the line's first piece once that one is spoken, as words and codes, which carry its timbre, pitch and pace
        into this one. Where the two would not fit the prompt's share of the context, the piece is asked bare. A piece
        that stops short (`stops_short`) is asked once more on the same reference on the seed after the line's, and
        only where that stops short too is it asked bare, and that one is kept. With no clip, the first piece is asked
        bare and carried for the rest of the line."""
        import torch
        from fish_speech.models.text2semantic.errors import InvalidInputError
        from fish_speech.models.text2semantic.inference import generate_long

        def spoken(reference, on: int = seed) -> tuple:
            random.seed(on)
            np.random.seed(on)
            torch.manual_seed(on)
            asked = {} if reference is None else {"prompt_text": [reference[0]], "prompt_tokens": [reference[1]]}
            codes = []
            answers = generate_long(
                model=self.model, device="cuda", decode_one_token=self.step, text=words,
                max_new_tokens=MOST_NEW_TOKENS, compile=False, **asked, **self.sampling,
            )

            for answer in answers:
                if answer.action == "sample" and answer.codes is not None:
                    codes.append(answer.codes)
                elif answer.action == "next":
                    break

            answers.close()

            if not codes:
                raise RuntimeError("the model closed its turn before it spoke")

            made = torch.cat(codes, dim=1)

            if made.shape[1] > BESIDE_CACHES:
                # The codec's pass grows with the piece: past what fits beside them, the caches and the graph the
                # next piece makes again are let go first.
                self.drop_caches()
                torch.cuda.empty_cache()

            audio = self.codec.from_indices(made.to("cuda")[None])[0].squeeze().float().cpu().numpy()

            return made, audio

        with torch.inference_mode():
            first = carried["first"]

            try:
                made, audio = spoken(first)
            except InvalidInputError:
                # The first piece and this one's text over the prompt's share: this one is asked bare.
                first = None
                made, audio = spoken(None)

            if first is not None and stops_short(audio, self.codec.sample_rate):
                print("voice-fish: a piece stopped short on its reference, so it is spoken again on the next seed", file=sys.stderr, flush=True)
                made, audio = spoken(first, seed + 1)

                if stops_short(audio, self.codec.sample_rate):
                    print("voice-fish: the piece stopped short again, so it is spoken bare", file=sys.stderr, flush=True)
                    made, audio = spoken(None)

        if carried["first"] is None:
            carried["first"] = (words, made.cpu())

        return audio

    def held(self):
        """Every tensor the loaded models hold, each once: their parameters, their buffers (the key-value caches
        among them), and the tensors their modules keep as plain attributes."""
        import torch

        seen: set[int] = set()

        for model in (self.model, self.codec):
            if model is None:
                continue

            for module in model.modules():
                owned = [*module._parameters.values(), *module._buffers.values()]
                owned.extend(value for value in vars(module).values() if isinstance(value, torch.Tensor))

                for tensor in owned:
                    if tensor is not None and id(tensor) not in seen:
                        seen.add(id(tensor))
                        yield tensor

    def drop_caches(self) -> None:
        """The key-value caches let go, so the next piece makes them again on the card. They are scratch every piece
        writes before it reads. The frame step's graph, which reads them where they stand, is let go first."""
        if self.step is not None:
            self.step.forget()

        for layer in (*self.model.layers, *self.model.fast_layers):
            layer.attention.kv_cache = None

        self.model.max_seq_len = -1
        self.model.max_batch_size = -1
        self.model._cache_setup_done = False

    def release(self) -> None:
        """The card given back and the host's memory with it: the graph and the caches let go, every tensor the models
        hold on the card emptied in place, the models dropped, the allocator's cache emptied and the C heap trimmed,
        whether or not a model was held, since a load that failed half way leaves what it allocated there. The tensors
        are emptied rather than left to their last reference, which the frames of a failed line's traceback may hold
        while this runs."""
        import gc

        import torch

        if self.model is not None:
            self.drop_caches()

        kept = self.model if self.keeps and self.step is not None and not getattr(self.step, "broken", False) else None

        for tensor in list(self.held()):
            if tensor.is_cuda:
                try:
                    tensor.data = torch.empty(0, dtype=tensor.dtype)
                except RuntimeError as failure:
                    print(f"voice-fish: a tensor could not be emptied, so it stands until its last reference goes: {failure}", file=sys.stderr, flush=True)

        if self.model is not None:
            self.frame = kept

        self.model = None
        self.codec = None
        self.reference = None
        self.checkpoint = None

        if self.step is not None and self.frame is None:
            # With no frame kept, what was compiled for the models let go is let go with them; the kernels stay on the
            # disk.
            import torch._dynamo

            self.step = None
            torch._dynamo.reset()

        gc.collect()

        if torch.cuda.is_available():
            torch.cuda.empty_cache()

        try:
            ctypes.CDLL("libc.so.6").malloc_trim(0)
        except (OSError, AttributeError):
            pass  # not glibc: nothing to trim this way


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
    model is let go whole and the card with it (docs/voice-plan.md, 「How it holds the card」). With no line
    waiting the turn rests with the model on the card, until the arbiter says another wants the card
    (watch_card); the next line after that loads the model again."""

    def __init__(self, voices: Path, base: Path, token: str, card: "Card", room_every_s: float = 0.5) -> None:
        self.voices = voices
        self.token = token
        self.card = card
        # How often a card just taken is asked whether its memory takes the model yet.
        self.room_every_s = room_every_s
        self.mouth = Mouth(base)
        self.lock = threading.Lock()
        self.busy = False
        # When each line waiting behind the one being spoken arrived, guarded apart from the line's lock.
        self.waiting: list[float] = []
        self.waiting_lock = threading.Lock()
        # Woken when the card starts to rest, which the watcher then asks after.
        self.watch_wake = threading.Event()

    def health(self) -> dict:
        import torch

        free = card_free_bytes()
        elsewhere = self.card.elsewhere()

        return {
            "loaded": None if self.mouth.checkpoint is None else self.mouth.checkpoint.name,
            "busy": self.busy,
            "onCard": self.mouth.model is not None,
            "device": torch.cuda.get_device_name(0) if torch.cuda.is_available() else "cpu",
            "vramBytes": torch.cuda.memory_allocated() if torch.cuda.is_available() else 0,
            "torch": torch.__version__,
            "cardFreeBytes": free,
            # Whether other work holds the node's shared GPU lock this moment.
            "cardTaken": elsewhere,
            # Whether this server holds the card itself, between lines that carry on or resting on it.
            "holding": self.card.holding,
            "resting": self.card.resting,
            # Whether a line could be spoken now: the card this server's own, or nobody else on it and
            # room on it for the weights (fits), what this server holds itself counted as room.
            "room": self.card.holding or (not elsewhere and self.fits(card_room_bytes())),
        }

    def fits(self, free: int | None) -> bool:
        """Whether the card's free memory takes a load of the model. A card the driver cannot be asked
        about is given the benefit of the doubt."""
        return free is None or free >= LOAD_BYTES

    def room(self, stage: LineStage, gone) -> None:
        """The card just taken, waited on until its memory takes the model, the memory this server holds itself
        counted as room (card_room_bytes), the wait written to the line's stage once. A turn is handed on the moment
        the holder before gives it back, and a holder can give it back while its process still stands on the card, so
        the turn alone never says the memory is free. A line whose asker has gone meanwhile lets the card go and is
        dropped; the engine's clock ends one that waits here too long."""
        told = False

        while True:
            room = card_room_bytes()

            if self.fits(room):
                return

            if not told:
                stage.tell("room", f"waits for room on the card for the model: {gib(room)} free of the {gib(LOAD_BYTES)} it needs")
                told = True

            if gone():
                self.let_go()
                raise Abandoned("the line's asker went while it waited for room on the card")

            time.sleep(self.room_every_s)

    def stage_file(self, named: str | None) -> Path | None:
        """The call's stage file the asker names: it stands in the server's call folder beside the voices, `spoken/`,
        and any other is refused; none where the asker names none."""
        if not named:
            return None

        path = Path(named)

        if path.suffix != ".stage" or path.parent.resolve() != (self.voices.parent / "spoken").resolve():
            raise ValueError("a stage file stands in the server's call folder")

        return path

    def speak(self, request: dict, stage: LineStage | None = None, gone=None) -> tuple[bytes, dict]:
        """One line spoken, each stage it reaches written to `stage`; a line whose asker has gone (`gone`) before it
        takes the card, or while it waits for room on it, is dropped (Abandoned)."""
        stage = stage or LineStage()
        gone = gone or (lambda: False)
        checkpoint = inside(self.voices, request.get("checkpoint"))
        text = request.get("text")

        if checkpoint is None or not isinstance(text, str) or not text.strip():
            raise ValueError("a request names a voice folder under the voices root, and a text")

        style = delivery(request.get("style"))
        by_sentence = request.get("sentences", False)
        language = request.get("language")

        if not isinstance(by_sentence, bool):
            raise ValueError("sentences is true or false")

        if language is not None and not (isinstance(language, str) and LANGUAGE.fullmatch(language)):
            raise ValueError("language is a language code of two or three lower-case letters")

        declared(checkpoint)
        arrived = time.monotonic()

        with self.waiting_lock:
            self.waiting.append(arrived)

        try:
            if self.lock.locked():
                stage.tell("queued", "waits behind another line on the voice server")

            self.lock.acquire()
        finally:
            with self.waiting_lock:
                self.waiting.remove(arrived)

        try:
            if gone():
                raise Abandoned("the line's asker went while it waited behind another line")

            self.busy = True
            if self.card.resting and not self.card.resume(time.monotonic() - arrived):
                # Another stands first for the card: the rest ends in giving way, and the line asks in line.
                self.let_go()
            fresh = not self.card.holding

            if fresh:
                stage.tell("turn", "waits for its turn on the card")

            self.card.take(time.monotonic() - arrived)

            if fresh:
                self.room(stage, gone)

            try:
                if self.mouth.model is None or self.mouth.checkpoint != checkpoint:
                    stage.tell("load", "loads the voice")

                loaded = self.mouth.ready(checkpoint)
                started = time.monotonic()
                audio, rate = self.mouth.speak(
                    checkpoint, text.strip(), int(request.get("seed", 0)), style, by_sentence, told=stage.piece,
                    language=language,
                )
                spoke = time.monotonic() - started
                self.mouth.judge_capture()
            except BaseException:
                if getattr(self.mouth, "refilled_now", False):
                    # The first line on a kept frame failed: whatever failed it, the frame is not trusted again.
                    self.mouth.forsake()

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
            "X-Voice-Load-Parts": self.mouth.account(),
            "X-Voice-Checkpoint": checkpoint.name,
        }

    def after_line(self) -> None:
        """The card after a line that was spoken: kept for the next line waiting while it still stands first
        for the card, let go where another stands first, and with no line waiting rested on, or let go where
        somebody already wants it or the arbiter keeps no rests."""
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
                print(f"voice-fish: watching the card failed: {type(failure).__name__}: {failure}", file=sys.stderr, flush=True)
                asked = True

            if not asked:
                self.watch_wake.wait(1.0)
                self.watch_wake.clear()
            elif time.monotonic() - asked_at < 0.05:
                time.sleep(again_s)

    def watch_step(self) -> bool:
        """One ask after the resting card's ticket, its answer held open until another wants the card; False where
        the card is not resting. What the answer calls for is done under the line's lock, and only where no line is in
        hand and the card still rests as it did when asked: a line being spoken holds that lock, so it is never given
        way under, and a line that came meanwhile has the card its own way."""
        card = self.card
        ticket = None if card.turn is None else card.turn.ticket

        if ticket is None or not card.resting:
            return False

        answered = card.turn.watch(ticket)

        with self.lock:
            with self.waiting_lock:
                in_hand = bool(self.waiting)

            if in_hand or card.turn.ticket != ticket or not card.resting:
                return True

            if answered is None or not answered.turn or answered.give_way_for is not None:
                # Asked for by another, or the arbiter no longer knows the turn: the model let go whole.
                self.let_go()

        return True

    def let_go(self) -> None:
        """The model let go whole and the card with it, at once, however the line ended. The card's lock is
        let go even where the model could not be, so a failure here never leaves the card locked."""
        try:
            self.mouth.release()
        finally:
            self.card.let_go()


class CardTaken(Exception):
    """The node's card arbiter gives the voice no place on the card."""


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
        # The turn resting with the model on the card, nothing left to speak (docs/card.md, 「Resting」).
        self.resting = False

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

    def take(self, waited: float = 0.0) -> None:
        """The lock taken once other work lets go of it, however long that takes, for a line that has
        waited `waited` already, and kept until `let_go`; taking it again while held does nothing.

        Where the node keeps a hold queue, the line waits its turn in it, the arbiter holding its answer
        open until the card is handed on, and stands there by the line's arrival. Elsewhere the lock is
        waited for as flock waits, until it is let go.
        """
        if self.handle is not None:
            return

        if self.turn is not None:
            from turn import hold

            try:
                handle = hold(self.turn, self.path, waited_s=waited)
            except PermissionError as refused:
                raise CardTaken(str(refused)) from None
            if handle is None:
                raise CardTaken("the turn client answered without the voice's turn")
            self.handle = handle
            return

        handle = os.open(self.path, os.O_RDWR | os.O_CREAT, 0o644)

        try:
            fcntl.flock(handle, fcntl.LOCK_EX)
        except BaseException:
            os.close(handle)
            raise

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
            print(f"voice-fish: {self.command} {self.path} {values[1] if len(values) > 1 else ''}", file=sys.stderr, flush=True)

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

        def gone(self) -> bool:
            """Whether the asker has let go of its connection: readable with nothing to read is its end."""
            try:
                readable, _, _ = select.select([self.connection], [], [], 0)

                return bool(readable) and self.connection.recv(1, socket.MSG_PEEK) == b""
            except OSError:
                return True

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
                stage = LineStage(service.stage_file(self.headers.get("X-Voice-Stage")))
                wav, headers = service.speak(request if isinstance(request, dict) else {}, stage, self.gone)
            except (ValueError, FileNotFoundError) as refusal:
                return self.refuse(400, str(refusal))
            except Abandoned as why:
                # Nobody waits for the answer: the connection is closed with none.
                print(f"voice-fish: a line was dropped: {why}", file=sys.stderr, flush=True)
                self.close_connection = True
                return
            except CardTaken as taken:
                # Never told when to come back: the caller chooses a node again at once (docs/voice-plan.md).
                return self.refuse(503, str(taken))
            except Exception as failure:  # the model's own failure, said without the line
                print(f"voice-fish: speaking failed: {type(failure).__name__}: {failure}", file=sys.stderr, flush=True)
                traceback.print_exc(file=sys.stderr)
                return self.refuse(500, f"speaking failed: {failed_at(failure)}")

            self.answer(200, wav, "audio/wav", headers)

    return Handler


def serve(args: argparse.Namespace) -> int:
    token_file = Path(args.token_file)

    if not token_file.is_file() or token_file.stat().st_mode & 0o777 != 0o600:
        print(f"voice-fish: the token file must exist and be 0600: {token_file}", file=sys.stderr)
        return 1

    token = token_file.read_text(encoding="utf-8").strip()

    if token == "":
        print(f"voice-fish: the token file is empty: {token_file}", file=sys.stderr)
        return 1

    card = Card(Path(args.gpu_lock).expanduser(), card_turn())
    service = Service(Path(args.voices), Path(args.base).expanduser(), token, card)
    server = ThreadingHTTPServer((args.host, args.port), handler(service))
    threading.Thread(target=service.watch_card, daemon=True).start()
    print(f"voice-fish: serving on {args.host}:{args.port} · voices {args.voices} · card lock {card.path}", file=sys.stderr, flush=True)
    server.serve_forever()

    return 0


def ask(args: argparse.Namespace) -> int:
    """One call to the server on this machine's loopback, its end printed as one line of JSON.

    Every way the call can end is printed rather than raised, so the caller on the other side of the
    node's shell reads what happened from that line alone: a status where the server answered,
    `unreachable` or `token` where it never did. The answer is waited for however long the server takes.
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

    if args.stage:
        headers["X-Voice-Stage"] = args.stage

    try:
        with urllib.request.urlopen(urllib.request.Request(url, data=body, headers=headers)) as answer:
            said["status"] = answer.status
            data = answer.read()

            if body is None:
                said["health"] = json.loads(data)
            else:
                Path(args.wav).write_bytes(data)
                said["seconds"] = float(answer.headers.get("X-Voice-Seconds") or 0)
                said["loadSeconds"] = float(answer.headers.get("X-Voice-Load-Seconds") or 0)
                said["speakSeconds"] = float(answer.headers.get("X-Voice-Speak-Seconds") or 0)
                said["loadParts"] = answer.headers.get("X-Voice-Load-Parts") or ""
                said["checkpoint"] = answer.headers.get("X-Voice-Checkpoint") or ""
    except urllib.error.HTTPError as refusal:
        said["status"] = refusal.code

        try:
            said["error"] = str(json.loads(refusal.read()).get("error", ""))[:MOST_ERROR] or None
        except ValueError:
            said["error"] = None
    except urllib.error.URLError as unreached:
        said["error"] = f"unreachable: {unreached.reason}"
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
        print("voice-fish: nothing to say", file=sys.stderr)
        return 1

    if not args.out or not args.checkpoint:
        print("voice-fish: speaking once needs --out and --checkpoint", file=sys.stderr)
        return 1

    checkpoint = Path(args.checkpoint)

    # Said before a model is loaded rather than after: half a minute of loading to find out that the weights are not
    # there is half a minute the room spends waiting for nothing.
    try:
        declared(checkpoint)
        style = delivery(args.style)
    except (ValueError, FileNotFoundError) as refusal:
        print(f"voice-fish: {refusal}", file=sys.stderr)
        return 1

    audio, rate = Mouth(Path(args.base).expanduser()).speak(checkpoint, text, args.seed, style, args.sentences)

    with tempfile.TemporaryDirectory(prefix="bmo-voice-fish-") as room:
        spoken = Path(room) / "spoken.wav"
        sf.write(spoken, audio, rate, subtype="PCM_16")
        print(f"voice-fish: {len(audio) / rate:.1f}s · {rate} Hz · {checkpoint.name}", file=sys.stderr)

        return encode(args.encoder_python, spoken, args)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--text")
    parser.add_argument("--text-file")
    parser.add_argument("--out")
    parser.add_argument("--wav")
    parser.add_argument("--checkpoint")
    parser.add_argument("--base", default=BASE)
    parser.add_argument("--style")
    parser.add_argument("--encoder-python", default=sys.executable)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--sentences", action="store_true")
    parser.add_argument("--serve", action="store_true")
    parser.add_argument("--voices")
    parser.add_argument("--token-file")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--ask", action="store_true")
    parser.add_argument("--request")
    parser.add_argument("--stage")
    parser.add_argument("--port", type=int, default=7432)
    parser.add_argument("--gpu-lock", default=GPU_LOCK)
    args = parser.parse_args()

    if args.serve:
        if not args.voices or not args.token_file:
            print("voice-fish: serving needs --voices and --token-file", file=sys.stderr)
            return 1

        return serve(args)

    if args.ask:
        if not args.token_file or (args.request and not args.wav):
            print("voice-fish: asking needs --token-file, and --wav beside --request", file=sys.stderr)
            return 1

        return ask(args)

    return once(args)


if __name__ == "__main__":
    raise SystemExit(main())
