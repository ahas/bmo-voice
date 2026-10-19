"""The voice's answer of the tool host's call protocol 1 (bmo-pocket's docs/api.md, 「The call protocol」): the
handed object read off standard input, the entry it names answered on standard output as an MCP tool result.

The protocol is the tool host's documented contract; this is the voice's own reading of it, so the voice depends
on no other repository's code. It holds only what the voice's entries use: a call's entry, arguments and session,
and the four kinds of answer. Standard library alone, so it runs on whichever `python3` the clean environment's
PATH finds.
"""

import json
import sys

PROTOCOL = 1


def text(said):
    """An answer of plain text."""
    return {"content": [{"type": "text", "text": said}], "isError": False}


def refusal(said):
    """A refusal, an answer marked as an error that the reader takes as any other."""
    return {"content": [{"type": "text", "text": said}], "isError": True}


def failure(reason):
    """An entry that could not read what it answers out of: the reason, after `ERROR: `."""
    return refusal(f"ERROR: {reason}")


def structured(obj):
    """An object as an entry with an output schema answers it: the object, and the same object as its text."""
    said = json.dumps(obj, ensure_ascii=False, separators=(",", ":"))
    return {"content": [{"type": "text", "text": said}], "isError": False, "structuredContent": obj}


class Call:
    """One call as it was handed: the entry, its arguments and the session record."""

    def __init__(self, handed):
        self.entry = handed["entry"]
        self.arguments = handed.get("arguments") or {}
        self.session = handed["session"]


def answer_handed(handlers, handed):
    """What the handed object asks, answered by the handlers: a call by its entry's handler, a claim with none,
    and a resource request as an executable serving none answers it."""
    kind = handed.get("kind")
    if kind == "claim":
        return {"arguments": None}
    if kind == "resources_list":
        return {"resources": []}
    if kind == "resources_read":
        return {"error": "this executable serves no resources"}
    if kind != "call":
        return failure("this executable answers calls alone")
    handler = handlers.get(handed.get("entry"))
    if handler is None:
        return failure(f"this executable holds no entry {handed.get('entry')}")
    try:
        return handler(Call(handed))
    except Exception as why:
        return failure(why)


def answer(handlers):
    """The whole of the entry's main: the call read off standard input and answered on standard output; a call that
    does not read is said on standard error and ends unclean."""
    try:
        handed = json.loads(sys.stdin.buffer.read().decode())
        if not isinstance(handed, dict) or not isinstance(handed.get("session"), dict):
            raise ValueError("it is no object holding a session")
        if handed.get("protocol") != PROTOCOL:
            raise ValueError(f"protocol {handed.get('protocol')} is not {PROTOCOL}")
    except ValueError as why:
        print(f"the call does not read: {why}", file=sys.stderr)
        sys.exit(1)
    sys.stdout.buffer.write(json.dumps(answer_handed(handlers, handed), ensure_ascii=False).encode())
    sys.stdout.buffer.flush()
