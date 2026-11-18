# Memory and CPU time of a renderer's whole process tree (PHP or Node, and the engine processes
# they start) on macOS.
#
#   driver.py single <label> <cmd...>               one process with phase markers
#   driver.py parallel <label> <workers> <cmd...>   <workers> copies at once, without markers
#
# The command is engine.php, chrome.js or weasy.py. Marked, it renders once and prints "@@cold",
# waits for a line on its input (the driver gives it IDLE_COLD seconds of rest first), renders
# repeatedly, prints "@@warm", waits again (IDLE_WARM seconds), then prints its times as JSON.
#
# The tree is sampled about every millisecond with proc_pid_rusage: the resident size (RSS) and
# phys_footprint of every process, summed, and their peaks per phase. CPU time is the sum over the
# live processes of their own time and that of their terminated children, so helpers that have
# ended count too. One JSON line per run is appended to out/results.jsonl (RESULTS=… for another
# file in out/).
import json, os, queue, subprocess, sys, threading, time

import psutil

import procmon

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, 'out')
IDLE_COLD, IDLE_WARM = 2.0, 3.0


def cpu_total(s):
    return sum(v[0] + v[1] for v in s.values())


def is_engine(name):
    n = name.lower()
    return 'fagus' in n or 'chrome' in n


def names(s):
    out = {}
    for pid in s:
        try:
            out[pid] = psutil.Process(pid).name()
        except psutil.Error:
            out[pid] = '?'
    return out


def snapshot(s):
    """The tree's memory, and that of the engine processes alone (fagus, Chrome)."""
    nm = names(s)
    eng = [p for p in s if is_engine(nm[p])]
    return {
        'rss': sum(v[2] for v in s.values()),
        'fp': sum(v[3] for v in s.values()),
        'engine_rss': sum(s[p][2] for p in eng),
        'engine_fp': sum(s[p][3] for p in eng),
        'procs': {f'{p}:{nm[p]}': [round(s[p][2] / 2**20, 1), round(s[p][3] / 2**20, 1)] for p in s},
    }


def reader(stream, q):
    for line in stream:
        q.put(line.rstrip('\n'))
    q.put(None)


