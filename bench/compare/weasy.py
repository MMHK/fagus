# python weasy.py <document.html> <repeats>
#
# Prints JSON like bench.php and writes out/weasyprint-<document>.pdf.
import json, os, sys, time

import weasyprint

file, repeats = sys.argv[1], int(sys.argv[2])


def render():
    return weasyprint.HTML(filename=file).write_pdf()


start = time.perf_counter()
pdf = render()
first = (time.perf_counter() - start) * 1000
times = []
for _ in range(repeats):
    start = time.perf_counter()
    pdf = render()
    times.append((time.perf_counter() - start) * 1000)
times.sort()

out = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'out')
os.makedirs(out, exist_ok=True)
with open(os.path.join(out, 'weasyprint-' + os.path.basename(file)[:-5] + '.pdf'), 'wb') as f:
    f.write(pdf)
print(json.dumps({'first': round(first, 1), 'repeat': round(times[len(times) // 2], 1) if times else None}))
