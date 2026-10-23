// Implements [PROF-UI-STATE]. All profiler UI state lives in signals here;
// screens render from them and mutate only through the actions. The host
// supplies the client and the debuggee map; the library owns the rest.

import * as vscode from "vscode";
import { configurePalette, resolvePalette, DEFAULT_PALETTE } from "./palette";
import { computed, signal, type ReadonlySignal, type Signal } from "@preact/signals-core";
import type { LanguageClient } from "vscode-languageclient/node";
import { isRecord } from "./unknown-shape";
import type { ProfilerCapabilities } from "./wire-generated";
import type { ProfilerHost } from "./host";
import { installIds } from "./ids";
import { bindLogger } from "./logger";
import {
  createProcessPanelActions,
  IDLE_PROCESS_PANEL,
  type ProcessPanelActions,
  type ProcessPanelState,
} from "./processes-state";
import {
  createProfilerActions,
  IDLE_PROFILER_SESSION,
  isCpuBusy,
  isMemoryBusy,
  isProfilerBusy,
  type ProfilerActions,
  type ProfilerSession,
} from "./profiler-state";

export interface ProfilerStore extends ProfilerActions, ProcessPanelActions {
  readonly host: ProfilerHost;
  readonly client: ReadonlySignal<LanguageClient | undefined>;
  readonly sessionIdToPid: ReadonlySignal<ReadonlyMap<string, number>>;
  readonly profiler: ReadonlySignal<ProfilerSession>;
  readonly profilerBusy: ReadonlySignal<boolean>;
  readonly cpuBusy: ReadonlySignal<boolean>;
  readonly memoryBusy: ReadonlySignal<boolean>;
  readonly processes: ReadonlySignal<ProcessPanelState>;
  readonly processesRevision: ReadonlySignal<number>;
  /** What the server's runtimes can do ([PROF-UI-CAPABILITIES]); `undefined` until the server initialised. */
  readonly capabilities: ReadonlySignal<ProfilerCapabilities | undefined>;
  getDebuggeeProcessId(sessionId: string): number | undefined;
  /** Whether `runtime` reads its own heap, so any attachable process can be memory-tracked. */
  memoryIsDirect(runtime: string): boolean;
  /** False once the server advertised its runtimes and none tracks memory, so no memory action is offered. */
  memoryOffered(): boolean;
  /** False once the server advertised `runtime` without a memory backend. */
  tracksMemory(runtime: string): boolean;
  /** The runtime's advertised `display_name`, or its wire id when the server named none. */
  runtimeName(runtime: string): string;
  /** Whether `runtime`'s process entries are source files the editor can open. */
  sourceEntries(runtime: string): boolean;
  /** Return every signal to idle (deactivation, test teardown). */
  reset(): void;
}

/** The `experimental.lspProfiler` block of the server's initialize result, when present. */
function advertisedCapabilities(client: LanguageClient | undefined): ProfilerCapabilities | undefined {
  const experimental = client?.initializeResult?.capabilities.experimental;
  const block = isRecord(experimental) ? experimental.lspProfiler : undefined;
  return isRecord(block) && Array.isArray(block.runtimes) ? (block as unknown as ProfilerCapabilities) : undefined;
}

/** Create the profiler's reactive state bound to `host`. Factory, never a singleton. */
export function createProfilerStore(host: ProfilerHost): ProfilerStore {
  installIds(host);
  configurePalette(() => resolvePalette(host.palette ?? DEFAULT_PALETTE,
    vscode.workspace.getConfiguration(host.configSection).get("profiler.palette")));
  bindLogger(() => host.logger);
  const profiler: Signal<ProfilerSession> = signal(IDLE_PROFILER_SESSION);
  const processes: Signal<ProcessPanelState> = signal(IDLE_PROCESS_PANEL);
  const capabilities = computed(() => advertisedCapabilities(host.client.value));
  return {
    capabilities,
    memoryIsDirect: (runtime) =>
      capabilities.value?.runtimes.find((info) => info.id === runtime)?.memory?.mode.kind === "Direct",
    memoryOffered: () =>
      capabilities.value?.runtimes.some((info) => info.memory !== undefined) ?? true,
    tracksMemory: (runtime) => {
      const known = capabilities.value?.runtimes.find((info) => info.id === runtime);
      return capabilities.value === undefined || known?.memory !== undefined;
    },
    runtimeName: (runtime) =>
      capabilities.value?.runtimes.find((info) => info.id === runtime)?.display_name ?? runtime,
    sourceEntries: (runtime) =>
      capabilities.value?.runtimes.find((info) => info.id === runtime)?.source_entries === true,
    host,
    client: host.client,
    sessionIdToPid: host.sessionIdToPid,
    profiler,
    profilerBusy: computed(() => isProfilerBusy(profiler.value)),
    cpuBusy: computed(() => isCpuBusy(profiler.value)),
    memoryBusy: computed(() => isMemoryBusy(profiler.value)),
    processes,
    processesRevision: computed(() => processes.value.revision),
    getDebuggeeProcessId: (sessionId) => host.sessionIdToPid.value.get(sessionId),
    ...createProfilerActions(profiler),
    ...createProcessPanelActions(processes),
    reset() {
      profiler.value = IDLE_PROFILER_SESSION;
      processes.value = IDLE_PROCESS_PANEL;
    },
  };
}
