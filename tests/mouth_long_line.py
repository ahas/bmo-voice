"""A long line through the voice server's own `POST /speak`, on the mouth's own pieces, levelling and joining, with
only the model stood in for (docs/voice-plan.md, 「What it answers」, 「What a long line takes of the card」): a line of
any length the engine takes in comes back as one sound, every word of it spoken once and in order.

Run on the voice environment's interpreter, which carries numpy and soundfile: `tests/mouth_long_line.rs` runs it.
"""

import importlib.util
import io
import json
import sys
import tempfile
import threading
import unittest
import urllib.request
from http.server import ThreadingHTTPServer
from pathlib import Path
from types import SimpleNamespace

import numpy as np
import soundfile as sf

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("voice_fish", HERE.parent / "synth" / "voice-fish.py")
fish = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fish)

# The most characters of spoken words a synthesis holds (docs/boundary-api.md, 「Syntheses」).
MOST_SPOKEN = 30_000


class StoodInMouth(fish.Mouth):
    """The mouth with the model stood in for: each piece is a tone as long as its words, and the words of every piece
    are kept, with whether the line's first piece was carried into it."""

    def __init__(self, base: Path) -> None:
        super().__init__(base)
        self.said: list[str] = []
        self.carried_into: list[bool] = []

    def ready(self, checkpoint: Path) -> float:
        self.codec = SimpleNamespace(sample_rate=fish.RATE)
        self.checkpoint = checkpoint
        self.model = object()
        return 0.0

    def release(self) -> None:
        pass

    def piece(self, words: str, seed: int, carried: dict) -> np.ndarray:
        self.said.append(words)
        self.carried_into.append(carried["first"] is not None)

        if carried["first"] is None:
            carried["first"] = (words, None)

        seconds = max(len(words) * 0.002, 0.05)
        at = np.arange(int(seconds * fish.RATE)) / fish.RATE

        return (0.3 * np.sin(2 * np.pi * 220 * at)).astype(np.float32)


class StoodInCard:
    """The node's card, always this server's to take and to rest on."""

    holding = False
    resting = False

    def take(self, waited: float = 0.0) -> None:
        self.holding = True

    def keeps(self, next_waited: float) -> bool:
        return True

    def rest(self) -> str:
        return "rests"

    def let_go(self) -> None:
        self.holding = False


def line_of(characters: int, sentence: str) -> str:
    """Sentences laid one after another until the line is exactly `characters` long, its last one cut to fit."""
    words = []
    length = 0

    while length < characters:
        words.append(sentence)
        length += len(sentence) + 1

    line = " ".join(words)[:characters]

    return line[:-1] + "." if line.endswith(" ") else line


class LongLine(unittest.TestCase):
    def setUp(self) -> None:
        self.folder = tempfile.TemporaryDirectory()
        voices = Path(self.folder.name)
        voice = voices / "a-voice"
        voice.mkdir()
        (voice / "voice.toml").write_text(f'engine = "{fish.ENGINE}"\n', encoding="utf-8")
        (voice / "lora.pt").write_bytes(b"")

        self.service = fish.Service(voices, voices, "a-token", StoodInCard())
        self.service.mouth = StoodInMouth(voices)
        self.service.fits = lambda free: True
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), fish.handler(self.service))
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def tearDown(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.folder.cleanup()

    def spoken(self, text: str, by_sentence: bool) -> tuple[int, bytes]:
        # Written as the engine writes a request: UTF-8 as it stands, no character escaped.
        body = json.dumps(
            {"text": text, "checkpoint": "a-voice", "seed": 0, "style": "", "sentences": by_sentence},
            ensure_ascii=False,
        ).encode("utf-8")
        asked = urllib.request.Request(
            f"http://127.0.0.1:{self.server.server_address[1]}/speak",
            data=body,
            headers={"Authorization": "Bearer a-token", "Content-Type": "application/json"},
        )

        with urllib.request.urlopen(asked) as answer:
            return answer.status, answer.read()

    def assert_one_sound_of_every_word(self, text: str, by_sentence: bool) -> None:
        status, wav = self.spoken(text, by_sentence)
        audio, rate = sf.read(io.BytesIO(wav))
        said = self.service.mouth.said

        self.assertEqual(status, 200)
        self.assertEqual(rate, fish.RATE)
        self.assertEqual(audio.ndim, 1, "one sound, one channel")
        self.assertGreater(len(said), 1, "a long line is spoken in several pieces")
        self.assertTrue(all(len(piece) <= fish.MOST_PIECE for piece in said))
        self.assertEqual(" ".join(said).split(), text.split(), "every word spoken once, in order")
        self.assertEqual(self.service.mouth.carried_into, [False] + [True] * (len(said) - 1))
        pauses = int(fish.SENTENCE_PAUSE * fish.RATE) * (len(said) - 1)
        tones = sum(int(max(len(piece) * 0.002, 0.05) * fish.RATE) for piece in said)
        self.assertEqual(len(audio), tones + pauses, "the pieces joined, a pause between each two")
        self.assertAlmostEqual(float(np.max(np.abs(audio))), fish.PEAK, places=3)

    def test_a_9000_character_line_comes_back_as_one_sound(self) -> None:
        text = line_of(9_000, "The index fell three point five percent to five thousand seven hundred and thirty.")

        self.assertEqual(len(text), 9_000)
        self.assert_one_sound_of_every_word(text, by_sentence=False)

    def test_the_longest_spoken_words_fit_one_request(self) -> None:
        text = line_of(MOST_SPOKEN, "코스피는 삼 점 오 퍼센트 밀려 오천칠백삼십 점 칠사에 마감했습니다.")

        self.assertEqual(len(text), MOST_SPOKEN)
        self.assert_one_sound_of_every_word(text, by_sentence=True)


if __name__ == "__main__":
    sys.exit(unittest.main(argv=[sys.argv[0], "-v"], exit=False).result.wasSuccessful() is False)
