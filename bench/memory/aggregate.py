# Prints the medians over the runs in out/results.jsonl and out/parallel.jsonl as Markdown tables.
import json, os, statistics, sys
from collections import defaultdict

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'out')
MB = 2**20
med = statistics.median


def load(name):
    runs = defaultdict(list)
    path = os.path.join(OUT, name)
    if os.path.exists(path):
        for line in open(path):
            r = json.loads(line)
            if r.get('exit') == 0 and r['json']:
                runs[r['label']].append(r)
    return runs


order = ['fagus', 'chrome', 'browsershot', 'mpdf', 'dompdf', 'weasyprint']
names = {'fagus': 'Fagus (PHP + worker)', 'chrome': 'Chrome kept running (node + Chrome)',
         'browsershot': 'Browsershot (PHP + node + Chrome per PDF)', 'mpdf': 'mPDF', 'dompdf': 'Dompdf',
         'weasyprint': 'WeasyPrint'}

runs = load('results.jsonl')
docs = ['classic-3', 'classic-150', 'classic-600', 'modern-3', 'modern-150']
for d in docs:
    print(f'\n### {d}\n')
    print('| Engine | cold peak RSS | cold peak footprint | warm peak RSS | warm peak footprint | RSS growth, idle after the first PDF -> after the last | CPU ms cold (1st PDF incl. startup) | CPU ms/PDF warm | wall ms/PDF warm (median) | wall ms cold | n |')
    print('|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|')
    for e in order:
        rs = runs.get(f'{d} {e}')
        if not rs:
            continue
        rs = [r for r in rs if r.get('exit') == 0 and 'warm' in r]
        n = rs[0]['json'][0]['n']
        row = [
            med(r['cold']['peak_rss'] for r in rs) / MB,
            med(r['cold']['peak_fp'] for r in rs) / MB,
            med(max(r['warm']['peak_rss'], r['cold']['peak_rss']) for r in rs) / MB,
            med(max(r['warm']['peak_fp'], r['cold']['peak_fp']) for r in rs) / MB,
            med((r['idle_after_warm']['rss'] - r['idle_after_cold']['rss']) for r in rs) / MB,
            med(r['cold']['cpu_ms'] for r in rs),
            med(r['warm']['cpu_ms'] / r['json'][0]['n'] for r in rs),
            med(r['json'][0]['median'] for r in rs),
            med(r['json'][0]['first'] for r in rs),
        ]
        print(f'| {names[e]} | {row[0]:.0f} MB | {row[1]:.0f} MB | {row[2]:.0f} MB | {row[3]:.0f} MB | {row[4]:+.1f} MB | {row[5]:.0f} | {row[6]:.1f} | {row[7]:.1f} | {row[8]:.0f} | {len(rs)} |')

print('\n### Idle (after the last PDF, 3 s idle)\n')
print('| Engine | tree RSS | tree footprint | engine-only RSS | engine-only footprint | idle CPU ms/s |')
print('|---|--:|--:|--:|--:|--:|')
for d in ['classic-3', 'classic-150', 'classic-600', 'modern-150']:
    for e in ['fagus', 'chrome']:
        rs = [r for r in runs.get(f'{d} {e}', []) if 'idle_after_warm' in r]
        if not rs:
            continue
        print(f"| {names[e]} {d} | {med(r['idle_after_warm']['rss'] for r in rs)/MB:.0f} MB | {med(r['idle_after_warm']['fp'] for r in rs)/MB:.0f} MB | "
              f"{med(r['idle_after_warm']['engine_rss'] for r in rs)/MB:.0f} MB | {med(r['idle_after_warm']['engine_fp'] for r in rs)/MB:.0f} MB | {med(r['idle_cpu_ms_per_s'] for r in rs):.2f} |")

print('\n### Cross-checks (medians)\n')
print('| run | cold+warm CPU (sampled) | total CPU wait4 (incl. idle+shutdown) | root ru_maxrss | sum lifetime-max fp cold vs sampled cold peak fp |')
print('|---|--:|--:|--:|--:|')
for label, rs in sorted(runs.items()):
    rs = [r for r in rs if 'warm' in r]
    print(f"| {label} | {med(r['cold']['cpu_ms'] + r['warm']['cpu_ms'] for r in rs):.0f} | {med(r['total_cpu_ms_wait4'] for r in rs):.0f} | "
          f"{med(r['root_maxrss'] for r in rs)/MB:.0f} MB | {med(r['cold']['lifemax_fp_sum'] for r in rs)/MB:.0f} / {med(r['cold']['peak_fp'] for r in rs)/MB:.0f} MB |")

par = load('parallel.jsonl')
if par:
    print('\n### Parallel, classic-150\n')
    print('| Setup | peak RSS (steady) | peak footprint (steady) | PDFs/s | CPU ms/PDF (window) | total CPU ms/PDF incl. startup | PDFs |')
    print('|---|--:|--:|--:|--:|--:|--:|')
    for label, rs in par.items():
        rows = []
        for r in rs:
            js = r['json']
            pdfs = sum(j['n'] for j in js)
            window = max(j['warm_end'] for j in js) - min(j['warm_start'] for j in js)
            total_pdfs = pdfs + len(js)
            rows.append((r['warm']['peak_rss'], r['warm']['peak_fp'], pdfs / window,
                         r['warm']['cpu_ms'] / pdfs, r['total_cpu_ms_wait4'] / total_pdfs, pdfs))
        m = [med(x[i] for x in rows) for i in range(6)]
        print(f'| {label} | {m[0]/MB:.0f} MB | {m[1]/MB:.0f} MB | {m[2]:.1f} | {m[3]:.1f} | {m[4]:.1f} | {m[5]:.0f} |')
