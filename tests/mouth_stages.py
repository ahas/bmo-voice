"""The voice server's account of a line (docs/voice-plan.md, 「How a line crosses」, 「What it answers」), with the model
and the card's driver stood in for: the room the model needs counts what the server holds on the card itself; each
stage a line reaches is written into its stage file, the pieces counted; a line whose asker has gone is dropped
before it takes the card, and while it waits for room; and a stage file outside the call folder is refused.

Run on the voice environment's interpreter, which carries numpy and soundfile: `tests/mouth_stages.rs` runs it.
"""

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("voice_fish", HERE.parent / "synth" / "voice-fish.py")
fish = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fish)

MIB = 1024 * 1024


class StoodInMouth:
    """The mouth, with a model that stands or does not, speaking a line in two pieces."""

    def __init__(self) -> None:
        self.model = None
        self.checkpoint = None
        self.loads = 0

    def ready(self, checkpoint: Path) -> float:
        if self.model is None or self.checkpoint != checkpoint:
            self.loads += 1
            self.model = object()
            self.checkpoint = checkpoint
        return 0.0

    def speak(self, checkpoint, text, seed, style, by_sentence, told=None):
        for at in (1, 2):
            told(at, 2)
        return np.zeros(10, dtype=np.float32), fish.RATE

    def release(self) -> None:
        self.model = None
        self.checkpoint = None


class Kept(fish.LineStage):
    """A line's stage file, each stage also kept in order."""

    def __init__(self, path: Path) -> None:
        super().__init__(path)
        self.told: list[tuple[str, str]] = []

    def tell(self, stage: str, said: str) -> None:
        self.told.append((stage, said))
        super().tell(stage, said)


class Stages(unittest.TestCase):
    def setUp(self) -> None:
        self.folder = tempfile.TemporaryDirectory()
        place = Path(self.folder.name)
        self.voices = place / "voices"
        self.spoken = place / "spoken"
        voice = self.voices / "a-voice"
        voice.mkdir(parents=True)
        self.spoken.mkdir()
        (voice / "voice.toml").write_text(f'engine = "{fish.ENGINE}"\n', encoding="utf-8")
        (voice / "lora.pt").write_bytes(b"")

        self.mouth = StoodInMouth()
        self.service = fish.Service(self.voices, place, "a-token", fish.Card(place / "gpu.lock"), room_every_s=0.0)
        self.service.mouth = self.mouth
        self.was = (fish.card_free_bytes, fish.own_card_bytes)

    def tearDown(self) -> None:
        fish.card_free_bytes, fish.own_card_bytes = self.was

        if self.service.card.handle is not None:
            fish.os.close(self.service.card.handle)

        self.folder.cleanup()

    def card(self, free: int, own: int) -> None:
        fish.card_free_bytes = lambda: free
        fish.own_card_bytes = lambda: own

    def stage(self) -> Kept:
        return Kept(self.service.stage_file(str(self.spoken / "a-call.stage")))

    def test_the_room_the_model_needs_counts_what_the_server_holds_itself(self) -> None:
        # The driver counts 4 MiB short of the load, while the server's own context holds 606 MiB.
        self.card(fish.LOAD_BYTES - 4 * MIB, 606 * MIB)
        stage = self.stage()

        self.service.room(stage, lambda: False)

        self.assertEqual(stage.told, [])
        self.assertTrue(self.service.fits(fish.card_room_bytes()))

    def test_a_line_writes_each_stage_it_reaches_and_counts_its_pieces(self) -> None:
        self.card(fish.LOAD_BYTES, 0)
        stage = self.stage()

        self.service.speak({"text": "A line.", "checkpoint": "a-voice"}, stage)

        self.assertEqual(
            [kind for kind, _ in stage.told],
            ["turn", "load", "speak", "speak"],
        )
        written = json.loads((self.spoken / "a-call.stage").read_text(encoding="utf-8"))
        self.assertEqual(written, {"stage": "speak", "step": 4, "said": "speaks piece 2 of 2"})

    def test_a_line_whose_asker_has_gone_is_dropped_before_it_takes_the_card(self) -> None:
        self.card(fish.LOAD_BYTES, 0)

        with self.assertRaises(fish.Abandoned):
            self.service.speak({"text": "A line.", "checkpoint": "a-voice"}, self.stage(), lambda: True)

        self.assertFalse(self.service.card.holding)
        self.assertEqual(self.mouth.loads, 0)
        self.assertFalse(self.service.lock.locked())

    def test_a_line_whose_asker_goes_while_it_waits_for_room_lets_the_card_go(self) -> None:
        self.card(fish.LOAD_BYTES - 4 * MIB, 0)
        stage = self.stage()
        looks = iter([False, True])

        with self.assertRaises(fish.Abandoned):
            self.service.speak({"text": "A line.", "checkpoint": "a-voice"}, stage, lambda: next(looks))

        self.assertEqual([kind for kind, _ in stage.told], ["turn", "room"])
        self.assertIn("free of the 11.00 GiB it needs", stage.told[-1][1])
        self.assertFalse(self.service.card.holding)
        self.assertEqual(self.mouth.loads, 0)

    def test_a_stage_file_outside_the_call_folder_is_refused(self) -> None:
        with self.assertRaises(ValueError):
            self.service.stage_file(str(self.voices / "a-call.stage"))

        with self.assertRaises(ValueError):
            self.service.stage_file(str(self.spoken / "a-call.wav"))

        self.assertIsNone(self.service.stage_file(None))


if __name__ == "__main__":
    unittest.main()
