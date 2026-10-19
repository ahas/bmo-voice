"""Downloads the source material the manifest names into the studio's data root, resuming through .part files, with
each video's subtitles (docs/script-studio.md, 「Media and downloads」).

usage: python3 studio/scripts/bmo-script-download.py [--root <data root>]
"""
import argparse
import concurrent.futures
import fcntl
import hashlib
import html.parser
import json
import os
import pathlib
import subprocess
import threading
import time
import urllib.parse

from bmo_script_settings import prefix, studio, studio_root

parser = argparse.ArgumentParser()
# The data root: the one --root names, else the studio's own (BMO_SCRIPT_DATA, else the settings' data root).
parser.add_argument('--root')
ROOT = pathlib.Path(parser.parse_args().root or studio_root())
STUDIO = studio()
if not STUDIO.get('cloud'):
    raise SystemExit('the settings name no [studio] cloud to download the material from')
CLOUD = STUDIO['cloud'].rstrip('/') + '/'
SOURCE = prefix(STUDIO.get('source_root'))
HOME = prefix(STUDIO.get('cloud_home'))
lock = threading.Lock()
ROOT.mkdir(parents=True, exist_ok=True)
_guard = open(ROOT / 'download.lock', 'w')
fcntl.flock(_guard, fcntl.LOCK_EX | fcntl.LOCK_NB)
items = json.loads((ROOT / 'source-manifest.json').read_text())
results = {}
if (ROOT / 'downloads.json').exists():
    results = json.loads((ROOT / 'downloads.json').read_text())


class PlayerPage(html.parser.HTMLParser):
    """A player page read as HTML is read: its first source's address and its tracks' addresses, in order."""

    def __init__(self):
        super().__init__()
        self.source = None
        self.tracks = []

    def handle_starttag(self, tag, attrs):
        src = dict(attrs).get('src')
        if not src:
            return
        if tag == 'source' and self.source is None:
            self.source = src
        elif tag == 'track':
            self.tracks.append(src)


def player(text):
    page = PlayerPage()
    page.feed(text)
    page.close()
    return page


def save():
    with lock:
        p = ROOT / 'downloads.json.tmp'
        p.write_text(json.dumps(results, ensure_ascii=False, indent=2))
        p.replace(ROOT / 'downloads.json')


def get(url, out, expected=None):
    out.parent.mkdir(parents=True, exist_ok=True)
    if out.exists() and (expected is None or out.stat().st_size == expected):
        return
    tmp = pathlib.Path(str(out) + '.part')
    error = ''
    for attempt in range(4):
        args = ['curl', '--fail', '--location', '--silent', '--show-error', '--connect-timeout', '30', '--max-time', '1800',
                '--retry', '2', '-H', f'Referer: {CLOUD}', '-A', 'Mozilla/5.0', '-o', str(tmp)]
        if tmp.exists():
            args += ['--continue-at', '-']
        r = subprocess.run(args + [url], capture_output=True, text=True)
        if r.returncode == 0 and (expected is None or tmp.stat().st_size == expected):
            tmp.replace(out)
            return
        if r.returncode == 33:
            tmp.unlink(missing_ok=True)
        error = r.stderr[-400:] or f'size mismatch: {tmp.stat().st_size if tmp.exists() else 0}/{expected}'
        time.sleep(2)
    raise RuntimeError(error)


def one(item):
    path = item['path']
    rel = path.removeprefix(SOURCE)
    target = ROOT / 'media' / rel
    previous = results.get(path, {})
    if previous.get('status') == 'complete' and target.exists() and target.stat().st_size == item['size'] and hashlib.file_digest(open(target, 'rb'), 'sha256').hexdigest() == previous.get('sha256'):
        return
    state = {'path': path, 'relative': rel, 'expectedBytes': item['size'], 'status': 'downloading'}
    results[path] = state
    save()
    try:
        if path.endswith('.mp4'):
            url = CLOUD + 'player/?video=' + urllib.parse.quote(path.removeprefix(HOME), safe='/')
            page = ROOT / 'pages' / (hashlib.sha256(path.encode()).hexdigest() + '.html')
            page.unlink(missing_ok=True)
            get(url, page)
            read = player(page.read_text())
            if not read.source:
                raise RuntimeError('player contains no source')
            source = read.source
            state['url'] = source
            tracks = []
            for n, t in enumerate(read.tracks):
                u = urllib.parse.urljoin(url, t)
                dest = ROOT / 'subtitles' / (hashlib.sha256(path.encode()).hexdigest() + f'-{n}.vtt')
                get(u, dest)
                tracks.append({'url': u, 'file': str(dest.relative_to(ROOT))})
            state['tracks'] = tracks
        else:
            source = CLOUD + urllib.parse.quote(path, safe='/')
        state['url'] = source
        save()
        if path.endswith('.htm'):
            raw = ROOT / 'responses' / rel
            get(source, raw)
            body = raw.read_bytes()
            original = body[:item['size']]
            if len(original) != item['size'] or not original.endswith(b'</script>'):
                raise RuntimeError('unexpected HTML transformation')
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(original)
            state['responseBytes'] = len(body)
            state['transformation'] = 'Cloudflare appended markup retained separately in responses/'
        else:
            for renewal in range(4):
                try:
                    get(source, target, item['size'])
                    break
                except RuntimeError as e:
                    if '403' not in str(e) or renewal == 3:
                        raise
                    # Player URLs carry a short-lived access identifier. Renew through the public player, exactly as
                    # reopening that page does; never guess a media host or access token.
                    page.unlink(missing_ok=True)
                    get(url, page)
                    renewed = player(page.read_text()).source
                    if not renewed:
                        raise RuntimeError('renewed player contains no source')
                    source = renewed
                    state['url'] = source
                    save()
                    time.sleep(2)
        digest = hashlib.file_digest(open(target, 'rb'), 'sha256').hexdigest()
        state.update(status='complete', bytes=target.stat().st_size, sha256=digest, completedAt=time.time())
    except Exception as e:
        state.update(status='failed', error=str(e))
    save()
    print(json.dumps({'file': rel, 'status': state['status'], 'complete': sum(r['status'] == 'complete' for r in results.values()), 'error': state.get('error')}, ensure_ascii=False), flush=True)


for sweep in range(3):
    pending = [item for item in items if results.get(item['path'], {}).get('status') != 'complete'] if sweep else items
    if not pending:
        break
    with concurrent.futures.ThreadPoolExecutor(max_workers=int(os.environ.get('BMO_DOWNLOAD_WORKERS', '8'))) as pool:
        list(pool.map(one, pending))
    if any(r.get('status') != 'complete' for r in results.values()):
        time.sleep(15)
fail = [r for r in results.values() if r['status'] != 'complete']
print('FINISHED', len(items), 'FAILED', len(fail), flush=True)
raise SystemExit(bool(fail))
