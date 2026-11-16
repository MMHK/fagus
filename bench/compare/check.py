# python check.py
#
# Prints the medians of out/results.txt with the page count of each PDF, then which feature probes
# (probes/*.html) each engine passed, judged from the text and its position in out/<engine>-<probe>.pdf.
import json, os, re, statistics
import pdfplumber, pypdf

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'out')
ENGINES = ['fagus', 'chrome', 'weasyprint', 'dompdf', 'mpdf']
PROBES = ['flex', 'grid', 'margin', 'running', 'strings', 'toc', 'thead', 'footnote', 'outline', 'hyphen']


def words(pdf):
    return [[(w['text'], w['top']) for w in p.extract_words()] for p in pdf.pages]


def find(page, text):
    return [t for (w, t) in page if text in w]


def joined(page):
    return ''.join(w for (w, _) in page)


def check(engine, probe):
    path = os.path.join(OUT, f'{engine}-{probe}.pdf')
    if not os.path.exists(path):
        return 'missing'
    try:
        if probe == 'outline':
            return 'yes' if pypdf.PdfReader(path).outline else 'no'
        with pdfplumber.open(path) as pdf:
            ws = words(pdf)
            h = pdf.pages[0].height
        allw = [w for p in ws for w in p]
        if probe == 'flex':
            a, b = find(allw, 'FLEXA'), find(allw, 'FLEXB')
            return 'yes' if a and b and abs(a[0] - b[0]) < 3 else 'no'
        if probe == 'grid':
            a, b, c = find(allw, 'GRIDA'), find(allw, 'GRIDB'), find(allw, 'GRIDC')
            return 'yes' if a and b and c and abs(a[0] - b[0]) < 3 and c[0] > a[0] + 3 else 'no'
        if probe == 'margin':
            if len(ws) < 2:
                return 'no'
            j = joined(ws[1])
            return 'yes' if 'MARGINHEAD' in j and 'PG2OF2' in j else ('partial' if 'MARGINHEAD' in j or 'PG2' in j else 'no')
        if probe == 'running':
            return 'yes' if len(ws) > 1 and find(ws[1], 'RUNNINGHEAD') else 'no'
        if probe == 'strings':
            return 'yes' if len(ws) > 1 and len(find(ws[1], 'CHAPTERTWO')) >= 2 and find(ws[0], 'CHAPTERONE') and min(find(ws[0], 'CHAPTERONE')) < 80 else 'no'
        if probe == 'toc':
            return 'yes' if 'TOCPAGE3' in joined(ws[0]) else 'no'
        if probe == 'thead':
            return 'yes' if len(ws) > 1 and find(ws[1], 'HEADERCELL') else 'no'
        if probe == 'footnote':
            f = find(allw, 'FOOTNOTETEXT')
            return 'yes' if f and f[0] > h * 0.7 else 'no'
        if probe == 'hyphen':
            return 'yes' if any(re.search(r'[-‐­]$', w) for (w, _) in allw) else 'no'
    except Exception as e:  # noqa
        return 'error ' + str(e)[:40]


results = {}
if os.path.exists(os.path.join(OUT, 'results.txt')):
    for line in open(os.path.join(OUT, 'results.txt')):
        document, engine, data = (line.split(' ', 2) + ['', ''])[:3]
        if not data.startswith('{'):
            continue
        results.setdefault((document, engine), []).append(json.loads(data))
for (document, engine), runs in results.items():
    first = statistics.median(r['first'] for r in runs)
    repeat = statistics.median(r['repeat'] for r in runs)
    pages = len(pypdf.PdfReader(os.path.join(OUT, f'{engine}-{document}.pdf')).pages)
    print(f'{document:12} {engine:12} first {first:8.1f} ms   repeated {repeat:8.1f} ms   {pages:3} pages')
print()

print('probe'.ljust(10), *[e.ljust(11) for e in ENGINES])
for p in PROBES:
    print(p.ljust(10), *[check(e, p).ljust(11) for e in ENGINES])