def run(cmd, expected_cold=1, marked=True, env=None):
    """Runs cmd to its end and returns what was measured. expected_cold: how many processes print
    the markers (all of them must have printed one for the phase to end)."""
    os.makedirs(OUT, exist_ok=True)
    t0 = time.monotonic()
    p = subprocess.Popen(cmd, cwd=HERE, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=open(os.path.join(OUT, 'stderr.log'), 'a'), text=True, env=env,
                         start_new_session=True)
    q = queue.Queue()
    threading.Thread(target=reader, args=(p.stdout, q), daemon=True).start()
    phase, colds, warms, idle_until = 'cold', 0, 0, None
    peak = {'cold': [0, 0], 'warm': [0, 0]}
    res = {'cmd': cmd, 'json': []}
    life_max, rss_max, pid_names, seen = {}, {}, {}, {}
    nsamples = 0
    status = rusage = None
    while True:
        s = procmon.sample(p.pid)
        nsamples += 1
        if s:
            rss = sum(v[2] for v in s.values())
            fp = sum(v[3] for v in s.values())
            if phase in peak:
                peak[phase][0] = max(peak[phase][0], rss)
                peak[phase][1] = max(peak[phase][1], fp)
            for pid, v in s.items():
                life_max[pid] = max(life_max.get(pid, 0), v[4])
                rss_max[pid] = max(rss_max.get(pid, 0), v[2])
                # A child is the parent's copy until it has called exec: names its first samples.
                seen[pid] = seen.get(pid, 0) + 1
                if seen[pid] <= 20:
                    pid_names.update(names({pid: v}))
        now = time.monotonic()
        while True:
            try:
                line = q.get_nowait()
            except queue.Empty:
                break
            if line is None:
                continue
            if line == '@@cold':
                colds += 1
                if colds == expected_cold:
                    res['cold'] = {'cpu_ms': cpu_total(s) / 1e6, 'wall_ms': (now - t0) * 1000,
                                   'peak_rss': peak['cold'][0], 'peak_fp': peak['cold'][1],
                                   'lifemax_fp_sum': sum(life_max.values())}
                    if marked:
                        phase, idle_until = 'idle1', now + IDLE_COLD
                    else:
                        phase = 'warm'
                        res['warm_cpu0'] = cpu_total(s) / 1e6
                        res['warm_t0'] = now
            elif line == '@@warm' and (warms := warms + 1) == expected_cold:
                res['warm'] = {'cpu_ms': cpu_total(s) / 1e6 - res['warm_cpu0'],
                               'wall_ms': (now - res['warm_t0']) * 1000,
                               'peak_rss': peak['warm'][0], 'peak_fp': peak['warm'][1],
                               'end': snapshot(s)}
                if marked:
                    phase, idle_until = 'idle2', now + IDLE_WARM
                    res['idle2_cpu0'] = cpu_total(s) / 1e6
            elif line.startswith('{'):
                res['json'].append(json.loads(line))
        if idle_until is not None and now >= idle_until:
            idle_until = None
            if phase == 'idle1':
                res['idle_after_cold'] = snapshot(s)
                res['warm_cpu0'] = cpu_total(s) / 1e6
                res['warm_t0'] = time.monotonic()
                phase = 'warm'
            else:
                res['idle_after_warm'] = snapshot(s)
                res['idle_cpu_ms_per_s'] = (cpu_total(s) / 1e6 - res['idle2_cpu0']) / IDLE_WARM
                phase = 'end'
            p.stdin.write('\n')
            p.stdin.flush()
        pid, status, rusage = os.wait4(p.pid, os.WNOHANG)
        if pid:
            p.returncode = status
            break
        time.sleep(0.001)
    time.sleep(0.05)  # the rest of the output
    while True:
        try:
            line = q.get_nowait()
        except queue.Empty:
            break
        if line and line.startswith('{'):
            res['json'].append(json.loads(line))
    res['exit'] = status
    res['total_cpu_ms_wait4'] = (rusage.ru_utime + rusage.ru_stime) * 1000
    res['root_maxrss'] = rusage.ru_maxrss
    res['total_wall_ms'] = (time.monotonic() - t0) * 1000
    res['samples'] = nsamples
    # The peaks of each process over its life: RSS as sampled, phys_footprint as the kernel kept it.
    res['peaks'] = {f'{pid}:{pid_names.get(pid, "?")}': [rss_max[pid], life_max[pid]] for pid in life_max}
    res['load'] = os.getloadavg()
    return res


def environment():
    env = dict(os.environ)
    # WeasyPrint finds Homebrew's Pango only through this; macOS drops it when it starts /bin/sh,
    # so the parallel mode sets it again inside the shell.
    env.setdefault('DYLD_FALLBACK_LIBRARY_PATH', '/opt/homebrew/lib')
    return env


def quote(arg):
    return "'" + arg.replace("'", "'\\''") + "'"


if __name__ == '__main__':
    mode, label = sys.argv[1], sys.argv[2]
    env = environment()
    if mode == 'single':
        r = run(sys.argv[3:], env=env)
    else:
        workers, cmd = int(sys.argv[3]), sys.argv[4:]
        if workers > 1:
            one = ' '.join(quote(a) for a in cmd) + ' < /dev/null'
            shell = (f"export DYLD_FALLBACK_LIBRARY_PATH={quote(env['DYLD_FALLBACK_LIBRARY_PATH'])}; "
                     + ' & '.join([one] * workers) + ' & wait')
            r = run(['/bin/sh', '-c', shell], expected_cold=workers, marked=False, env=env)
        else:
            r = run(cmd, marked=False, env=env)
    r['label'] = label
    r['mode'] = mode
    with open(os.path.join(OUT, os.environ.get('RESULTS', 'results.jsonl')), 'a') as f:
        f.write(json.dumps(r) + '\n')
    print(json.dumps({k: r.get(k) for k in ('label', 'cold', 'json', 'exit')}, default=str)[:200])
