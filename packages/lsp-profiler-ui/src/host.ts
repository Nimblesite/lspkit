// Implements [PROF-UI-HOST]. The host extension injects everything that is
// its own — identity, the language client, logging, the debug adapter — and
// the library never imports a product.

import type { ReadonlySignal } from "@preact/signals-core";
import type * as vscode from "vscode";
import type { LanguageClient } from "vscode-languageclient/node";
import type { LogSink } from "./logger";
import type { ProfilerPalette } from "./palette";

/** Which metric a "run the current file" launch collects. */
export type LaunchMetric = "cpu" | "memory";

export interface ProfilerHost {
  /** The host's colours ([PROF-UI-PALETTE]); the original orange/purple when omitted. */
  readonly palette?: ProfilerPalette;
  /** Command/context/view prefix, e.g. `sharplsp`. */
  readonly idPrefix: string;
  /** Product name for messages, e.g. `SharpLsp`. */
  readonly brand: string;
  /** Settings section holding `profiler.*` keys, e.g. `sharplsp`. */
  readonly configSection: string;
  /** The host's debug adapter type; `""` when the host has none. */
  readonly debugType: string;
  /** Language IDs of files the host can "run & profile". */
  readonly languageIds: readonly string[];
  /** Those languages as a message names them to a user, e.g. `F#`, `C#`. */
  readonly languageNames: readonly string[];
  /** The live language client; `undefined` until the server starts. */
  readonly client: ReadonlySignal<LanguageClient | undefined>;
  /** Where the library logs. */
  readonly logger: LogSink;
  /** Debuggee PIDs by debug session ID, as the host learns them from DAP. */
  readonly sessionIdToPid: ReadonlySignal<ReadonlyMap<string, number>>;
  /** A launch configuration that runs `program` for profiling, or `undefined` when unsupported. */
  readonly launchConfig: (metric: LaunchMetric, program: string) => vscode.DebugConfiguration | undefined;
}
