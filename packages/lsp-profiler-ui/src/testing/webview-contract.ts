// Implements [PROF-UI-WEBVIEW] + [PROF-EXPORT-FLAMEGRAPH] + [PROF-UI-PALETTE].
// See docs/specs/LSP-PROFILER-SPEC.md
/**
 * PROFILER WEBVIEW HOST — hardening and honesty of every profiler results
 * panel (CPU results, memory dashboard, retention graph). A host runs the whole
 * contract by importing this module inside its own extension host.
 *
 * Why this contract exists: before the shared host, the memory dashboard and the
 * retention graph shipped with NO Content-Security-Policy, embedded
 * profiled-program data (allocation paths, type reprs, leak reasons) into
 * their inline <script> without escaping `<` (a hostile `</script>` payload
 * broke out of the script element), and re-registered their message handler on
 * EVERY open — with the autopilot re-rendering the dashboard on each debugger
 * pause, one row click navigated N times. These tests pin all three fixes on
 * the real builders and, for the handler, on a real live webview panel.
 */

import './webview-contract-memory';
import './webview-contract-results';
import './webview-contract-panel';
import './webview-contract-palette';
