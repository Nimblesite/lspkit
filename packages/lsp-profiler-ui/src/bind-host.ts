// Implements [PROF-UI-STATE]. Host binding for the profiler UI.
import * as vscode from "vscode";
import { DEFAULT_PALETTE, configurePalette, resolvePalette } from "./palette";
import type { ProfilerHost } from "./host";
import { installIds } from "./ids";
import { bindLogger } from "./logger";

/** The host palette under the user's `profiler.palette` overrides, read on every render. */
function bindHostPalette(host: ProfilerHost): void {
  configurePalette(() => resolvePalette(host.palette ?? DEFAULT_PALETTE,
    vscode.workspace.getConfiguration(host.configSection).get("profiler.palette")));
}

/** Install `host`'s ids, palette and logger for every module that reads them. */
export function bindHost(host: ProfilerHost): void {
  installIds(host);
  bindHostPalette(host);
  bindLogger(() => host.logger);
}
