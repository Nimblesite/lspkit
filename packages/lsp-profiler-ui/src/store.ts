// Implements [PROF-UI-STATE]. All profiler UI state lives in signals here;
// screens render from them and mutate only through the actions. The host
// supplies the client and the debuggee map; the library owns the rest.

import { computed, signal } from "@preact/signals-core";
import type { ReadonlySignal, Signal } from "@preact/signals-core";
import type { LanguageClient } from "vscode-languageclient/node";
import { isRecord } from "./unknown-shape";
import type { ProfilerCapabilities, RuntimeInfo } from "./wire-generated";
import type { ProfilerHost } from "./host";
import { bindHost } from "./bind-host";
import { IDLE_PROCESS_PANEL, createProcessPanelActions } from "./processes-state";
import type { ProcessPanelActions, ProcessPanelState } from "./processes-state";
import { IDLE_PROFILER_SESSION, createProfilerActions, isCpuBusy, isMemoryBusy, isProfilerBusy } from "./profiler-state";
import type { ProfilerActions, ProfilerSession } from "./profiler-state";

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
  getDebuggeeProcessId: (sessionId: string) => number | undefined;
  /** Whether `runtime` reads its own heap, so any attachable process can be memory-tracked. */
  memoryIsDirect: (runtime: string) => boolean;
  /** False once the server advertised its runtimes and none tracks memory, so no memory action is offered. */
  memoryOffered: () => boolean;
  /** False once the server advertised its runtimes and `runtime` is not one with a memory backend. */
  tracksMemory: (runtime: string) => boolean;
  /** The runtime's advertised `display_name`, or its wire id when the server named none. */
  runtimeName: (runtime: string) => string;
  /** Whether `runtime`'s process entries are source files the editor can open. */
  sourceEntries: (runtime: string) => boolean;
  /** Return every signal to idle (deactivation, test teardown). */
  reset: () => void;
}

/** The `experimental.lspProfiler` block of the server's initialize result, when present. */
function advertisedCapabilities(client: LanguageClient | undefined): ProfilerCapabilities | undefined {
  const experimental: unknown = client?.initializeResult?.capabilities.experimental;
  const block = isRecord(experimental) ? experimental.lspProfiler : undefined;
  return isProfilerCapabilities(block) ? block : undefined;
}

/** A capabilities block: a runtime list, each runtime naming its id ([PROF-UI-CAPABILITIES]). */
function isProfilerCapabilities(block: unknown): block is ProfilerCapabilities {
  return isRecord(block) && Array.isArray(block.runtimes)
    && block.runtimes.every((runtime) => isRecord(runtime) && typeof runtime.id === "string");
}

type RuntimeQueries = Pick<ProfilerStore, "memoryIsDirect" | "memoryOffered" | "tracksMemory" | "runtimeName" | "sourceEntries">;

/** What the advertised runtimes can do; before the server advertises, nothing is ruled out. */
function runtimeQueries(capabilities: ReadonlySignal<ProfilerCapabilities | undefined>): RuntimeQueries {
  const find = (runtime: string): RuntimeInfo | undefined => capabilities.value?.runtimes.find((info) => info.id === runtime);
  return {
    memoryIsDirect: (runtime) => find(runtime)?.memory?.mode.kind === "Direct",
    memoryOffered: () => capabilities.value?.runtimes.some((info) => info.memory !== undefined) ?? true,
    tracksMemory: (runtime) => capabilities.value === undefined || find(runtime)?.memory !== undefined,
    runtimeName: (runtime) => find(runtime)?.display_name ?? runtime,
    sourceEntries: (runtime) => find(runtime)?.source_entries === true,
  };
}

/** Create the profiler's reactive state bound to `host`. Factory, never a singleton. */
export function createProfilerStore(host: ProfilerHost): ProfilerStore {
  bindHost(host);
  const profiler: Signal<ProfilerSession> = signal(IDLE_PROFILER_SESSION);
  const processes: Signal<ProcessPanelState> = signal(IDLE_PROCESS_PANEL);
  const capabilities = computed(() => advertisedCapabilities(host.client.value));
  return {
    capabilities,
    ...runtimeQueries(capabilities),
    host,
    client: host.client,
    sessionIdToPid: host.sessionIdToPid,
    getDebuggeeProcessId: (sessionId) => host.sessionIdToPid.value.get(sessionId),
    ...sessionState(profiler),
    processes,
    processesRevision: computed(() => processes.value.revision),
    ...createProcessPanelActions(processes),
    reset() { profiler.value = IDLE_PROFILER_SESSION; processes.value = IDLE_PROCESS_PANEL; },
  };
}

/** The session signal, what it says about each leg, and the only way to change it. */
function sessionState(profiler: Signal<ProfilerSession>):
  Pick<ProfilerStore, "profiler" | "profilerBusy" | "cpuBusy" | "memoryBusy"> & ReturnType<typeof createProfilerActions> {
  return {
    profiler,
    profilerBusy: computed(() => isProfilerBusy(profiler.value)),
    cpuBusy: computed(() => isCpuBusy(profiler.value)),
    memoryBusy: computed(() => isMemoryBusy(profiler.value)),
    ...createProfilerActions(profiler),
  };
}
