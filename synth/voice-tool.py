#!/usr/bin/env python3
"""The voice as a tool (docs/voice-plan.md, 「The voice as a tool」): the mouth, the encoder and the
transcriber, each an entry for the system, answered by running the file that does the work.

It runs on the system's `python3` and the standard library alone, from the voice tool's placed folder, where the
build places it with the three files it runs and the answer of the tool host's call protocol it answers through,
`tool_protocol.py`. The encoder and the transcriber run on the voice environment's interpreter and the mouth on the
engine environment's (docs/voice-plan.md, 「The environments」).
"""

import json
import os
import subprocess
from pathlib import Path

import tool_protocol

HERE = Path(__file__).resolve().parent

# The voice environment where the process names none, under the call's home.
VOICE_ENVIRONMENT = "bmo-voice/.venv/bin/python"
# The engine environment the mouth speaks on (docs/voice-plan.md, 「The environments」), relative to the home.
ENGINE_ENVIRONMENT = "bmo-voice/fish-speech/.venv/bin/python"

# How much of a failed run's standard error an answer carries.
TAIL_LINES = 8


def interpreter(variable, environment=None):
    """The interpreter the variable names, where the process holds it, else the environment's, the voice environment's
    where none is named."""
    named = os.environ.get(variable)
    return named if named else str(Path.home() / (environment or VOICE_ENVIRONMENT))


def run(line, cwd=None):
    """One file run to its end, its output kept off this executable's own, which is the answer."""
    return subprocess.run(line, cwd=cwd, stdin=subprocess.DEVNULL, capture_output=True, text=True)


def failed(what, ran):
    tail = "\n".join(ran.stderr.strip().splitlines()[-TAIL_LINES:])
    return tool_protocol.failure(f"{what} exited {ran.returncode}" + (f": {tail}" if tail else ""))


def mouth(call):
    """One line spoken on this machine's card, standing in the card's line for as long as the lines ahead of it
    take (docs/voice-plan.md, 「One at a time on the operations machine's card」): the one lock file under the data
    root's voice folder, the one the voice's own runs stand in, with the synthesiser the very process the lock
    runs, so a call its caller stops ends it."""
    said = call.arguments
    if said.get("text_file") is None and said.get("text") is None:
        return tool_protocol.refusal("the mouth is handed the words, as text or as text_file")

    lock = Path(call.session["dataRoot"]) / "voice" / "locks" / "voice-synth.lock"
    lock.parent.mkdir(parents=True, exist_ok=True)
    lock.touch()

    line = ["flock", "--no-fork", str(lock)]
    line += [interpreter("BMO_VOX_PYTHON", ENGINE_ENVIRONMENT), str(HERE / "voice-fish.py")]
    line += ["--text-file", said["text_file"]] if said.get("text_file") is not None else ["--text", said["text"]]
    line += ["--out", said["out"]]
    if said.get("wav"):
        line += ["--wav", said["wav"]]
    line += ["--checkpoint", said["checkpoint"], "--seed", str(said["seed"])]
    line += ["--encoder-python", interpreter("BMO_VOICE_PYTHON")]
    if said.get("style"):
        line += ["--style", said["style"]]
    if said.get("sentences"):
        line.append("--sentences")

    ran = run(line)
    if ran.returncode != 0:
        return failed("the synthesiser", ran)
    return tool_protocol.text(f"spoken into {said['out']}")


def encoder(call):
    """A wav written out as OGG/Opus, in no line, since the card is not touched."""
    said = call.arguments
    line = [interpreter("BMO_VOICE_PYTHON"), str(HERE / "voice-encode.py"), "--from-wav", said["from_wav"], "--out", said["out"]]
    if said.get("wav"):
        line += ["--wav", said["wav"]]

    ran = run(line)
    if ran.returncode != 0:
        return failed("the encoder", ran)
    return tool_protocol.text(f"encoded into {said['out']}")


def transcriber(call):
    """Spoken files taken down in one language, each with what was heard, in the order given."""
    said = call.arguments
    ran = run([interpreter("BMO_ASR_PYTHON"), str(HERE / "voice-transcribe.py"), said["language"], *said["files"]])
    if ran.returncode != 0:
        return failed("the transcriber", ran)
    heard = [json.loads(line) for line in ran.stdout.splitlines() if line.strip()]
    return tool_protocol.structured({"heard": [{"file": one["file"], "heard": one["heard"]} for one in heard]})


if __name__ == "__main__":
    tool_protocol.answer({
        "voice/mouth": mouth,
        "voice/encoder": encoder,
        "voice/transcriber": transcriber,
    })
