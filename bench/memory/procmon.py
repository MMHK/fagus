# Memory and CPU time of a process tree on macOS, through libproc (proc_pid_rusage,
# proc_listchildpids). The resident size (RSS) also counts the pages of the binary and of shared
# libraries, and memory the allocator has handed back to the system (madvise) as long as the
# system has not reclaimed it; phys_footprint, what Activity Monitor shows as memory, counts
# neither.
import ctypes, ctypes.util

libc = ctypes.CDLL(ctypes.util.find_library('c'), use_errno=True)
libproc = ctypes.CDLL('/usr/lib/libproc.dylib', use_errno=True)

class RusageInfoV4(ctypes.Structure):
    _fields_ = [
        ('ri_uuid', ctypes.c_uint8 * 16),
        ('ri_user_time', ctypes.c_uint64),
        ('ri_system_time', ctypes.c_uint64),
        ('ri_pkg_idle_wkups', ctypes.c_uint64),
        ('ri_interrupt_wkups', ctypes.c_uint64),
        ('ri_pageins', ctypes.c_uint64),
        ('ri_wired_size', ctypes.c_uint64),
        ('ri_resident_size', ctypes.c_uint64),
        ('ri_phys_footprint', ctypes.c_uint64),
        ('ri_proc_start_abstime', ctypes.c_uint64),
        ('ri_proc_exit_abstime', ctypes.c_uint64),
        ('ri_child_user_time', ctypes.c_uint64),
        ('ri_child_system_time', ctypes.c_uint64),
        ('ri_child_pkg_idle_wkups', ctypes.c_uint64),
        ('ri_child_interrupt_wkups', ctypes.c_uint64),
        ('ri_child_pageins', ctypes.c_uint64),
        ('ri_child_elapsed_abstime', ctypes.c_uint64),
        ('ri_diskio_bytesread', ctypes.c_uint64),
        ('ri_diskio_byteswritten', ctypes.c_uint64),
        ('ri_cpu_time_qos_default', ctypes.c_uint64),
        ('ri_cpu_time_qos_maintenance', ctypes.c_uint64),
        ('ri_cpu_time_qos_background', ctypes.c_uint64),
        ('ri_cpu_time_qos_utility', ctypes.c_uint64),
        ('ri_cpu_time_qos_legacy', ctypes.c_uint64),
        ('ri_cpu_time_qos_user_initiated', ctypes.c_uint64),
        ('ri_cpu_time_qos_user_interactive', ctypes.c_uint64),
        ('ri_billed_system_time', ctypes.c_uint64),
        ('ri_serviced_system_time', ctypes.c_uint64),
        ('ri_logical_writes', ctypes.c_uint64),
        ('ri_lifetime_max_phys_footprint', ctypes.c_uint64),
        ('ri_instructions', ctypes.c_uint64),
        ('ri_cycles', ctypes.c_uint64),
        ('ri_billed_energy', ctypes.c_uint64),
        ('ri_serviced_energy', ctypes.c_uint64),
        ('ri_interval_max_phys_footprint', ctypes.c_uint64),
        ('ri_runnable_time', ctypes.c_uint64),
    ]

RUSAGE_INFO_V4 = 4
libproc.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
libproc.proc_listchildpids.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_int]

class Timebase(ctypes.Structure):
    _fields_ = [('numer', ctypes.c_uint32), ('denom', ctypes.c_uint32)]

_tb = Timebase()
libc.mach_timebase_info(ctypes.byref(_tb))
TICK_NS = _tb.numer / _tb.denom  # the times of rusage_info are in mach absolute time units


def rusage(pid):
    ri = RusageInfoV4()
    if libproc.proc_pid_rusage(pid, RUSAGE_INFO_V4, ctypes.byref(ri)) != 0:
        return None
    return ri


_buf = (ctypes.c_int * 4096)()


def children(pid):
    n = libproc.proc_listchildpids(pid, _buf, ctypes.sizeof(_buf))
    if n <= 0:
        return []
    # The number of pids, not of bytes.
    return [p for p in _buf[:n] if p > 0]


def tree(root):
    pids, stack = [], [root]
    while stack:
        p = stack.pop()
        pids.append(p)
        stack.extend(children(p))
    return pids


def sample(root):
    """{pid: (cpu_ns, cpu_of_reaped_children_ns, rss, footprint, lifetime_max_footprint)}."""
    out = {}
    for p in tree(root):
        ri = rusage(p)
        if ri is None:
            continue
        out[p] = (
            (ri.ri_user_time + ri.ri_system_time) * TICK_NS,
            (ri.ri_child_user_time + ri.ri_child_system_time) * TICK_NS,
            ri.ri_resident_size,
            ri.ri_phys_footprint,
            ri.ri_lifetime_max_phys_footprint,
        )
    return out
