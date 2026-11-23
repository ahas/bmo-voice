"""What the studio's Python tools read of the deployment (docs/script-studio.md, 「Access and service」): the voice's
one settings file, the one BMO_VOICE_SETTINGS names or settings.toml at the top of the release they stand in, for the
data root and the studio's own values, and the Korean language material under the release's data/ko/."""
import json
import os
import pathlib
import tomllib

RELEASE = pathlib.Path(__file__).resolve().parents[2]


def settings():
    named = os.environ.get('BMO_VOICE_SETTINGS')
    path = pathlib.Path(named) if named else RELEASE / 'settings.toml'
    if not path.exists():
        raise SystemExit(f'there are no settings at {path}; name them in BMO_VOICE_SETTINGS')
    return tomllib.loads(path.read_text())


def studio():
    """The studio's deployment values, empty where the settings name none."""
    return settings().get('studio', {})


def studio_root():
    """The studio's data root: the folder BMO_VOICE_STUDIO_DATA names, else the voice folder's studio/."""
    named = os.environ.get('BMO_VOICE_STUDIO_DATA')
    return named if named else str(pathlib.Path(settings()['data_root']) / 'voice' / 'studio')


def prefix(value):
    """A folder of the manifest's paths, with its slash, or nothing where it is the top."""
    value = (value or '').strip('/')
    return value + '/' if value else ''


def notes():
    """The Korean notes and sentences the tools write into records."""
    return json.loads((RELEASE / 'data' / 'ko' / 'studio-notes.json').read_text())
