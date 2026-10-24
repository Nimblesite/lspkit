// Implements [PROF-UI-DEBUG-LINK]. Shared watch context in CPU, memory and retention panels.
import { selectedProfileVariable } from "./debug-selection";
export const DEBUG_LINK_BODY = `<section id="debug-variable" hidden aria-label="Debugger variable">
  <strong id="debug-variable-name"></strong><span id="debug-variable-type"></span>
  <pre id="debug-variable-value"></pre><button id="debug-variable-watch">Add to Watch</button>
  <button id="debug-variable-retainers">Inspect type retainers</button>
  <small>Value captured at the selected debug pause. Type retainers describe the type, not a unique object.</small>
</section>`;
export const DEBUG_LINK_CSS = `#debug-variable { border: 1px solid var(--prof-hot); border-radius: 8px;
  padding: 14px; margin-bottom: 16px; } #debug-variable span { margin-left: 12px; opacity: .7; }
  #debug-variable pre { white-space: pre-wrap; overflow-wrap: anywhere; margin: 8px 0; }
  #debug-variable button { background: transparent; color: inherit; border: 1px solid var(--prof-hot);
    padding: 6px 10px; border-radius: 4px; margin-right: 8px; cursor: pointer; }
  #debug-variable small { display: block; margin-top: 8px; opacity: .7; }`;

/** Repaint an open panel when the host palette changes; canvases redraw on resize. */
const PALETTE_REPAINT = `function repaint(vars) {
      Object.entries(vars).forEach(([name, value]) => document.documentElement.style.setProperty(name, value));
      window.dispatchEvent(new Event('resize'));
    }`;

/** Show the selected debugger variable, or hide the section when there is none. */
const VARIABLE_RENDER = `const text = (id, value) => { document.getElementById(id).textContent = value; };
    function render(variable) {
      document.getElementById('debug-variable').hidden = !variable;
      if (!variable) return;
      text('debug-variable-name', variable.expression); text('debug-variable-type', variable.type);
      text('debug-variable-value', variable.value);
      document.getElementById('debug-variable-retainers').hidden = !variable.canInspectType;
    }`;

/** The debug-link section's script, starting from the current selection. */
export function debugLinkScript(): string {
  const initial = JSON.stringify(selectedProfileVariable.value ?? null).split("<").join("\\u003c");
  return `(function () {
    const api = () => globalThis.profilerVsCodeApi || (globalThis.profilerVsCodeApi = acquireVsCodeApi());
    ${VARIABLE_RENDER}
    ${PALETTE_REPAINT}
    render(${initial});
    window.addEventListener('message', event => {
      if (event.data.type === 'profilerVariable') render(event.data.variable);
      if (event.data.type === 'profilerPalette') repaint(event.data.vars);
    });
    document.getElementById('debug-variable-watch').onclick = () => api().postMessage({type:'profileWatch'});
    document.getElementById('debug-variable-retainers').onclick = () => api().postMessage({type:'profileRetainers'});
  })();`;
}
