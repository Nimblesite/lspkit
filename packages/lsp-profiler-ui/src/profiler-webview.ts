// Implements [PROF-UI-WEBVIEW]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Shared webview host for every profiler results panel (flamegraph, memory
 * dashboard, retention graph).
 *
 * All three panels share the same lifecycle needs — a singleton panel that is
 * created once and re-revealed after, a message handler that must be registered
 * exactly ONCE per panel instance, a nonce-gated CSP, and safe embedding of
 * profiled-program data into inline scripts. Before this module each panel
 * hand-rolled that stack and two of them re-registered their message handler on
 * every open (so after N snapshots one click navigated N times) and shipped no
 * CSP at all. Hosting the primitives once makes those bugs unrepresentable.
 */

import { DEBUG_LINK_BODY, DEBUG_LINK_CSS, debugLinkScript } from "./debug-link-view";
import { bindPanelPalette } from "./palette-vscode";
import { bindDebugSelection } from "./debug-selection";
import { cmd } from "./ids";
import { paletteCss } from "./palette";
import { isRecord } from "./unknown-shape";
import { randomBytes } from "node:crypto";
import * as vscode from "vscode";

// ── Safe script embedding ─────────────────────────────────────────────────

/** Entropy (bytes) for the per-render webview CSP nonce. */
const CSP_NONCE_BYTES = 16;

/**
 * Serialize a value for embedding inside an inline `<script>`. `JSON.stringify`
 * does not escape `<`, so profiled-program data containing `</script>` would
 * close the script element early; escaping `<` keeps it an opaque JS string.
 */
export function embedJson(value: unknown): string {
  return JSON.stringify(value).split("<").join("\\u003c");
}

// ── Document shell ────────────────────────────────────────────────────────

/** The parts every profiler webview document is assembled from. */
export interface WebviewDocument {
  readonly title: string;
  readonly css: string;
  readonly body: string;
  readonly script: string;
}

/**
 * Build a complete, CSP-locked webview HTML document. A fresh nonce gates the
 * (self-generated) inline script; `default-src 'none'` blocks every external
 * resource; `img-src data:` admits only inline data URIs (the embedded flame
 * graph SVG). Even if profiled-program data slipped an escape, the browser
 * refuses to run any inline script without the nonce.
 */
