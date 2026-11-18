# The performance budgets of Fagus alone, on macOS: render times and peak memory at 1, 10 and 100
# pages, the start of a process until it is ready, and one invoice with the start of a process.
#
#   python budgets.py [what...]    what: times, memory, start, retained (default: all of them)
#
# The invoices are those of ../compare/gen-classic.py (CSS 2.1) and ../../tools/gen.py (flex, grid,
# margin boxes) with as many lines as give 1, 10 and 100 pages. Before each measurement the script
# waits (up to WAIT seconds, 300 by default) until the load average of the last minute is below
# MAX_LOAD (4 by default) and records the load it measured under. Results are appended to
# out/budgets.jsonl and printed as Markdown.
import json, os, statistics, struct, subprocess, sys, time

import driver
import procmon

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
DOCS = os.path.join(REPO, 'bench', 'compare', 'docs')
OUT = os.path.join(HERE, 'out')
FAGUS = os.environ.get('FAGUS', os.path.join(REPO, 'target', 'release', 'fagus'))
MAX_LOAD = float(os.environ.get('MAX_LOAD', '4'))
WAIT = float(os.environ.get('WAIT', '300'))
MB = 2**20
med = statistics.median

# Lines for 1, 10 and 100 pages.
CLASSIC = {1: 3, 10: 150, 100: 1580}
MODERN = {1: 3, 10: 150, 100: 1680}


def quiet():
    """Waits for a quiet moment and returns the load averages it found."""
    end = time.monotonic() + WAIT
    while os.getloadavg()[0] >= MAX_LOAD and time.monotonic() < end:
        time.sleep(2)
    return [round(x, 2) for x in os.getloadavg()]


def documents():
    os.makedirs(os.path.join(DOCS, 'fonts'), exist_ok=True)
    for name in ('Inter-Regular.ttf', 'Inter-Bold.ttf'):
        with open(os.path.join(REPO, 'tests', 'fonts', name), 'rb') as f:
            data = f.read()
        with open(os.path.join(DOCS, 'fonts', name), 'wb') as f:
            f.write(data)
    docs = {}
    for kind, lines, generator in (('classic', CLASSIC, [os.path.join(REPO, 'bench', 'compare', 'gen-classic.py')]),
                                   ('modern', MODERN, [os.path.join(REPO, 'tools', 'gen.py')])):
        for pages, n in lines.items():
            path = os.path.join(DOCS, f'{kind}-{n}.html')
            args = [str(n)] + (['fonts'] if kind == 'modern' else [])
            with open(path, 'w') as f:
                subprocess.run([sys.executable] + generator + args, stdout=f, check=True)
            docs[(kind, pages)] = path
    with open(os.path.join(OUT, 'trivial.html'), 'w') as f:
        f.write('<!doctype html><p>Hello</p>\n')
    return docs


def render(doc, repeat):
    """fagus render --repeat: the page count and the median in milliseconds of one warm engine."""
    p = subprocess.run([FAGUS, 'render', doc, '-o', os.path.join(OUT, 'budget.pdf'), '--timings',
                        '--repeat', str(repeat)], capture_output=True, text=True, check=True)
    line = p.stderr.strip().splitlines()[-1]  # "N pages, median X ms (…)"
    return int(line.split()[0]), float(line.split('median ')[1].split()[0])


def times(docs, out):
    repeats = {1: 200, 10: 50, 100: 10}
    for (kind, pages), doc in docs.items():
        load = quiet()
        runs = [render(doc, repeats[pages]) for _ in range(3)]
        out.append({'what': 'time', 'kind': kind, 'pages': runs[0][0], 'ms': med(r[1] for r in runs),
                    'runs': [r[1] for r in runs], 'load': load})


def peak(cmd):
    """Runs cmd and returns its peak RSS (from wait4) and peak phys_footprint (the kernel's)."""
    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    footprint = 0
    while True:
        ri = procmon.rusage(p.pid)  # also answers for the zombie, until it is reaped
        if ri is not None:
            footprint = max(footprint, ri.ri_lifetime_max_phys_footprint)
        pid, status, rusage = os.wait4(p.pid, os.WNOHANG)
        if pid:
            return rusage.ru_maxrss, footprint
        time.sleep(0.0005)


