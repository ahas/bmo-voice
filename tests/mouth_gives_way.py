"""The voice server's model as it gives the card way (docs/voice-plan.md, 「How it holds the card」), with the model, torch
and the card arbiter's turn client stood in for: a rest keeps the model, giving way lets it go whole and asks for no
ticket, a line being spoken is never given way under, and letting it go empties every tensor on the card in place.

Run on the voice environment's interpreter, which carries numpy and soundfile: `tests/mouth_gives_way.rs` runs it.
"""

import importlib.util
import sys
import tempfile
import threading
import types
import unittest
from pathlib import Path
from types import SimpleNamespace

import numpy as np

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("voice_fish", HERE.parent / "synth" / "voice-fish.py")
fish = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fish)


class StoodInTurn:
    """The turn client: a ticket that rests where it is asked to, answers its watch once told to, and counts what it
    was asked."""

    def __init__(self) -> None:
        self.ticket = None
        self.asked_again = 0
        self.given_back = 0
        self.answer = threading.Event()
        self.watching = threading.Event()
        self.gives_way = False

    def ask(self, rest: bool = False):
        self.asked_again += 1
        self.ticket = "a-ticket"
        return self.ticket

    def keep(self, next_waited: float) -> bool:
        return True

    def rest(self):
        return SimpleNamespace(turn=True, resting=True, give_way_for="another" if self.gives_way else None)

    def watch(self, ticket):
        self.watching.set()
        self.answer.wait()
        return SimpleNamespace(turn=True, resting=True, give_way_for="another")

    def give_back(self) -> None:
        self.given_back += 1
        self.ticket = None


class StoodInMouth:
    """The mouth, with a model that stands or does not and a count of how often it was loaded and let go."""

    def __init__(self) -> None:
        self.model = None
        self.checkpoint = None
        self.loads = 0
        self.releases = 0
        self.speaking = lambda: None

    def ready(self, checkpoint: Path) -> float:
        if self.model is None or self.checkpoint != checkpoint:
            self.loads += 1
            self.model = object()
            self.checkpoint = checkpoint
        return 0.0

    def speak(self, checkpoint, text, seed, style, by_sentence, told=None):
        self.speaking()
        return np.zeros(10, dtype=np.float32), fish.RATE

    def release(self) -> None:
        self.releases += 1
        self.model = None
        self.checkpoint = None


class GivingWay(unittest.TestCase):
    def setUp(self) -> None:
        self.folder = tempfile.TemporaryDirectory()
        room = Path(self.folder.name)
        voice = room / "a-voice"
        voice.mkdir()
        (voice / "voice.toml").write_text(f'engine = "{fish.ENGINE}"\n', encoding="utf-8")
        (voice / "lora.pt").write_bytes(b"")

        self.lock = room / "gpu.lock"
        self.turn = StoodInTurn()
        self.mouth = StoodInMouth()
        self.service = fish.Service(room, room, "a-token", fish.Card(self.lock, self.turn))
        self.service.mouth = self.mouth
        self.service.fits = lambda free: True

        def hold(turn, path, waited_s=0.0):
            turn.ticket = "a-ticket"
            return fish.os.open(path, fish.os.O_RDWR | fish.os.O_CREAT, 0o644)

        self.stood_in_turn = sys.modules.get("turn")
        sys.modules["turn"] = SimpleNamespace(hold=hold)

    def tearDown(self) -> None:
        self.turn.answer.set()

        if self.service.card.handle is not None:
            fish.os.close(self.service.card.handle)

        if self.stood_in_turn is None:
            del sys.modules["turn"]
        else:
            sys.modules["turn"] = self.stood_in_turn

        self.folder.cleanup()

    def line(self) -> None:
        self.service.speak({"text": "A line.", "checkpoint": "a-voice"})

    def test_a_rest_keeps_the_model_on_the_card(self) -> None:
        self.line()

        self.assertTrue(self.service.card.resting)
        self.assertTrue(self.service.card.holding)
        self.assertIsNotNone(self.mouth.model)
        self.assertEqual(self.mouth.releases, 0)

    def test_giving_way_lets_the_model_go_whole_and_asks_for_no_ticket(self) -> None:
        self.line()
        self.turn.answer.set()

        self.assertTrue(self.service.watch_step())
        self.assertEqual(self.mouth.releases, 1)
        self.assertIsNone(self.mouth.model)
        self.assertFalse(self.service.card.holding)
        self.assertFalse(self.service.card.resting)
        self.assertEqual(self.turn.given_back, 1)
        self.assertEqual(self.turn.asked_again, 0)
        # Nothing is left to watch until the next line comes.
        self.assertFalse(self.service.watch_step())

    def test_somebody_already_waiting_as_a_line_ends_takes_the_model_with_the_card(self) -> None:
        self.turn.gives_way = True
        self.line()

        self.assertEqual(self.mouth.releases, 1)
        self.assertFalse(self.service.card.holding)
        self.assertEqual(self.turn.asked_again, 0)

    def test_the_next_line_after_giving_way_loads_the_model_again(self) -> None:
        self.line()
        self.turn.answer.set()
        self.service.watch_step()
        self.line()

        self.assertEqual(self.mouth.loads, 2)
        self.assertIsNotNone(self.mouth.model)

    def test_a_line_being_spoken_is_never_given_way_under(self) -> None:
        self.line()
        watcher = threading.Thread(target=self.service.watch_step)
        watcher.start()
        self.assertTrue(self.turn.watching.wait(10))
        during = []

        def speaking() -> None:
            # The arbiter's answer lands in the middle of the line: the watcher waits for the line's own lock.
            self.turn.answer.set()
            watcher.join(0.3)
            during.append((watcher.is_alive(), self.mouth.releases, self.mouth.model is not None))

        self.mouth.speaking = speaking
        self.line()
        watcher.join(10)

        self.assertEqual(during, [(True, 0, True)])
        self.assertFalse(watcher.is_alive())
        # The line ended and the card rested again before the answer was acted on, and only then did it give way.
        self.assertEqual(self.mouth.releases, 1)

    def test_a_line_that_fails_lets_the_model_go_and_the_lock_with_it(self) -> None:
        def speaking() -> None:
            raise RuntimeError("the line failed")

        self.mouth.speaking = speaking

        with self.assertRaises(RuntimeError):
            self.line()

        self.assertEqual(self.mouth.releases, 1)
        self.assertFalse(self.service.card.holding)


