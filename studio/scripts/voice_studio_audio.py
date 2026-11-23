"""Render readable caption cues from ASR word times, without inventing speaker labels."""
from html import escape

def render_vtt(segments):
    def stamp(n):
        ms = round(n * 1000)
        return f'{ms//3600000:02}:{ms//60000%60:02}:{ms//1000%60:02}.{ms%1000:03}'
    cues = []
    for segment in segments:
        group = []
        for word in segment.get('words', []):
            if group and (word['start'] - group[-1]['end'] > 1 or word['end'] - group[0]['start'] > 6 or len(group) >= 12):
                cues.append(group)
                group = []
            group.append(word)
            if word['word'].strip().endswith(('.', '?', '!')):
                cues.append(group)
                group = []
        if group:
            cues.append(group)
    blocks = []
    for group in cues:
        if group[-1]['end'] <= group[0]['start']:
            continue
        text = escape(''.join(w['word'] for w in group).strip(), quote=False)
        blocks.append(f'{len(blocks)+1}\n{stamp(group[0]["start"])} --> {stamp(group[-1]["end"])}\n{text}')
    return 'WEBVTT\n\nNOTE Automatically transcribed; review required.\n\n' + '\n\n'.join(blocks) + '\n'