export function buildWebviewDocument(doc: WebviewDocument): string {
  const nonce = randomBytes(CSP_NONCE_BYTES).toString("base64");
  const csp = `default-src 'none'; img-src data:; style-src 'nonce-${nonce}'; script-src 'nonce-${nonce}';`;
  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta http-equiv="Content-Security-Policy" content="${csp}">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>${doc.title}</title>
  <style nonce="${nonce}">${doc.css}${paletteCss()}${DEBUG_LINK_CSS}</style>
</head>
<body>${DEBUG_LINK_BODY}${doc.body}
  <script nonce="${nonce}">${debugLinkScript()}${doc.script}</script>
</body>
</html>`;
}

// ── Singleton panel ───────────────────────────────────────────────────────

/** A message posted from a profiler webview back to the extension. */
export interface WebviewMessage {
  readonly type: string;
  readonly file?: string;
  readonly line?: number;
}

/** A posted value as a profiler message: an object with a string `type`, a string `file` and a numeric `line` when present. */
export function webviewMessage(raw: unknown): WebviewMessage | undefined {
  if (!isRecord(raw) || typeof raw.type !== "string") return undefined;
  const { type, file, line } = raw;
  if ((file !== undefined && typeof file !== "string") || (line !== undefined && typeof line !== "number")) return undefined;
  return { type, ...(file === undefined ? {} : { file }), ...(line === undefined ? {} : { line }) };
}

/** Lifecycle options for a singleton editor-area webview. */
export interface SingletonWebviewPanelOptions {
  readonly viewColumn?: vscode.ViewColumn;
  readonly retainContextWhenHidden?: boolean;
  readonly enableFindWidget?: boolean;
  readonly onDidReveal?: () => void;
  readonly onDidDispose?: () => void;
}

/**
 * Route the message every profiler panel shares: a click on a source location
 * opens that file beside the panel. Returns whether the message was handled so
 * panels can layer their own message types on top.
 */
export function handleSourceNavigation(msg: WebviewMessage): boolean {
  if (msg.type === "profileWatch" || msg.type === "profileRetainers") {
    void vscode.commands.executeCommand(cmd(msg.type === "profileWatch" ? "watchVariable" : "inspectVariableType"))
      .then(undefined, (error: unknown) => { void vscode.window.showErrorMessage(String(error)); });
    return true;
  }
  if (msg.type !== "navigateToSource" || msg.file === undefined || msg.line === undefined) {
    return false;
  }
  void revealProfileSource({ file: msg.file, line: msg.line }).catch(() => {
    void vscode.window.showWarningMessage("Cannot open the selected profile source.");
  });
  return true;
}

/** Open a measured source line, clamped to the current document (source may have changed). */
export async function revealProfileSource(location: { file: string; line: number; type?: string }): Promise<void> {
  if (!Number.isFinite(location.line)) throw new Error("Invalid profile source line");
  const document = await vscode.workspace.openTextDocument(location.file);
  const editor = await vscode.window.showTextDocument(document, vscode.ViewColumn.One);
  const line = Math.min(Math.max(Math.trunc(location.line) - 1, 0), document.lineCount - 1);
  const range = new vscode.Range(line, 0, line, 0);
  editor.revealRange(range, vscode.TextEditorRevealType.InCenter);
  editor.selection = new vscode.Selection(line, 0, line, 0);
}

/**
 * A create-once / reveal-after webview panel whose message handler is bound
 * exactly once per panel instance — re-opening with fresh data re-renders the
 * HTML but never stacks a second handler.
 */
export class SingletonWebviewPanel {
  private panel: vscode.WebviewPanel | undefined;
  private visible = false;

  /** `viewType` is read when the panel is created, so it follows the host prefix installed later. */
  constructor(
    private readonly viewType: () => string,
    private readonly onMessage: (msg: WebviewMessage) => void,
    private readonly options: SingletonWebviewPanelOptions = {},
  ) {}

  /** Open the panel (or reveal the existing one) and swap in the new document. */
  public show(title: string, html: string): void {
    if (this.panel !== undefined) {
      this.panel.title = title;
      this.panel.reveal(this.options.viewColumn ?? vscode.ViewColumn.Beside);
    }
    (this.panel ??= this.create(title)).webview.html = html;
  }

  private create(title: string): vscode.WebviewPanel {
    const panel = vscode.window.createWebviewPanel(this.viewType(), title, this.options.viewColumn ?? vscode.ViewColumn.Beside, {
      enableScripts: true,
      retainContextWhenHidden: this.options.retainContextWhenHidden ?? true,
      enableFindWidget: this.options.enableFindWidget ?? false,
      localResourceRoots: [],
    });
    bindDebugSelection(panel);
    bindPanelPalette(panel);
    this.visible = panel.visible;
    this.bindLifecycle(panel);
    return panel;
  }

  private bindLifecycle(panel: vscode.WebviewPanel): void {
    panel.onDidDispose(() => {
      this.panel = undefined;
      this.visible = false;
      this.options.onDidDispose?.();
    });
    // Bound once per panel instance — the whole reason this class exists.
    panel.webview.onDidReceiveMessage((raw: unknown) => {
      const msg = webviewMessage(raw);
      if (msg !== undefined) this.onMessage(msg);
    });
    panel.onDidChangeViewState((event) => {
      const becameVisible = event.webviewPanel.visible && !this.visible;
      this.visible = event.webviewPanel.visible;
      if (becameVisible) this.options.onDidReveal?.();
    });
  }

  /** Post data to the open panel; false when no panel exists. */
  public postMessage(message: unknown): Thenable<boolean> {
    return this.panel?.webview.postMessage(message) ?? Promise.resolve(false);
  }

  /** Whether the panel is currently open (e2e seam). */
  public isOpen(): boolean {
    return this.panel !== undefined;
  }

  /** The panel's view type and title, when open (e2e seam). */
  public describe(): { readonly viewType: string; readonly title: string } | undefined {
    return this.panel === undefined ? undefined : { viewType: this.panel.viewType, title: this.panel.title };
  }

  /** The HTML currently rendered, when open (e2e seam). */
  public html(): string | undefined {
    return this.panel?.webview.html;
  }

  /** Whether the live panel is already frontmost in an editor group. */
  public isVisible(): boolean {
    return this.visible;
  }

  /** Close and forget the panel (extension teardown). */
  public dispose(): void {
    this.panel?.dispose();
    this.panel = undefined;
    this.visible = false;
  }
}
