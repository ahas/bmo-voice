"""Where the transcriber hears (docs/voice-plan.md, 「Where a transcription runs」), with torch, torchaudio and Whisper's
pipeline stood in for: a free card is heard on under its lock, a card another occupant holds sends the work to the
processor, and a card that runs out of memory, loading or hearing, hands the work to the processor with the lock
given back and every file answered in order.

Run on the Python tests' own interpreter, which carries numpy and soundfile (README.md, 「Tests」):
`tests/transcriber_card.rs` runs it.
"""

import contextlib
import fcntl
import importlib.util
import io
import json
import os
import sys
import tempfile
import types
import unittest
from pathlib import Path

import numpy as np
import soundfile as sf

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("voice_transcribe", HERE.parent / "synth" / "voice-transcribe.py")
transcribe = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(transcribe)


class OutOfMemoryError(RuntimeError):
    """torch's own, as the card raises it."""


def free(lock: Path) -> bool:
    """Whether the card lock can be taken by another occupant now."""
    handle = os.open(lock, os.O_RDWR | os.O_CREAT, 0o644)
    try:
        fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        return True
    except BlockingIOError:
        return False
    finally:
        os.close(handle)


class StoodInWhisper:
    """Whisper's pipeline: what it was loaded on, what each file was heard on with the lock free or not, and where
    the card runs out of memory."""

    def __init__(self, lock: Path) -> None:
        self.lock = lock
        self.loads = []
        self.heard = []
        self.full_on_load = False
        self.full_on_hearing = False

    def pipeline(self, task, model, device, dtype):
        if device == "cuda" and self.full_on_load:
            raise OutOfMemoryError("CUDA out of memory")
        self.loads.append((device, dtype))

        def hear(said, return_timestamps, generate_kwargs):
            if device == "cuda" and self.full_on_hearing:
                raise OutOfMemoryError("CUDA out of memory")
            self.heard.append((device, free(self.lock)))
            return {"text": f" heard on {device} "}

        return hear


class TranscriberCard(unittest.TestCase):
    def setUp(self) -> None:
        self.folder = tempfile.TemporaryDirectory()
        room = Path(self.folder.name)
        self.lock = room / ".gpu.lock"
        self.files = []
        for name in ("one.wav", "two.wav"):
            sf.write(room / name, np.zeros(1600, dtype="float32"), 16000)
            self.files.append(str(room / name))

        self.whisper = StoodInWhisper(self.lock)
        self.emptied = 0

        def empty_cache() -> None:
            self.emptied += 1

        torch = types.ModuleType("torch")
        torch.float16, torch.float32 = "float16", "float32"
        torch.cuda = types.SimpleNamespace(is_available=lambda: True, OutOfMemoryError=OutOfMemoryError,
                                           empty_cache=empty_cache)
        transformers = types.ModuleType("transformers")
        transformers.pipeline = self.whisper.pipeline
        stood_in = {"torch": torch, "torchaudio": types.ModuleType("torchaudio"), "transformers": transformers}
        self.kept = {name: sys.modules.get(name) for name in stood_in}
        sys.modules.update(stood_in)
        self.home = os.environ.get("HOME")
        os.environ["HOME"] = str(room)
        self.argv = sys.argv

    def tearDown(self) -> None:
        for name, module in self.kept.items():
            if module is None:
                sys.modules.pop(name, None)
            else:
                sys.modules[name] = module
        if self.home is None:
            os.environ.pop("HOME", None)
        else:
            os.environ["HOME"] = self.home
        sys.argv = self.argv
        self.folder.cleanup()

    def transcribe(self) -> list:
        said = io.StringIO()
        sys.argv = ["voice-transcribe.py", "ko", *self.files]
        with contextlib.redirect_stdout(said):
            self.assertEqual(transcribe.main(), 0)
        return [json.loads(line) for line in said.getvalue().splitlines()]

    def test_a_free_card_is_heard_on_under_its_lock_and_given_back(self) -> None:
        answered = self.transcribe()

        self.assertEqual(self.whisper.loads, [("cuda", "float16")])
        self.assertEqual(self.whisper.heard, [("cuda", False), ("cuda", False)])
        self.assertEqual([one["heard"] for one in answered], ["heard on cuda", "heard on cuda"])
        self.assertTrue(free(self.lock))

    def test_a_card_another_occupant_holds_sends_the_work_to_the_processor(self) -> None:
        held = os.open(self.lock, os.O_RDWR | os.O_CREAT, 0o644)
        fcntl.flock(held, fcntl.LOCK_EX)
        try:
            answered = self.transcribe()
        finally:
            os.close(held)

        self.assertEqual(self.whisper.loads, [("cpu", "float32")])
        self.assertEqual([one["heard"] for one in answered], ["heard on cpu", "heard on cpu"])
        self.assertEqual(self.emptied, 0)

    def test_a_card_full_as_the_model_loads_hands_the_work_to_the_processor(self) -> None:
        self.whisper.full_on_load = True

        answered = self.transcribe()

        self.assertEqual(self.whisper.loads, [("cpu", "float32")])
        self.assertEqual(self.whisper.heard, [("cpu", True), ("cpu", True)])
        self.assertEqual([one["heard"] for one in answered], ["heard on cpu", "heard on cpu"])
        self.assertEqual(self.emptied, 1)

    def test_a_card_full_while_hearing_hears_that_file_and_the_rest_on_the_processor(self) -> None:
        self.whisper.full_on_hearing = True

        answered = self.transcribe()

        self.assertEqual(self.whisper.loads, [("cuda", "float16"), ("cpu", "float32")])
        self.assertEqual(self.whisper.heard, [("cpu", True), ("cpu", True)])
        self.assertEqual([one["file"] for one in answered], self.files)
        self.assertEqual([one["heard"] for one in answered], ["heard on cpu", "heard on cpu"])
        self.assertEqual(self.emptied, 1)


if __name__ == "__main__":
    unittest.main()
