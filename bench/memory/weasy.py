# python weasy.py <document.html> <repeats> [marked|free]
#
# weasy.py of ../compare with the phase markers of engine.php.
import json, os, sys, time

import weasyprint

file, repeats = sys.argv[1], int(sys.argv[2])
marked = (sys.argv[3] if len(sys.argv) > 3 else 'marked') == 'marked'


def render():
    return weasyprint.HTML(filename=file).write_pdf()


def mark(m):
    print('@@' + m, flush=True)
    if marked:
        sys.stdin.readline()


start = time.perf_counter()
pdf = render()
first = (time.perf_counter() - start) * 1000
mark('cold')
warm_start = time.time()
times = []
for _ in range(repeats):
    start = time.perf_counter()
    pdf = render()
    times.append((time.perf_counter() - start) * 1000)
warm_end = time.time()
mark('warm')
times.sort()

out = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'out')
os.makedirs(out, exist_ok=True)
with open(os.path.join(out, 'weasyprint-' + os.path.basename(file)[:-5] + '.pdf'), 'wb') as f:
    f.write(pdf)
print(json.dumps({
    'first': round(first, 1),
    'median': round(times[len(times) // 2], 1) if times else None,
    'mean': round(sum(times) / len(times), 1) if times else None,
    'n': len(times), 'warm_start': warm_start, 'warm_end': warm_end,
}))
