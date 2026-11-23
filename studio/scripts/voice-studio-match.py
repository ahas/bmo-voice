"""Matches the speaker's reference phrases from web scripts to the text transcribed from the supplied videos, and
writes a batch the import script registers (docs/script-studio.md, 「Importing speakers from web scripts」). The lines
it writes always carry the transcript's words.

usage: python3 studio/scripts/voice-studio-match.py --references <file> --links <file> [--root <data root>]
"""
import argparse
import difflib
import hashlib
import json
import pathlib
import re

from voice_studio_settings import notes, studio, studio_root

p = argparse.ArgumentParser(description='Match speaker reference phrases to text transcribed from supplied videos. Outputs always use ASR words.')
p.add_argument('--root')
p.add_argument('--references', required=True)
p.add_argument('--links', required=True)
args = p.parse_args()
root = pathlib.Path(args.root or studio_root())
# The speaker the references name, as the settings' first speaker spelling, which an automatic line's id and notes name.
speaker = (studio().get('speakers') or [''])[0]
line_prefix = f'asr-{speaker.lower()}-' if speaker else 'asr-'
note = notes()['asrCandidate']
anchors = json.load(open(args.references))
links = json.load(open(args.links))
episodes = {}
audit = []


def norm(s):
    s = s.lower().replace('’', "'")
    s = re.sub(r"\bcan't\b", 'cannot', s)
    s = re.sub(r"\bwon't\b", 'will not', s)
    s = re.sub(r"\b(\w+)n't\b", r'\1 not', s)
    s = re.sub(r"\bi'm\b", 'i am', s)
    s = re.sub(r"\b(you|we|they)'re\b", r'\1 are', s)
    s = re.sub(r"\b(i|you|we|they)'ve\b", r'\1 have', s)
    return re.findall(r'[a-z0-9]+', s.replace("'", ''))


for link in links:
    aid = hashlib.sha256(link['path'].encode()).hexdigest()
    path = root / 'asr' / (aid + '.json')
    if not path.exists():
        continue
    data = json.load(open(path))
    words = [w for seg in data['segments'] for w in seg['words']]
    tokens = []
    positions = []
    for i, w in enumerate(words):
        for token in norm(w['word']):
            tokens.append(token)
            positions.append(i)
    targetphrases = []
    for anchor in anchors:
        if anchor['url'] != link['url']:
            continue
        for phrase in [anchor['text'], *re.split(r'(?<=[.!?])\s+', anchor['text'])]:
            target = norm(phrase)
            if 5 <= len(target) <= 65 and target not in targetphrases:
                targetphrases.append(target)
    candidates = []
    unresolved = 0
    for target in targetphrases:
        hits = []
        n = len(target)
        for i, token in enumerate(tokens):
            if token != target[0]:
                continue
            for size in range(max(5, n - 1), n + 2):
                if i + size > len(tokens):
                    continue
                sample = tokens[i:i + size]
                score = difflib.SequenceMatcher(None, target, sample, autojunk=False).ratio()
                if score < 0.92:
                    continue
                a, b = positions[i], positions[i + size - 1]
                span = words[a:b + 1]
                start, end = span[0]['start'], span[-1]['end']
                if end <= start or end - start > 35 or any(y['start'] - x['end'] > 2 for x, y in zip(span, span[1:])):
                    continue
                confidence = sum(w['probability'] for w in span) / len(span)
                if confidence < 0.75:
                    continue
                hits.append({'a': a, 'b': b, 'start': start, 'end': end, 'score': score, 'confidence': confidence, 'text': ''.join(w['word'] for w in span).strip()})
        hits.sort(key=lambda h: (-h['score'], -(h['b'] - h['a'])))
        if not hits:
            unresolved += 1
            continue
        best = hits[0]
        if any((h['end'] <= best['start'] or h['start'] >= best['end']) and h['score'] >= best['score'] - 0.05 for h in hits[1:]):
            unresolved += 1
            continue
        best['url'] = link['url']
        candidates.append(best)
    # Prefer complete longer spans, while never copying reference text into the imported transcription.
    candidates.sort(key=lambda h: (-(h['b'] - h['a']), -h['score']))
    accepted = []
    for c in candidates:
        if any(c['start'] < a['end'] and c['end'] > a['start'] for a in accepted):
            continue
        accepted.append(c)
    ep = episodes.setdefault(aid, {'id': aid, 'source': link['url'], 'lines': []})
    for c in sorted(accepted, key=lambda c: c['start']):
        lid = line_prefix + hashlib.sha256((aid + str(c['start']) + str(c['end'])).encode()).hexdigest()[:24]
        if any(line['id'] == lid for line in ep['lines']):
            continue
        written = note.replace('{speaker}', speaker).replace('{score}', f"{c['score']:.0%}").replace('{confidence}', f"{c['confidence']:.0%}").replace('{url}', c['url'])
        ep['lines'].append({'id': lid, 'text': c['text'], 'annotated': '', 'start': round(c['start'], 3), 'end': round(c['end'], 3), 'emotion': '', 'delivery': '', 'notes': written})
    audit.append({'path': link['path'], 'source': link['url'], 'referenceCandidates': len(targetphrases), 'matched': len(accepted), 'unresolved': unresolved})
for ep in episodes.values():
    ep['lines'].sort(key=lambda line: line['start'])
(root / 'asr-import-batch.json').write_text(json.dumps({'episodes': list(episodes.values())}, ensure_ascii=False, indent=2))
(root / 'asr-comparison-audit.json').write_text(json.dumps(audit, ensure_ascii=False, indent=2))
print(json.dumps({'episodes': len(episodes), 'lines': sum(len(e['lines']) for e in episodes.values()), 'sources': len(audit)}, ensure_ascii=False))
