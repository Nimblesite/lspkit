// The package.json contributions a host declares for the profiler UI.
// Implements [PROF-UI-IDS]: one generator, parameterised by the host's prefix,
// so a host's manifest and the library's command registrations never drift.
//
//   import { profilerContributions } from "@nimblesite/lsp-profiler-ui/scripts/contributions.mjs";
//   const { commands, views, viewsWelcome, menus, configuration, keybindings } =
//     profilerContributions({ prefix: "sharplsp", brand: "SharpLsp", container: "sharplsp-explorer", debugType: "coreclr" });
//
// A host whose runtimes have no memory backend passes `memory: false`: no
// memory command, menu, launch link or autopilot setting is contributed, so
// the editor never offers what the server cannot do ([PROF-UI-CAPABILITIES]).

/** The client commands that only a memory backend can serve. */
const MEMORY_COMMANDS = new Set(["memoryMenu", "memoryStart", "memorySnapshot", "memoryStop", "memoryDiff",
  "memoryGcCollect", "memoryReferences", "trackMemoryCurrentFile", "memoryTrackProcess"]);
/** The settings that only tune memory tracking. */
const MEMORY_SETTINGS = new Set(["autoSnapshotOnPause", "autoSnapshot", "autoSnapshotInterval"]);

/** @param {{ prefix: string, brand: string, container: string, debugType: string, restartCommand?: string, memory?: boolean }} host */
export function profilerContributions(host) {
  const all = allContributions(host);
  return host.memory === false ? withoutMemory(all, `${host.prefix}.profiler.`) : all;
}

/** `contributions` with every memory command, its menu entries and the memory settings removed. */
function withoutMemory(contributions, prefix) {
  const cpuOnly = (entry) => !MEMORY_COMMANDS.has(entry.command.slice(prefix.length));
  const menus = Object.fromEntries(Object.entries(contributions.menus)
    .map(([menu, entries]) => [menu, entries.filter(cpuOnly)])
    .filter(([, entries]) => entries.length > 0));
  const configuration = Object.fromEntries(Object.entries(contributions.configuration)
    .filter(([key]) => !MEMORY_SETTINGS.has(key.slice(prefix.length))));
  return { ...contributions, commands: contributions.commands.filter(cpuOnly), menus, configuration,
    keybindings: contributions.keybindings.filter(cpuOnly) };
}

/** Each command: its name under `<prefix>.profiler.`, its title, and its icon when it has one. */
const COMMANDS = [
  ["profileStart", "Start Profiling", "$(flame)"],
  ["profileStop", "Stop Profiling", "$(debug-stop)"],
  ["profileSnapshot", "Take Profile Snapshot", "$(device-camera)"],
  ["profileAttachToDebug", "Profile Debug Session", "$(debug-alt)"],
  ["profileShowResults", "Show Profile Results", "$(flame)"],
  ["memoryMenu", "Memory…", "$(database)"],
  ["memoryStart", "Start Memory Tracking", "$(database)"],
  ["memorySnapshot", "Take Memory Snapshot", "$(device-camera)"],
  ["memoryStop", "Stop Memory Tracking", "$(debug-stop)"],
  ["memoryDiff", "Compare Memory Snapshots", "$(diff)"],
  ["memoryGcCollect", "Force Garbage Collection", "$(trash)"],
  ["inspectVariable", "Inspect Variable in Profiler", "$(graph)"],
  ["memoryReferences", "Show Reference Graph", "$(type-hierarchy)"],
  ["refreshProcesses", "Refresh Processes", "$(refresh)"],
  ["sortProcesses", "Sort Processes", "$(sort-precedence)"],
  ["groupProcesses", "Group Processes", "$(group-by-ref-type)"],
  ["filterProcesses", "Filter Processes", "$(filter)"],
  ["profileCurrentFileCpu", "Run & Profile CPU (Current File)", "$(flame)"],
  ["trackMemoryCurrentFile", "Run & Track Memory (Current File)", "$(database)"],
  ["profileProcess", "Profile CPU", "$(flame)"],
  ["memoryTrackProcess", "Track Memory", "$(database)"],
  ["copyProcessPid", "Copy PID"],
  ["revealProcessScript", "Reveal Entry in Editor"],
  ["killProcess", "Kill Process", "$(close)"],
];