def memory(docs, out):
    for (kind, pages), doc in docs.items():
        for repeat in (1, 20):
            load = quiet()
            runs = [peak([FAGUS, 'render', doc, '-o', os.path.join(OUT, 'budget.pdf'), '--repeat', str(repeat)])
                    for _ in range(3)]
            out.append({'what': 'memory', 'kind': kind, 'pages': pages, 'repeat': repeat,
                        'rss': med(r[0] for r in runs), 'fp': med(r[1] for r in runs), 'load': load})
    # The worker, driven from PHP as an application drives it: the peaks of the fagus process.
    php = ['php', '-d', 'memory_limit=4G', '-d', 'error_reporting=0', os.path.join(HERE, 'engine.php'), 'fagus']
    for kind in ('classic', 'modern'):
        load = quiet()
        runs = [driver.run(php + [docs[(kind, 100)], '20'], env=driver.environment()) for _ in range(3)]
        engine = [next(v for k, v in r['peaks'].items() if k.endswith(':fagus')) for r in runs]
        out.append({'what': 'worker-memory', 'kind': kind, 'pages': 100, 'repeat': 21,
                    'rss': med(e[0] for e in engine), 'fp': med(e[1] for e in engine),
                    'ms': med(r['json'][0]['median'] for r in runs),
                    'first_ms': med(r['json'][0]['first'] for r in runs),
                    'idle_rss': med(r['idle_after_warm']['engine_rss'] for r in runs),
                    'idle_fp': med(r['idle_after_warm']['engine_fp'] for r in runs), 'load': load})


# The worker protocol (docs/protocol.md).
def frame(kind, header, blobs=()):
    head = json.dumps(dict(header, blobs=[len(b) for b in blobs]) if blobs else header).encode()
    body = bytes([kind]) + struct.pack('<I', len(head)) + head + b''.join(blobs)
    return struct.pack('<I', len(body)) + body


def read_exact(stream, n):
    data = b''
    while len(data) < n:
        chunk = stream.read(n - len(data))
        if not chunk:
            raise EOFError('the worker has ended')
        data += chunk
    return data


def write_all(stream, data):
    view = memoryview(data)
    while view:
        view = view[stream.write(view):]


def read_frame(stream, first_byte=None):
    """Reads a frame; first_byte gets the time its first byte arrived."""
    length = read_exact(stream, 1)
    if first_byte is not None:
        first_byte.append(time.perf_counter())
    length += read_exact(stream, 3)
    body = read_exact(stream, struct.unpack('<I', length)[0])
    head_length = struct.unpack('<I', body[1:5])[0]
    return chr(body[0]), json.loads(body[5:5 + head_length])


def request(doc, id):
    with open(doc, 'rb') as f:
        html = f.read()
    return frame(ord('R'), {'id': id, 'html': 0, 'base': os.path.dirname(doc)}, [html])


