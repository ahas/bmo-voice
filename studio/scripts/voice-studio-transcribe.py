"""Transcribes the studio's supplied videos into English with word timings, keeping the provided subtitles apart
(docs/script-studio.md, 「Automatic transcription」).

usage: python3 studio/scripts/voice-studio-transcribe.py --queue <paths.json> [--root <data root>] [--model turbo]
       [--device cuda|cpu] [--batch-size 8]
"""
import argparse
import hashlib
import json
import os
import pathlib
import sys
import sysconfig
import time

from voice_studio_audio import render_vtt
from voice_studio_settings import prefix, studio, studio_root

parser = argparse.ArgumentParser(description='Transcribe user-provided studio videos, keeping word timestamps and originals separate.')
parser.add_argument('--root')
parser.add_argument('--queue', required=True, help='JSON array of exact source-manifest paths')
parser.add_argument('--model', default='turbo')
parser.add_argument('--device', choices=['cuda', 'cpu'], default='cuda')
parser.add_argument('--batch-size', type=int, default=8)
args = parser.parse_args()
root = pathlib.Path(args.root or studio_root())
source = prefix(studio().get('source_root'))
if args.device == 'cuda' and not os.environ.get('BMO_ASR_CUDA_LIBS'):
    lib = pathlib.Path(sysconfig.get_paths()['purelib']) / 'nvidia'
    paths = [str(p) for p in lib.glob('*/lib')]
    if paths:
        env = dict(os.environ, BMO_ASR_CUDA_LIBS='1', LD_LIBRARY_PATH=':'.join(paths + [os.environ.get('LD_LIBRARY_PATH', '')]))
        os.execvpe(sys.executable, [sys.executable, *sys.argv], env)
from faster_whisper import BatchedInferencePipeline, WhisperModel  # noqa: E402

out = root / 'asr'
out.mkdir(exist_ok=True)
model = WhisperModel(args.model, device=args.device, compute_type='int8_float16' if args.device == 'cuda' else 'int8', cpu_threads=4, num_workers=1)
pipe = BatchedInferencePipeline(model=model)
manifest = json.loads((root / 'source-manifest.json').read_text())
queue = json.loads(pathlib.Path(args.queue).read_text())
for path in queue:
    row = next(r for r in manifest if r['path'] == path)
    if not path.startswith(source) or '..' in pathlib.PurePosixPath(path).parts:
        raise ValueError('Invalid source path')
    aid = hashlib.sha256(path.encode()).hexdigest()
    target = out / (aid + '.json')
    if target.exists():
        (out / (aid + '.vtt')).write_text(render_vtt(json.loads(target.read_text())['segments']))
        continue
    started = time.time()
    print('START', path, flush=True)
    segments, info = pipe.transcribe(str(root / 'media' / path.removeprefix(source)), language='en', batch_size=args.batch_size, beam_size=5, word_timestamps=True, vad_filter=True)
    result = []
    for seg in segments:
        result.append({'start': seg.start, 'end': seg.end, 'text': seg.text, 'avg_logprob': seg.avg_logprob, 'words': [{'start': w.start, 'end': w.end, 'word': w.word, 'probability': w.probability} for w in seg.words or []]})
    payload = {'assetId': aid, 'path': path, 'model': 'faster-whisper ' + args.model, 'language': 'en', 'duration': info.duration, 'elapsed': round(time.time() - started, 2), 'segments': result, 'humanVerified': False}
    temp = target.with_suffix('.part')
    temp.write_text(json.dumps(payload, ensure_ascii=False))
    temp.replace(target)
    (out / (aid + '.vtt')).write_text(render_vtt(result))
    print('DONE', path, len(result), payload['elapsed'], flush=True)
