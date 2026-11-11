#!/usr/bin/env python3
"""Summarize all trials in one schema-v2 run; never select a best trial."""
import json
import math
from pathlib import Path
import statistics
import sys

run = Path(sys.argv[1])
metadata = json.loads((run/'metadata.json').read_text())
assert metadata.get('schema_version') == 2, 'Use corrected schema-v2 run'
rows = [json.loads(x) for x in (run/'raw.jsonl').read_text().splitlines()]
matrix = [x for x in rows if x['kind'] == 'matrix']
assert len(matrix) == 36
assert all(x['daemon_exit_code'] == 0 and x['private_directory_removed'] for x in matrix)
assert {(x['repeat'],x['mode'],x['n']) for x in matrix} == {(r,m,n) for r in range(1,4) for m in ('stalled','idle','active') for n in (1,8,32,64)}

def pct(v):
    v=sorted(v)
    return {'n':len(v), **({f'p{p}_us':v[math.ceil(p/100*len(v))-1] for p in (50,95)} if v else {})}

summary={'source': metadata['source_head']['stdout'], 'binary_sha256':metadata['binary_sha256'],
         'run':str(run), 'performance_claim':False, 'cells':[], 'storms':[]}
for mode in ('stalled','idle','active'):
    for n in (1,8,32,64):
        trials=[x for x in matrix if x['mode']==mode and x['n']==n]
        t=[]
        for x in trials:
            b=x['baseline'][-1]
            loaded=x['loaded']
            z=x['drain'][-1]
            last=loaded[-1]
            t.append({'repeat':x['repeat'], 'baseline_rss_kib':b['rss_bytes']/1024,
                'baseline_fds':b['fd_count'], 'baseline_threads':b['threads'],
                'loaded_median_rss_delta_kib':statistics.median(y['rss_bytes'] for y in loaded)/1024-b['rss_bytes']/1024,
                'loaded_peak_rss_delta_kib':max(y['rss_bytes'] for y in loaded)/1024-b['rss_bytes']/1024,
                'loaded_peak_fd_delta':max(y['fd_count'] for y in loaded)-b['fd_count'],
                'loaded_peak_thread_delta':max(y['threads'] for y in loaded)-b['threads'],
                'loaded_peak_vm_delta_kib':(max(y['virtual_bytes'] for y in loaded)-b['virtual_bytes'])/1024,
                'drain_fd_delta':z['fd_count']-b['fd_count'], 'drain_thread_delta':z['threads']-b['threads'],
                'drain_rss_delta_kib':(z['rss_bytes']-b['rss_bytes'])/1024,
                'cpu_percent_connect_and_load':100*(last['cpu_total_ns']-b['cpu_total_ns'])/(last['monotonic_ns']-b['monotonic_ns'])})
        assert all(x['loaded_peak_fd_delta']==n and x['loaded_peak_thread_delta']==n for x in t)
        assert all(x['drain_fd_delta']==0 and x['drain_thread_delta']==0 for x in t)
        summary['cells'].append({'mode':mode,'n':n,'trials':t,
            'median_loaded_rss_delta_kib':statistics.median(x['loaded_median_rss_delta_kib'] for x in t),
            'hello':pct([v for x in trials for v in x['hello_us']]),
            'resume':pct([v for x in trials for v in x['resume_us']])})
storm=[x for x in rows if x['kind']=='storm_run']
assert len(storm)==1 and storm[0]['daemon_exit_code']==0 and storm[0]['private_directory_removed']
for x in storm[0]['cycles']:
    b=x['baseline_with_admitted_client'][-1]
    z=x['drain'][-1]
    assert x['stalled_peer_eof_count']==62
    assert z['fd_count']==b['fd_count'] and z['threads']==b['threads']
    summary['storms'].append({'cycle':x['cycle'],'duration_seconds':x['duration_seconds'],
        'hello':pct(x['hello_us']),'resume':pct(x['resume_us']),
        'baseline_rss_kib':b['rss_bytes']/1024,'drain_rss_kib':z['rss_bytes']/1024,
        'baseline_fds':b['fd_count'],'drain_fds':z['fd_count'],
        'baseline_threads':b['threads'],'drain_threads':z['threads'],
        'peak_threads':max(y['threads'] for y in x['samples']),
        'peak_fds':max(y['fd_count'] for y in x['samples']),
        'eof_count':x['stalled_peer_eof_count'],'post_drain_resume_us':x['post_drain_resume_us']})
(run/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
lines=['| State | N | RSS delta KiB (median of 3 trial medians) | FD / thread delta (all trials) | Hello p50 / p95 us (samples) |',
       '|---|---:|---:|---:|---:|']
for x in summary['cells']:
    h=x['hello']
    v=f"{h['p50_us']:.1f} / {h['p95_us']:.1f} ({h['n']})" if h['n'] else 'N/A (no Hello)'
    lines.append(f"| {x['mode']} | {x['n']} | {x['median_loaded_rss_delta_kib']:.0f} | +{x['n']} / +{x['n']} | {v} |")
(run/'table.md').write_text('\n'.join(lines)+'\n')
print('\n'.join(lines))
print(json.dumps(summary['storms'],indent=2))