def worker_first(doc, warm=0):
    """Starts a worker and sends it doc at once. Returns the time from the start of the process to
    its hello frame and to the first byte of the PDF, the times of the render (its own timings),
    and the median of `warm` further renders of doc (sent to the PDF's last byte)."""
    req = request(doc, 1)
    t0 = time.perf_counter()
    p = subprocess.Popen([FAGUS, 'worker'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, bufsize=0)
    write_all(p.stdin, req)
    first = []
    kind, _ = read_frame(p.stdout, first)
    assert kind == 'H'
    hello = first[0] - t0
    kind, header = read_frame(p.stdout, first)
    assert kind == 'P', header
    pdf = first[1] - t0
    rendered = sum(header['timings'].values()) / 1000
    warms = []
    for i in range(warm):
        start = time.perf_counter()
        write_all(p.stdin, request(doc, i + 2))
        read_frame(p.stdout)
        warms.append(time.perf_counter() - start)
    p.stdin.close()
    p.wait()
    return hello * 1000, pdf * 1000, rendered * 1000, (med(warms) * 1000 if warms else None)


def worker_hello():
    t0 = time.perf_counter()
    p = subprocess.Popen([FAGUS, 'worker'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, bufsize=0)
    read_frame(p.stdout)
    ready = time.perf_counter() - t0
    p.stdin.close()
    p.wait()
    return ready * 1000


def spawn(cmd):
    t0 = time.perf_counter()
    subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    return (time.perf_counter() - t0) * 1000


def start(docs, out):
    n = 50
    trivial = os.path.join(OUT, 'trivial.html')
    invoice = docs[('classic', 1)]
    pdf = os.path.join(OUT, 'budget.pdf')
    load = quiet()
    true = med(spawn(['/usr/bin/true']) for _ in range(n))
    hello = med(worker_hello() for _ in range(n))
    out.append({'what': 'start', 'case': 'process start of /usr/bin/true (what the harness costs)', 'ms': true, 'load': load})
    out.append({'what': 'start', 'case': 'fagus worker, start to hello (ready)', 'ms': hello, 'load': load})
    for name, doc in (('trivial document', trivial), ('1-page invoice', invoice)):
        load = quiet()
        runs = [worker_first(doc, warm=20) for _ in range(n)]
        out.append({'what': 'start', 'case': f'fagus worker, start to first byte of the PDF, {name}',
                    'ms': med(r[1] for r in runs), 'load': load})
        out.append({'what': 'start', 'case': f'  the same minus the render (its own timings), {name}',
                    'ms': med(r[1] - r[2] for r in runs), 'load': load})
        out.append({'what': 'start', 'case': f'  the same minus a warm render (sent to last byte), {name}',
                    'ms': med(r[1] - r[3] for r in runs), 'load': load})
        load = quiet()
        cli = med(spawn([FAGUS, 'render', doc, '-o', pdf]) for _ in range(n))
        out.append({'what': 'start', 'case': f'fagus render, start to exit, {name}', 'ms': cli, 'load': load})
    # From PHP: a new Fagus\Engine, its worker and the first PDF, as in a PHP-FPM request.
    load = quiet()
    php = []
    for _ in range(10):
        p = subprocess.run(['php', '-d', 'error_reporting=0', os.path.join(HERE, 'engine.php'), 'fagus',
                            invoice, '0', 'free'], capture_output=True, text=True, check=True)
        php.append(json.loads(p.stdout.strip().splitlines()[-1])['first'])
    out.append({'what': 'start', 'case': 'PHP, new Engine to the PDF, 1-page invoice', 'ms': med(php), 'load': load})


def retained(docs, out):
    """What a worker keeps after a large document: one worker renders small, large, then small
    documents; RSS and phys_footprint of the fagus process after each step, at rest. Once with
    the default allocator settings and once each with two settings of macOS's malloc."""
    small, large = docs[('classic', 1)], docs[('classic', 100)]
    for env_name, extra in (('default', {}), ('MallocLargeCache=0', {'MallocLargeCache': '0'}),
                            ('MallocSpaceEfficient=1', {'MallocSpaceEfficient': '1'})):
        load = quiet()
        p = subprocess.Popen([FAGUS, 'worker'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, bufsize=0,
                             env=dict(os.environ, **extra))
        read_frame(p.stdout)
        steps, id = [], 0

        def send(doc, times):
            nonlocal id
            for _ in range(times):
                id += 1
                write_all(p.stdin, request(doc, id))
                read_frame(p.stdout)
            time.sleep(1)
            ri = procmon.rusage(p.pid)
            return (round(ri.ri_resident_size / MB, 1), round(ri.ri_phys_footprint / MB, 1),
                    round(ri.ri_lifetime_max_phys_footprint / MB, 1))

        steps.append(('started', send(small, 0)))
        steps.append(('1 x 1 page', send(small, 1)))
        steps.append(('then 1 x 100 pages', send(large, 1)))
        steps.append(('then 20 x 1 page', send(small, 20)))
        steps.append(('then 20 x 100 pages', send(large, 20)))
        steps.append(('then 20 x 1 page', send(small, 20)))
        p.stdin.close()
        p.wait()
        out.append({'what': 'retained', 'env': env_name, 'steps': steps, 'load': load})


def table(out):
    budget = {1: 5, 10: 35, 100: 300}
    print('| Measure | Pages | Measured | Budget | Load |')
    print('|---|--:|--:|--:|--:|')
    for r in out:
        if r['what'] == 'time':
            print(f"| render, {r['kind']} invoice | {r['pages']} | {r['ms']:.1f} ms | ≤ {budget.get(r['pages'], '–')} ms | {r['load'][0]} |")
        elif r['what'] == 'memory':
            print(f"| fagus render --repeat {r['repeat']}, {r['kind']} invoice: peak RSS / footprint | {r['pages']} | "
                  f"{r['rss'] / MB:.0f} / {r['fp'] / MB:.0f} MB | {'≤ 64 MB' if r['pages'] == 100 else ''} | {r['load'][0]} |")
        elif r['what'] == 'worker-memory':
            print(f"| worker from PHP, {r['repeat']} PDFs, {r['kind']} invoice: peak RSS / footprint (idle after) | 100 | "
                  f"{r['rss'] / MB:.0f} / {r['fp'] / MB:.0f} MB ({r['idle_rss'] / MB:.0f} / {r['idle_fp'] / MB:.0f} MB), "
                  f"{r['ms']:.0f} ms per PDF | ≤ 64 MB | {r['load'][0]} |")
        elif r['what'] == 'start':
            print(f"| {r['case']} | | {r['ms']:.2f} ms | | {r['load'][0]} |")
    for r in out:
        if r['what'] == 'retained':
            print(f"\nRetained by one worker ({r['env']}, load {r['load'][0]}), at rest after each step:\n")
            print('| Step | RSS | footprint | peak footprint so far |')
            print('|---|--:|--:|--:|')
            for step, (rss, fp, peak_fp) in r['steps']:
                print(f'| {step} | {rss} MB | {fp} MB | {peak_fp} MB |')


if __name__ == '__main__':
    what = sys.argv[1:] or ['times', 'memory', 'start', 'retained']
    os.makedirs(OUT, exist_ok=True)
    docs = documents()
    out = []
    for w in what:
        globals()[w](docs, out)
    with open(os.path.join(OUT, 'budgets.jsonl'), 'a') as f:
        for r in out:
            f.write(json.dumps(dict(r, date=time.strftime('%Y-%m-%d %H:%M'))) + '\n')
    table(out)