class StoodInTensor:
    def __init__(self, on_card: bool) -> None:
        self.is_cuda = on_card
        self.dtype = "bf16"
        self.data = "the weights"


class StoodInModule:
    def __init__(self, *tensors) -> None:
        self._parameters = {str(at): tensor for at, tensor in enumerate(tensors)}
        self._buffers = {}

    def modules(self):
        return [self]


class LettingGo(unittest.TestCase):
    def setUp(self) -> None:
        self.calls: list[str] = []
        calls = self.calls
        stood_in = types.ModuleType("torch")
        stood_in.__path__ = []
        stood_in.Tensor = StoodInTensor
        stood_in.empty = lambda size, dtype: f"empty {dtype}"
        stood_in.cuda = SimpleNamespace(is_available=lambda: True, empty_cache=lambda: calls.append("emptied"))
        dynamo = types.ModuleType("torch._dynamo")
        dynamo.reset = lambda: calls.append("reset")
        stood_in._dynamo = dynamo
        self.was = {name: sys.modules.get(name) for name in ("torch", "torch._dynamo")}
        sys.modules["torch"] = stood_in
        sys.modules["torch._dynamo"] = dynamo

    def tearDown(self) -> None:
        for name, module in self.was.items():
            if module is None:
                del sys.modules[name]
            else:
                sys.modules[name] = module

    def test_every_tensor_on_the_card_is_emptied_in_place_and_nothing_stands_after(self) -> None:
        mouth = fish.Mouth(Path("."))
        on_card, on_host, codec_tensor = StoodInTensor(True), StoodInTensor(False), StoodInTensor(True)
        attention = SimpleNamespace(kv_cache="a cache")
        model = StoodInModule(on_card, on_host)
        model.layers = [SimpleNamespace(attention=attention)]
        model.fast_layers = []
        mouth.model = model
        mouth.codec = StoodInModule(codec_tensor)
        mouth.checkpoint = Path("a-voice")
        mouth.step = SimpleNamespace(forget=lambda: self.calls.append("forgot"))

        # A reference outside the mouth, as a failed line's traceback holds one, frees nothing by itself.
        still_held = on_card

        mouth.release()

        self.assertEqual(still_held.data, "empty bf16")
        self.assertEqual(codec_tensor.data, "empty bf16")
        self.assertEqual(on_host.data, "the weights")
        self.assertIsNone(attention.kv_cache)
        self.assertIsNone(mouth.model)
        self.assertIsNone(mouth.codec)
        self.assertIsNone(mouth.checkpoint)
        self.assertIsNone(mouth.step)
        self.assertEqual(self.calls, ["forgot", "reset", "emptied"])

    def test_a_mouth_with_no_model_lets_go_of_what_a_failed_load_left(self) -> None:
        fish.Mouth(Path(".")).release()

        self.assertEqual(self.calls, ["emptied"])


if __name__ == "__main__":
    unittest.main()
