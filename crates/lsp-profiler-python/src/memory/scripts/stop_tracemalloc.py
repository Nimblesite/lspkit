
import tracemalloc, gc, atexit, signal
tracemalloc.stop()
gc.set_debug(0)
# DEBUG_SAVEALL kept every collected object in gc.garbage; release them.
gc.garbage.clear()
if hasattr(tracemalloc, '_lspprof_prev_snapshot'):
    del tracemalloc._lspprof_prev_snapshot
_lspprof_exit_hook = globals().get('_lspprof_write_exit_snapshot')
if _lspprof_exit_hook is not None:
    atexit.unregister(_lspprof_exit_hook)
for _lspprof_sig, _lspprof_prev in globals().get('_lspprof_prev_handlers', {}).items():
    try:
        signal.signal(_lspprof_sig, _lspprof_prev)
    except (ValueError, OSError, TypeError):
        pass  # only settable on the main thread; a None handler cannot be restored
print('__LSPPROF_MEM_OK__')
