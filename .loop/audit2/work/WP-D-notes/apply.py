# Replace spec section bodies with the ```text blocks of a WP-D notes file. Usage: apply.py notes.md [§...]
import re, sys
notes, only = sys.argv[1], set(sys.argv[2:])
spec = 'docs/init/AAE-SPEC.md'
blocks, cur, f, buf = {}, None, False, []
for l in open(notes, encoding='utf-8').read().split('\n'):
    m = re.match(r'^### (§[\d.]+) — сжимать: да', l)
    if m: cur = m.group(1); continue
    if l.startswith('### '): cur = None
    if l == '```text' and cur: f, buf = True, []; continue
    if l == '```' and f: f = False; blocks[cur] = buf; cur = None; continue
    if f: buf.append(l)
lines = open(spec, encoding='utf-8').read().split('\n')
for sec, body in blocks.items():
    if only and sec not in only: continue
    num = sec[1:]
    h = [i for i, l in enumerate(lines) if re.match(r'^#{2,3} ' + re.escape(num) + r'[. ]', l)]
    assert len(h) == 1, (sec, h)
    s = h[0] + 1; e = s; fence = False
    while e < len(lines):
        if lines[e].startswith('```'): fence = not fence
        if not fence and (lines[e].startswith('#') or lines[e] == '---'): break
        e += 1
    old = e - s
    lines[s:e] = [''] + body + ['']
    print(f'{sec}: {old} -> {len(body)+2} lines')
open(spec, 'w', encoding='utf-8').write('\n'.join(lines))