/** Each setting under `<prefix>.profiler.`. */
const SETTINGS = {
  sampleRate: { type: "number", default: 100, description: "Sampling rate in Hz for CPU profiling (1-1000)." },
  includeNative: { type: "boolean", default: false, description: "Include native frames in profile results." },
  lineThreshold: { type: "number", default: 1, description: "Minimum percentage of total samples for a line to appear as a hotspot (0.1-100)." },
  functionThreshold: { type: "number", default: 2, description: "Minimum percentage of total samples for a function to appear as a hotspot (0.1-100)." },
  maxDiagnosticsPerFile: { type: "number", default: 20, description: "Maximum number of profiling diagnostics per file." },
  showInlineHeatMap: { type: "boolean", default: true, description: "Show inline heat map decorations for hot lines after profiling." },
  profileOnLaunch: { type: "boolean", default: false, description: "Automatically start CPU profiling when a debug session launches." },
  processRefreshMs: { type: "number", default: 2000, minimum: 250, description: "Poll interval (ms) for refreshing the Processes panel while it is visible." },
  preset: {
    type: "string",
    enum: ["default", "quick", "detailed", "longRunning"],
    enumDescriptions: [
      "Use the sampleRate / includeNative settings",
      "Short burst: 10 seconds at 100 Hz, for quick hotspot checks",
      "Thorough: 60 seconds at 200 Hz, higher-fidelity data",
      "No time limit at 50 Hz, for long-running servers and batch jobs",
    ],
    default: "default",
    description: "Profiling preset that configures sample rate and duration.",
  },
  palette: { type: "object", default: {}, additionalProperties: { type: "string", pattern: "^#[0-9a-fA-F]{6}$" }, description: "Override profiler colors: critical, hot, warm, memory, memoryHot, memoryWarm, memoryCool, leak, success." },
  autoSnapshotOnPause: { type: "boolean", default: true, description: "Memory autopilot: while memory tracking is active, automatically take and compare a snapshot every time the debugger pauses." },
  autoSnapshot: { type: "boolean", default: false, description: "Memory autopilot: while memory tracking is active, automatically take a memory snapshot every few seconds even when the program never pauses." },
  autoSnapshotInterval: { type: "number", default: 30, minimum: 1, description: "Seconds between automatic memory snapshots when autoSnapshot is enabled." },
};

/** @param {{ prefix: string, brand: string, container: string, debugType: string, restartCommand?: string, memory?: boolean }} host */
function allContributions(host) {
  const id = (name) => `${host.prefix}.profiler.${name}`;
  const view = id("processes");
  return {
    commands: COMMANDS.map(([name, title, icon]) => ({ command: id(name), title, category: host.brand, ...(icon ? { icon } : {}) })),
    views: { [host.container]: [{ id: view, name: "Processes", icon: "$(pulse)" }] },
    viewsWelcome: welcomeContents(host, id, view),
    menus: { ...hiddenFromPalette(id), ...debugMenus(host, id), "view/title": viewTitleMenu(id, view), "view/item/context": rowMenu(id, view) },
    configuration: Object.fromEntries(Object.entries(SETTINGS).map(([name, setting]) => [id(name), setting])),
    keybindings: [
      { command: id("profileStart"), key: "ctrl+alt+p", mac: "cmd+alt+p" },
      { command: id("profileStop"), key: "ctrl+alt+shift+p", mac: "cmd+alt+shift+p" },
    ],
  };
}

