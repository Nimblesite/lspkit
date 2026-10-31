// Implements [PROF-UI-CPU]. Existing SVG export preview.
import { readFileSync } from "node:fs";
import { Logger } from "./logger";

const MAX_INLINE_SVG_BYTES = 4_194_304;

/**
 * Inline the LSP-exported flame graph SVG as a `data:` URI so the CSP-locked
 * webview (`img-src data:`) can render it without running its embedded script.
 * Returns undefined when there is no SVG, it cannot be read, or it is too large
 * to inline — the panel then simply omits the hero (the tables still work).
 */
export function loadFlamegraphSvgDataUri(flamegraphPath: string | undefined): string | undefined {
  if (flamegraphPath === undefined || flamegraphPath === "") {
    return undefined;
  }
  try {
    const svg = readFileSync(flamegraphPath);
    if (svg.byteLength === 0 || svg.byteLength > MAX_INLINE_SVG_BYTES) {
      return undefined;
    }
    return `data:image/svg+xml;base64,${svg.toString("base64")}`;
  } catch (error: unknown) {
    Logger.warn(
      `Flame graph SVG could not be inlined (${error instanceof Error ? error.message : String(error)})`,
    );
    return undefined;
  }
}

/** The flame graph hero markup, or an empty string when no SVG is available. */
export function flamegraphHeroHtml(svgDataUri: string | undefined): string {
  if (svgDataUri === undefined) {
    return "";
  }
  return `
  <h2>Flame Graph</h2>
  <div class="flame-hero">
    <img id="flame-svg" alt="CPU flame graph" src="${svgDataUri}">
    <button class="action-link" id="open-flame-svg" title="Open the interactive flame graph (zoom and search) in your default viewer">
      Open Interactive Flame Graph
    </button>
  </div>`;
}

