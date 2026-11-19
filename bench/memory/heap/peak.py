# What the heap holds at its peak, from the profile `fagus-heap peak` writes, summed by the code of
# Fagus (or krilla) that allocated it, largest first.
#
#   python3 peak.py dhat-heap.json [DEPTH] [ROWS]
#
# DEPTH frames of Fagus name each row (1 by default), ROWS rows are printed (30 by default).
import collections, json, re, sys

profile = json.load(open(sys.argv[1]))
depth = int(sys.argv[2]) if len(sys.argv) > 2 else 1
rows = int(sys.argv[3]) if len(sys.argv) > 3 else 30
frames, points = profile['ftbl'], profile['pps']
MB = 2**20


def ours(index):
    """The frame as `function @file:line` if it is code of Fagus or krilla."""
    match = re.search(r': (.*) \(((fagus|krilla)[^)]*)\)$', frames[index])
    return f'{match.group(1)[:60]} @{match.group(2)}' if match else None


held, blocks = collections.Counter(), collections.Counter()
for point in points:
    if point['gb'] == 0:
        continue
    names = [name for name in map(ours, point['fs']) if name]
    key = ' <- '.join(names[:depth]) if names else '(elsewhere)'
    held[key] += point['gb']
    blocks[key] += point['gbk']
print(f"{sum(point['gb'] for point in points) / MB:.1f} MB held at the peak")
for key, size in held.most_common(rows):
    print(f'{size / MB:7.2f} MB {blocks[key]:7d} blocks  {key}')