/** The Processes view's welcome copy for each server and fetch state, with the current-file launches. */
function welcomeContents(host, id, view) {
  const launchLinks = [
    `[Run & Profile CPU (Current File)](command:${id("profileCurrentFileCpu")})`,
    ...(host.memory === false ? [] : [`[Run & Track Memory (Current File)](command:${id("trackMemoryCurrentFile")})`]),
  ].join("\n");
  const server = id("serverState");
  const running = `${server} == running`;
  const restart = host.restartCommand ? `\n[Restart Server](command:${host.restartCommand})` : "";
  return [
    { view, contents: `Connecting to the ${host.brand} language server…`, when: `${server} != running && ${server} != stopped` },
    { view, contents: `The ${host.brand} language server is not running.${restart}`, when: `${server} == stopped` },
    { view, contents: `Loading processes…\n${launchLinks}`, when: `${running} && ${id("processesState")} == loading` },
    { view, contents: `Couldn't load the process list.\n${launchLinks}`, when: `${running} && ${id("processesState")} == error` },
    { view, contents: `No processes running.\n${launchLinks}`, when: `${running} && ${id("processesState")} == loaded` },
  ];
}

/** Row actions need a row, so the palette never offers them. */
function hiddenFromPalette(id) {
  const rowOnly = ["profileProcess", "memoryTrackProcess", "copyProcessPid", "revealProcessScript", "killProcess"];
  return { commandPalette: rowOnly.map((name) => ({ command: id(name), when: "false" })) };
}

/** The debugger's variable and watch actions, and the memory toolbar while tracking. */
function debugMenus(host, id) {
  const debugging = `debugType == ${host.debugType}`;
  const tracking = `${debugging} && ${id("memoryTracking")}`;
  const inspect = [{ command: id("inspectVariable"), when: debugging, group: "inline@10" }];
  return {
    "debug/variables/context": inspect,
    "debug/watch/context": inspect,
    "debug/toolBar": [
      { command: id("memorySnapshot"), when: tracking, group: "navigation@1" },
      { command: id("memoryDiff"), when: tracking, group: "navigation@2" },
      { command: id("memoryStop"), when: tracking, group: "navigation@3" },
    ],
  };
}

/** The Processes view's title bar: launches while idle, session controls while busy, then ordering. */
function viewTitleMenu(id, view) {
  const inView = `view == ${view}`;
  const tracking = `${inView} && ${id("memoryTracking")}`;
  return [
    { command: id("profileCurrentFileCpu"), when: `${inView} && !${id("cpuBusy")}`, group: "navigation@1" },
    { command: id("trackMemoryCurrentFile"), when: `${inView} && !${id("memoryBusy")}`, group: "navigation@2" },
    { command: id("profileStop"), when: `${inView} && ${id("profiling")}`, group: "navigation@1" },
    { command: id("memorySnapshot"), when: tracking, group: "navigation@1" },
    { command: id("memoryDiff"), when: tracking, group: "navigation@2" },
    { command: id("memoryStop"), when: tracking, group: "navigation@3" },
    { command: id("refreshProcesses"), when: inView, group: "navigation@3" },
    { command: id("sortProcesses"), when: inView, group: "1_order@1" },
    { command: id("groupProcesses"), when: inView, group: "1_order@2" },
    { command: id("filterProcesses"), when: inView, group: "1_order@3" },
  ];
}

/** A row's inline buttons and context menu, gated by its contextValue. */
function rowMenu(id, view) {
  const profiling = `view == ${view} && viewItem == processProfiling`;
  const profilable = `view == ${view} && viewItem =~ /^process/ && !${id("cpuBusy")}`;
  const trackable = `view == ${view} && viewItem == processDebuggee && !${id("memoryBusy")}`;
  const anyRow = `view == ${view} && viewItem =~ /^(process|blockedProcess)/`;
  return [
    { command: id("profileStop"), when: profiling, group: "inline@0" },
    { command: id("profileProcess"), when: profilable, group: "inline@1" },
    { command: id("memoryTrackProcess"), when: trackable, group: "inline@2" },
    { command: id("profileStop"), when: profiling, group: "1_profile@0" },
    { command: id("profileProcess"), when: profilable, group: "1_profile@1" },
    { command: id("memoryTrackProcess"), when: trackable, group: "1_profile@2" },
    { command: id("copyProcessPid"), when: anyRow, group: "9_copy@1" },
    { command: id("revealProcessScript"), when: anyRow, group: "9_copy@2" },
    { command: id("killProcess"), when: anyRow, group: "9_danger@1" },
  ];
}
