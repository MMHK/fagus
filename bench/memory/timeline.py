# The footprint and RSS of `fagus render` over the phases of one render (macOS): sampled every few
# tenths of a millisecond through proc_pid_rusage and placed against the render's --timings.
#
#   python3 timeline.py DOCUMENT [OPTIONS OF fagus render...]
#
# FAGUS names the binary (default ../../target/release/fagus).
import os, re, subprocess, sys, time

import procmon

HERE = os.path.dirname(os.path.abspath(__file__))
FAGUS = os.environ.get('FAGUS', os.path.join(HERE, '..', '..', 'target', 'release', 'fagus'))
MB = 2**20

os.makedirs(os.path.join(HERE, 'out'), exist_ok=True)
command = [FAGUS, 'render', sys.argv[1], '-o', os.path.join(HERE, 'out', 'timeline.pdf'), '--timings'] + sys.argv[2:]
start = time.perf_counter()
process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
samples = []  # (milliseconds, footprint, RSS)
while process.poll() is None:
    usage = procmon.rusage(process.pid)
    if usage is not None:
        samples.append(((time.perf_counter() - start) * 1000, usage.ri_phys_footprint / MB,
                        usage.ri_resident_size / MB))
    time.sleep(0.0003)
errors = process.stderr.read().decode()
phases = [(name, float(ms)) for name, ms in re.findall(r'(parse|style|layout|paint|pdf) ([\d.]+)', errors)]
# The phases end where the process does, less about 2 ms to write the file and exit.
at = samples[-1][0] - sum(ms for _, ms in phases) - 2
for name, ms in phases:
    window = [sample for sample in samples if at <= sample[0] < at + ms]
    if window:
        print(f'{name:7} {ms:8.1f} ms  footprint at most {max(s[1] for s in window):6.1f} MB, at the end '
              f'{window[-1][1]:6.1f} MB; RSS at most {max(s[2] for s in window):6.1f} MB')
    at += ms
print(f'whole render: footprint at most {max(s[1] for s in samples):.1f} MB, RSS {max(s[2] for s in samples):.1f} MB')
