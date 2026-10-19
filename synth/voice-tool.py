#!/usr/bin/env python3
"""The voice as a tool (docs/voice-plan.md, 「The voice as a tool」): the mouth, the encoder, the
transcriber and the briefing, each an entry for the system, answered by running the file that does the work.

It runs on the system's `python3` and the standard library alone, from the voice tool's placed folder, where the
build places it with the three files it runs and the answer of the tool host's call protocol it answers through,
`tool_protocol.py`. Each of those files runs
on the voice environment's interpreter (docs/voice-plan.md, 「One voice environment」).
"""

import json
import os
import shutil
import subprocess
from pathlib import Path

import tool_protocol

HERE = Path(__file__).resolve().parent

# The voice environment where the process names none, under the call's home.
VOICE_ENVIRONMENT = "bmo-voice/.venv/bin/python"

# How long a line stands in front of the card before it gives up its turn, and what the lock exits with when it
# does, kept clear of anything the synthesiser exits with (docs/voice-plan.md, 「One at a time on the
# operations machine's card」).
QUEUE_WAIT_S = 300
QUEUE_EXIT = 75

# How much of a failed run's standard error an answer carries.
TAIL_LINES = 8


def interpreter(variable):
    """The interpreter the variable names, where the process holds it, else the voice environment's."""
    named = os.environ.get(variable)
    return named if named else str(Path.home() / VOICE_ENVIRONMENT)


def run(line, cwd=None):
    """One file run to its end, its output kept off this executable's own, which is the answer."""
    return subprocess.run(line, cwd=cwd, stdin=subprocess.DEVNULL, capture_output=True, text=True)


def failed(what, ran):
    tail = "\n".join(ran.stderr.strip().splitlines()[-TAIL_LINES:])
    return tool_protocol.failure(f"{what} exited {ran.returncode}" + (f": {tail}" if tail else ""))


def mouth(call):
    """One line spoken on this machine's card, standing in the card's line: the one lock file under the data
    root's voice folder, the one the voice's own runs stand in, with the synthesiser the very process the lock
    runs, so a call ended at its timeout ends it."""
    said = call.arguments
    if said.get("text_file") is None and said.get("text") is None:
        return tool_protocol.refusal("the mouth is handed the words, as text or as text_file")

    lock = Path(call.session["dataRoot"]) / "voice" / "locks" / "voice-synth.lock"
    lock.parent.mkdir(parents=True, exist_ok=True)
    lock.touch()

    line = ["flock", "--timeout", str(QUEUE_WAIT_S), "--conflict-exit-code", str(QUEUE_EXIT), "--no-fork", str(lock)]
    line += [interpreter("BMO_VOX_PYTHON"), str(HERE / "voice-vox.py")]
    line += ["--text-file", said["text_file"]] if said.get("text_file") is not None else ["--text", said["text"]]
    line += ["--out", said["out"]]
    if said.get("wav"):
        line += ["--wav", said["wav"]]
    line += ["--checkpoint", said["checkpoint"], "--reference", said["reference"]]
    line += ["--cfg", str(said["cfg"]), "--steps", str(said["steps"]), "--seed", str(said["seed"])]
    line += ["--encoder-python", interpreter("BMO_VOICE_PYTHON")]
    if said.get("style"):
        line += ["--style", said["style"]]
    if said.get("sentences"):
        line.append("--sentences")
    for prompt in said.get("prompts") or []:
        line += ["--prompt", prompt["language"], prompt["clip"], prompt["words"]]

    ran = run(line)
    if ran.returncode == QUEUE_EXIT:
        return tool_protocol.refusal("no turn: another line held the card past the wait, and this one was never spoken")
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


def unit_command():
    """The voice's unit command: the one `BMO_VOICE_BINARY` names where the process holds it, else `bmo-voice` on
    the path, else the one in a release above the placed folder."""
    named = os.environ.get("BMO_VOICE_BINARY") or shutil.which("bmo-voice")
    if named:
        return Path(named)
    here = Path.cwd().resolve()
    return next((tree / "bin" / "bmo-voice" for tree in [here, *here.parents] if (tree / "bin" / "bmo-voice").is_file()), None)


def briefing(call):
    """A briefing launched as `bmo-voice voice-brief` launches one, answered once its run is up; its refusals are
    the command's."""
    command = unit_command()
    if command is None:
        return tool_protocol.failure("the voice's unit command, bmo-voice, is not found")

    said = call.arguments
    line = [str(command), "voice-brief", "--chat", str(said["chat"]), "--by", said["by"], "--text", said["text"]]
    for field in ("emotion", "title", "document"):
        if said.get(field) is not None:
            line += [f"--{field}", str(said[field])]

    ran = run(line, cwd=command.parent)
    if ran.returncode != 0:
        return tool_protocol.refusal((ran.stderr.strip() or ran.stdout.strip()) or f"bmo-voice voice-brief exited {ran.returncode}")
    return tool_protocol.text(ran.stdout.strip())


if __name__ == "__main__":
    tool_protocol.answer({
        "voice/mouth": mouth,
        "voice/encoder": encoder,
        "voice/transcriber": transcriber,
        "voice/briefing": briefing,
    })
