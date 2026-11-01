#!/usr/bin/env python
"""Speaks a line in an agent's own trained voice: Fish Audio S2-Pro with the voice's adapter folded into its weights.

The voice is a LoRA fine-tune of S2-Pro's slow autoregressive model on the agent's own recorded lines, spoken with no
reference clip (docs/voice-plan.md, 「The voices」). What picks a run's weights is the voice folder named by the
caller, never a table this file reads for itself: the row was chosen by the caller, and a mouth that went looking for
its own settings could speak in a voice nobody asked for.

It works three ways; the first two speak through the same `Mouth`, and the third asks the second:

- **Once** (the default): speak one line on this machine's card, write a wav, and hand that wav to
  `voice-encode.py`, in a process of its own, for the OGG/Opus encode.
- **As a server** (`--serve`): the voice server on each voice node (docs/voice-plan.md, 「The voice nodes」),
  listening on the node's loopback. It answers `POST /speak` with the spoken line as a wav and `GET /health` with
  what it holds. The card is shared under the node's shared GPU lock (`Card`): the model stays on the card between
  lines until another occupant asks for it, and waits in page-locked host memory while it is off. Encoding stays on
  the caller's side, where the encoder lives.
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
       [--host 127.0.0.1] [--port 7432] [--gpu-lock ~/.gpu.lock] [--gpu-wait 60]
       <engine venv>/bin/python synth/voice-fish.py --ask --token-file <path> [--port 7432]
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
import tomllib
import traceback
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import numpy as np
import soundfile as sf

# What the whole thing is kept under. It is the same number `synth/voice-encode.py` holds its ceiling at.
PEAK = 0.95

# The rate a line leaves at: the one OGG/Opus is written at. The codec speaks at 44.1 kHz, so the mouth resamples
# once, the one change it makes to the sound (docs/voice-plan.md, 「What the sound carries」).
RATE = 48000

# The engine a voice folder must name in its `voice.toml` to be spoken here: the policy's (`policy/voice.toml`).
ENGINE = "fish-s2-pro"

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

# How a line is cut into sentences: at a full stop, a question or exclamation mark or an ellipsis before a space,
# and at every line break.
SENTENCE_END = re.compile(r"(?<=[.?!…])\s+|\n+")

# This many seconds of silence stand between two pieces of a line.
SENTENCE_PAUSE = 0.3

# The loudness every piece of a line is brought to before the line is joined (docs/voice-plan.md, 「What the sound
# carries」): its RMS over its voiced frames, those within `VOICED_DB` of its loudest frame, in dBFS.
PIECE_RMS_DB = -20.0
VOICED_DB = 40.0
LEVEL_FRAME = 1024

# How long each edge of a spoken piece is faded over, in seconds (docs/voice-plan.md, 「The voices」).
EDGE_FADE = 0.01

# The server's own limits. A line longer than this is not a briefing anybody asked for, and a body is read whole
# into memory, so it is bounded before it is read.
MOST_TEXT = 4000
MOST_BODY = 64 * 1024
# A delivery instruction is a sentence, not a script: anything longer is a mistake in a row.
MOST_STYLE = 300
# The most characters of a refusal the asker hands on: room for a failure's kind, message and frame.
MOST_ERROR = 600

# What loading the model takes of the card: the server's process peaks at 11,295 MiB while a line is spoken on an
# RTX 4070, its own CUDA context among them, so a card with less free than this cannot take a load; the health says
# so (docs/voice-plan.md, 「What it answers」).
LOAD_BYTES = int(11.0 * 1024**3)

# What putting the kept weights back on the card takes for one line, once the server's own CUDA context is there.
UP_BYTES = int(10.7 * 1024**3)

# The node's shared GPU lock: every service that puts work on the node's card holds this one file under flock for
# as long as its work is there, so the services take the card in turn rather than all at once.
GPU_LOCK = "~/.gpu.lock"

# How long a line waits for other work to let go of the card, in seconds; work that holds it longer is told to the
# caller as the card taken.
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


class Mouth:
    """S2-Pro with one voice's adapter folded into its weights, loaded on the first line and again for another voice.

    Loaded, the model can be put down (`down`): every tensor it and the codec hold moved into page-locked host memory
    and the card given back, and put up again (`up`) for the next line at the speed of the bus rather than of the
    disk. The model is the same objects throughout; only where their storage stands changes.

    torch and fish_speech are imported inside rather than at the top of the file because they are what makes starting
    slow, and neither the encode hand-off nor the server's own start needs them.
    """

    def __init__(self, base: Path) -> None:
        self.base = base
        self.model = None
        self.codec = None
        self.checkpoint: Path | None = None
        self.sampling = {"temperature": 0.7, "top_p": 0.7}
        # Every tensor the models hold, with its host copy, by the tensor's identity; the tensor itself is kept
        # beside its copy, so the identity is never reused while the entry stands.
        self.kept: dict[int, tuple] = {}
        self.on_card = False

    def ready(self, checkpoint: Path) -> float:
        """The model holding this voice's weights; answers the seconds it took to get there."""
        started = time.monotonic()

        if self.model is None or self.checkpoint != checkpoint:
            said = declared(checkpoint)
            self.release()
            self.model = self.slow(checkpoint / "lora.pt")
            self.codec = self.decoder()
            self.sampling = {"temperature": float(said.get("temperature", 0.7)), "top_p": float(said.get("top_p", 0.7))}
            self.checkpoint = checkpoint
            self.on_card = True

        return time.monotonic() - started

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

    def speak(self, checkpoint: Path, text: str, seed: int, style: str = "", by_sentence: bool = False):
        """The line in the voice those weights carry, peak-matched, at `RATE` (`pieces`, joined 0.3 seconds apart)."""
        pieces, rate = self.pieces(checkpoint, text, seed, style, by_sentence)
        pause = np.zeros(int(SENTENCE_PAUSE * rate), dtype=np.float32)
        joined: list[np.ndarray] = []

        for wav in pieces:
            if joined:
                joined.append(pause)

            joined.append(wav)

        audio = resampled(np.concatenate(joined), rate)
        peak = float(np.max(np.abs(audio))) or 1.0

        return (audio / peak * PEAK).astype(np.float32), RATE

    def pieces(self, checkpoint: Path, text: str, seed: int, style: str = "", by_sentence: bool = False):
        """The line's pieces at the codec's rate, each levelled (`levelled`) and faded at both edges (`faded`): a
        sentence at a time where asked (`sentences`), in passes the context holds otherwise (`passes`). Each piece
        after the first is spoken on from the one before it (`piece`), and every piece on the same seed, so the same
        line gives back the same samples every time."""
        self.ready(checkpoint)
        rate = self.codec.sample_rate
        # The piece before, as its words and the codes the model made for them; none before the first.
        carried: dict = {"before": None}
        out: list[np.ndarray] = []

        for piece in sentences(text) if by_sentence else passes(text):
            for wav in self.spoken(piece, lambda words: self.piece(styled(style, words), seed, carried)):
                out.append(faded(levelled(wav), rate))

        return out, rate

    def spoken(self, piece: str, say) -> list:
        """The piece as `say` speaks it, as one wav; or, where the card runs out of memory for it, its sentences
        spoken one by one once the allocator's cache is emptied. A second shortfall is the line's failure."""
        import gc

        import torch

        try:
            return [say(piece)]
        except torch.cuda.OutOfMemoryError:
            pass

        print("voice-fish: the card ran out of memory for a piece, so it is spoken again by its sentences", file=sys.stderr, flush=True)
        gc.collect()
        torch.cuda.empty_cache()

        return [say(sentence) for sentence in sentences(piece)]

    def piece(self, words: str, seed: int, carried: dict) -> np.ndarray:
        """One piece on the line's seed, asked with the piece before it as its reference where there is one: that
        piece's words and its generated codes, which carry its timbre, pitch and pace into this one. Where the two
        would not fit the prompt's share of the context the piece is asked bare. What this piece made is then carried
        for the next, and its codes are decoded by the codec."""
        import torch
        from fish_speech.models.text2semantic.errors import InvalidInputError
        from fish_speech.models.text2semantic.inference import decode_one_token_ar, generate_long

        def generated(before) -> list:
            random.seed(seed)
            np.random.seed(seed)
            torch.manual_seed(seed)
            reference = {} if before is None else {"prompt_text": [before[0]], "prompt_tokens": [before[1]]}
            codes = []

            for answer in generate_long(
                model=self.model, device="cuda", decode_one_token=decode_one_token_ar, text=words,
                max_new_tokens=MOST_NEW_TOKENS, compile=False, **reference, **self.sampling,
            ):
                if answer.action == "sample" and answer.codes is not None:
                    codes.append(answer.codes)
                elif answer.action == "next":
                    break

            return codes

        with torch.inference_mode():
            try:
                codes = generated(carried["before"])
            except InvalidInputError:
                # The piece before and this one's text over the prompt's share: this one is asked bare.
                codes = generated(None)

            if not codes:
                raise RuntimeError("the model closed its turn before it spoke")

            spoken = torch.cat(codes, dim=1)
            carried["before"] = (words, spoken.cpu())
            audio = self.codec.from_indices(spoken.to("cuda")[None])[0].squeeze()

        return audio.float().cpu().numpy()

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

    def down(self) -> None:
        """The models put in page-locked host memory and the card given back.

        The first time a tensor is put down its host copy is made from the card; after that the copy already holds
        what the card held, since speaking changes no weight. A tensor the models no longer hold is let go with its
        copy, and one they hold for the first time is copied like any other. Whatever the allocator still caches is
        emptied however the models stood, and the C heap is trimmed.
        """
        import torch

        if self.model is not None and self.on_card:
            self.drop_caches()
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

    def drop_caches(self) -> None:
        """The key-value caches let go, so the next piece makes them again on the card. They are scratch every piece
        writes before it reads, and they are made inside inference mode: a cache carried down into host memory and put
        back up refuses the next piece's write into it ("Inference tensors do not track version counter")."""
        for layer in (*self.model.layers, *self.model.fast_layers):
            layer.attention.kv_cache = None

        self.model.max_seq_len = -1
        self.model.max_batch_size = -1
        self.model._cache_setup_done = False

    def up(self) -> float:
        """The models put back on the card from their host copies; answers the seconds it took."""
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
        """The card given back and the host copies with it: the models dropped and the allocator's cache emptied,
        whether or not a model was held, since a load that failed half way leaves what it allocated there."""
        import gc

        import torch

        self.model = None
        self.codec = None
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

    def __init__(self, voices: Path, base: Path, token: str, card: "Card", card_wait: float, room_every_s: float = 0.5) -> None:
        self.voices = voices
        self.token = token
        self.card = card
        self.card_wait = card_wait
        # How often a card just taken is asked whether its memory takes the model yet.
        self.room_every_s = room_every_s
        self.mouth = Mouth(base)
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
        text = request.get("text")

        if checkpoint is None or not isinstance(text, str) or not text.strip():
            raise ValueError("a request names a voice folder under the voices root, and a text")

        if len(text) > MOST_TEXT:
            raise ValueError(f"a text is at most {MOST_TEXT} characters")

        style = delivery(request.get("style"))
        by_sentence = request.get("sentences", False)

        if not isinstance(by_sentence, bool):
            raise ValueError("sentences is true or false")

        declared(checkpoint)
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
                audio, rate = self.mouth.speak(checkpoint, text.strip(), int(request.get("seed", 0)), style, by_sentence)
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
                print(f"voice-fish: watching the card failed: {type(failure).__name__}: {failure}", file=sys.stderr, flush=True)
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
                    print(f"voice-fish: putting the model back up failed, so the card was let go: {type(failure).__name__}: {failure}", file=sys.stderr, flush=True)
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
            print(f"voice-fish: putting the model down failed, so it was let go: {type(failure).__name__}: {failure}", file=sys.stderr, flush=True)
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
    service = Service(Path(args.voices), Path(args.base).expanduser(), token, card, args.gpu_wait)
    server = ThreadingHTTPServer((args.host, args.port), handler(service))
    threading.Thread(target=service.watch_card, daemon=True).start()
    print(f"voice-fish: serving on {args.host}:{args.port} · voices {args.voices} · card lock {card.path}", file=sys.stderr, flush=True)
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
            said["error"] = str(json.loads(refusal.read()).get("error", ""))[:MOST_ERROR] or None
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
    parser.add_argument("--timeout", type=float, default=600.0)
    parser.add_argument("--port", type=int, default=7432)
    parser.add_argument("--gpu-lock", default=GPU_LOCK)
    parser.add_argument("--gpu-wait", type=float, default=GPU_WAIT)
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
