#!/usr/bin/env python3
"""What the agent does call by call, from the conversations `eval/agent.py` keeps (`*.trace.jsonl`):
what a run's calls are, which of them bring nothing, which runs never use a gitnext tool, what is lost
and won against plain tools on the same task, and how the failed runs end.

    python3 eval/agent_traces.py <folder with agent-<model>-r<set>-<repo>.trace.jsonl, at any depth>
"""
import collections, glob, json, os, re, sys

DIR = sys.argv[1] if len(sys.argv) > 1 else "."


def runs():
    for f in sorted(glob.glob(os.path.join(DIR, '**', 'agent-*.trace.jsonl'), recursive=True)):
        m = re.search(r'agent-(?:qwen3\.5-)?([\w.]+)-r(\d+)-([\w-]+)\.trace', f)
        for l in open(f):
            r = json.loads(l)
            r['model'] = m.group(1); r['set'] = int(m.group(2))
            yield r
def calls(r):
    """(tool, args text, result text) of every call, in order."""
    out = []; ms = r['messages']
    for i, m in enumerate(ms):
        if m['role'] == 'assistant':
            cs = m.get('calls') or []
            res = [x['content'] for x in ms[i+1:i+1+len(cs)] if x['role'] == 'tool']
            for k, c in enumerate(cs):
                t, _, a = c.partition(' ')
                out.append((t, a, res[k] if k < len(res) else ''))
    return out


R = list(runs())
MODELS = sorted({r['model'] for r in R}, key=lambda m: (float(re.sub(r'[^\d.]', '', m) or 0), m))
CAT = {'grep': 'search', 'list_dir': 'search', 'read_file': 'read', 'edit_file': 'edit', 'replace': 'edit', 'replace_symbol': 'edit', 'add_after': 'edit',
       'rename': 'edit', 'run': 'run', 'check': 'check', 'look': 'look', 'finish': 'finish'}
def waste(t, a, res, seen):
    if res.startswith('Not run again'): return 'repeat'
    if t == 'grep' and res.startswith('no matches'): return 'empty search'
    if t == 'look' and res.startswith(('nothing found', 'nothing has all')): return 'empty search'
    if t in ('edit_file', 'replace', 'replace_symbol', 'add_after', 'rename') and res.startswith('error'): return 'edit refused'
    if t == 'read_file':
        try: p = json.loads(a).get('path')
        except Exception: p = a
        if p in seen: return 'reread'
        seen.add(p)
    if t == 'run' and re.match(r"exit [1-9]\d*\n(/bin/sh|Error in sitecustomize|error: no such|bash:)", res): return 'command error'
    return ''
print('== calls per run and what they are (mean per run) ==')
for task in [t for t in ('find', 'callers', 'fix', 'rename') if any(r['task'] == t for r in R)]:
    for mode in ('base', 'gitnext', 'lite'):
        rs = [r for r in R if r['task'] == task and r['mode'] == mode]
        c = collections.Counter(); w = collections.Counter(); n = 0
        for r in rs:
            seen = set()
            for t, a, res in calls(r):
                n += 1; c[CAT.get(t, t)] += 1
                x = waste(t, a, res, seen)
                if x: w[x] += 1
        k = len(rs)
        print(f"{task:8}{mode:8} pass {sum(r['passed'] for r in rs):3}/{k} calls {n/k:5.1f} | " + ' '.join(f"{a} {b/k:.1f}" for a, b in c.most_common()) + ' || wasted ' + f"{sum(w.values())/k:.1f}: " + ' '.join(f"{a} {b/k:.1f}" for a, b in w.most_common()))
print('== the same by model, all tasks: calls per run, wasted per run ==')
for model in MODELS:
    for mode in ('base', 'gitnext', 'lite'):
        rs = [r for r in R if r['model'] == model and r['mode'] == mode]
        n = w = 0
        for r in rs:
            seen = set()
            for t, a, res in calls(r):
                n += 1; w += bool(waste(t, a, res, seen))
        print(f"{model:4}{mode:8} calls {n/len(rs):5.1f} wasted {w/len(rs):4.1f} ({100*w/n:.0f}%)")
print('== gitnext mode (11 tools): runs that never call a gitnext tool ==')
G = {'look', 'check', 'rename', 'replace_symbol', 'insert_after'}
for model in MODELS:
    rs = [r for r in R if r['model'] == model and r['mode'] == 'gitnext']
    no = [r for r in rs if not any(t in G for t, _, _ in calls(r))]
    yes = [r for r in rs if r not in no]
    print(model, f"no gitnext tool: {len(no)}/{len(rs)} runs, passed {sum(r['passed'] for r in no)}; with: {len(yes)}, passed {sum(r['passed'] for r in yes)}",
          'by task (no tool):', dict(collections.Counter(r['task'] for r in no)))
print('== lost and won against plain tools (same task, same set) ==')
key = lambda r: (r['model'], r['repo'], r['task'], r['target'], r['set'])
by = {(key(r), r['mode']): r for r in R}
for mode in ('gitnext', 'lite'):
    lost = [k for (k, m), r in by.items() if m == 'base' and r['passed'] and (k, mode) in by and not by[(k, mode)]['passed']]
    won = [k for (k, m), r in by.items() if m == 'base' and not r['passed'] and (k, mode) in by and by[(k, mode)]['passed']]
    print(mode, 'lost', len(lost), dict(collections.Counter((k[0], k[2]) for k in lost)), '| won', len(won), dict(collections.Counter((k[0], k[2]) for k in won)))
print('== how failed runs end ==')
for mode in ('base', 'gitnext', 'lite'):
    c = collections.Counter()
    for r in R:
        if r['mode'] != mode or r['passed']: continue
        cs = calls(r)
        fin = any(t == 'finish' for t, _, _ in cs)
        ed = any(CAT.get(t) == 'edit' and not res.startswith(('error', 'Not run')) for t, _, res in cs)
        steps = sum(1 for m in r['messages'] if m['role'] == 'assistant')
        c[(r['task'], 'out of steps' if steps >= 25 and not fin else ('finished' if fin else 'stopped') + (', changed code' if ed else ', no change') if r['task'] in ('fix', 'rename') else ('wrong answer' if fin else 'no answer'))] += 1
    print(mode, sorted(c.items()))
