"""[PROF-MEMORY-STOP] Stopping memory tracking must release everything the
start script installed in the debuggee: the program keeps running for days
after tracking stops.

Runs the real stop script (`src/memory/scripts/stop_tracemalloc.py`) in a
namespace prepared the way the start script prepares the debuggee's globals.
"""
import gc
import pathlib
import signal
import tracemalloc
import unittest
import weakref

STOP_SCRIPT = (
    pathlib.Path(__file__).resolve().parents[2] / "src" / "memory" / "scripts" / "stop_tracemalloc.py"
).read_text(encoding="utf-8")
CYCLES = 32


class Node:
    """One half of a reference cycle."""

    def __init__(self):
        self.peer = None


def started_namespace():
    """Globals as the start script leaves them, with collected cycles saved."""
    namespace = {}
    exec(
        "import tracemalloc, gc, atexit, signal\n"
        "def _lspprof_write_exit_snapshot():\n"
        "    pass\n"
        "def _lspprof_signal_exit(_signum, _frame):\n"
        "    pass\n"
        "_lspprof_prev_handlers = {}\n"
        "tracemalloc.start(1)\n"
        "gc.set_debug(gc.DEBUG_SAVEALL)\n"
        "atexit.register(_lspprof_write_exit_snapshot)\n"
        "for _sig in (signal.SIGTERM, signal.SIGINT):\n"
        "    _lspprof_prev_handlers[_sig] = signal.getsignal(_sig)\n"
        "    signal.signal(_sig, _lspprof_signal_exit)\n"
        "tracemalloc._lspprof_prev_snapshot = tracemalloc.take_snapshot()\n",
        namespace,
    )
    for _ in range(CYCLES):
        first, second = Node(), Node()
        first.peer, second.peer = second, first
    gc.collect()
    return namespace


class StopReleasesTracking(unittest.TestCase):
    def test_stop_releases_saved_garbage_snapshot_and_hooks(self):
        handlers = {sig: signal.getsignal(sig) for sig in (signal.SIGTERM, signal.SIGINT)}
        namespace = started_namespace()
        hook = weakref.ref(namespace["_lspprof_write_exit_snapshot"])
        self.assertGreater(len(gc.garbage), 0, "DEBUG_SAVEALL kept the collected cycles")
        exec(STOP_SCRIPT, namespace)
        self.assertFalse(tracemalloc.is_tracing(), "tracemalloc still traces")
        self.assertEqual(gc.get_debug(), 0, "gc debug flags still set")
        self.assertEqual(len(gc.garbage), 0, "collected cycles stay in gc.garbage")
        self.assertFalse(
            hasattr(tracemalloc, "_lspprof_prev_snapshot"), "the baseline snapshot is still referenced"
        )
        for sig, handler in handlers.items():
            self.assertIs(signal.getsignal(sig), handler, f"signal {sig} still routes to the profiler")
        namespace.clear()
        gc.collect()
        self.assertIsNone(hook(), "atexit still holds the exit-snapshot hook")


if __name__ == "__main__":
    unittest.main()
