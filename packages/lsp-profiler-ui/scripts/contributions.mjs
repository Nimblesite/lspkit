// The package.json contributions a host declares for the profiler UI.
// Implements [PROF-UI-IDS]: one generator, parameterised by the host's prefix,
// so a host's manifest and the library's command registrations never drift.
//
//   import { profilerContributions } from "@nimblesite/lsp-profiler-ui/scripts/contributions.mjs";
//   const { commands, views, viewsWelcome, menus, configuration, keybindings } =
//     profilerContributions({ prefix: "sharplsp", brand: "SharpLsp", container: "sharplsp-explorer", debugType: "coreclr" });

/** @param {{ prefix: string, brand: string, container: string, debugType: string, restartCommand?: string }} host */
export function profilerContributions(host) {
  const cmd = (name) => `${host.prefix}.profiler.${name}`;
  const ctx = (name) => `${host.prefix}.profiler.${name}`;
  const view = `${host.prefix}.profiler.processes`;
  const B = host.brand;
  const commands = [
    { command: cmd("profileStart"), title: "Start Profiling", category: B, icon: "$(flame)" },
    { command: cmd("profileStop"), title: "Stop Profiling", category: B, icon: "$(debug-stop)" },
    { command: cmd("profileSnapshot"), title: "Take Profile Snapshot", category: B, icon: "$(device-camera)" },
    { command: cmd("profileAttachToDebug"), title: "Profile Debug Session", category: B, icon: "$(debug-alt)" },
    { command: cmd("profileShowResults"), title: "Show Profile Results", category: B, icon: "$(flame)" },
    { command: cmd("memoryMenu"), title: "Memory…", category: B, icon: "$(database)" },
    { command: cmd("memoryStart"), title: "Start Memory Tracking", category: B, icon: "$(database)" },
    { command: cmd("memorySnapshot"), title: "Take Memory Snapshot", category: B, icon: "$(device-camera)" },
    { command: cmd("memoryStop"), title: "Stop Memory Tracking", category: B, icon: "$(debug-stop)" },
    { command: cmd("memoryDiff"), title: "Compare Memory Snapshots", category: B, icon: "$(diff)" },
    { command: cmd("memoryGcCollect"), title: "Force Garbage Collection", category: B, icon: "$(trash)" },
    { command: cmd("memoryReferences"), title: "Show Reference Graph", category: B, icon: "$(type-hierarchy)" },
    { command: cmd("refreshProcesses"), title: "Refresh Processes", category: B, icon: "$(refresh)" },
    { command: cmd("sortProcesses"), title: "Sort Processes", category: B, icon: "$(sort-precedence)" },
    { command: cmd("groupProcesses"), title: "Group Processes", category: B, icon: "$(group-by-ref-type)" },
    { command: cmd("filterProcesses"), title: "Filter Processes", category: B, icon: "$(filter)" },
    { command: cmd("profileCurrentFileCpu"), title: "Run & Profile CPU (Current File)", category: B, icon: "$(flame)" },
    { command: cmd("trackMemoryCurrentFile"), title: "Run & Track Memory (Current File)", category: B, icon: "$(database)" },
    { command: cmd("profileProcess"), title: "Profile CPU", category: B, icon: "$(flame)" },
    { command: cmd("memoryTrackProcess"), title: "Track Memory", category: B, icon: "$(database)" },
    { command: cmd("copyProcessPid"), title: "Copy PID", category: B },
    { command: cmd("revealProcessScript"), title: "Reveal Entry in Editor", category: B },
    { command: cmd("killProcess"), title: "Kill Process", category: B, icon: "$(close)" },
  ];
  const views = { [host.container]: [{ id: view, name: "Processes", icon: "$(pulse)" }] };
  const launchLinks =
    `[Run & Profile CPU (Current File)](command:${cmd("profileCurrentFileCpu")})\n` +
    `[Run & Track Memory (Current File)](command:${cmd("trackMemoryCurrentFile")})`;
  const running = `${ctx("serverState")} == running`;
  const viewsWelcome = [
    { view, contents: `Connecting to the ${B} language server…`, when: `${ctx("serverState")} != running && ${ctx("serverState")} != stopped` },
    { view, contents: `The ${B} language server is not running.${host.restartCommand ? `\n[Restart Server](command:${host.restartCommand})` : ""}`, when: `${ctx("serverState")} == stopped` },
    { view, contents: `Loading processes…\n${launchLinks}`, when: `${running} && ${ctx("processesState")} == loading` },
    { view, contents: `Couldn't load the process list.\n${launchLinks}`, when: `${running} && ${ctx("processesState")} == error` },
    { view, contents: `No processes running.\n${launchLinks}`, when: `${running} && ${ctx("processesState")} == loaded` },
  ];
  const rowRe = `viewItem =~ /^process/`;
  const menus = {
    commandPalette: [
      { command: cmd("profileProcess"), when: "false" },
      { command: cmd("memoryTrackProcess"), when: "false" },
      { command: cmd("copyProcessPid"), when: "false" },
      { command: cmd("revealProcessScript"), when: "false" },
      { command: cmd("killProcess"), when: "false" },
    ],
    "debug/toolBar": [
      { command: cmd("memorySnapshot"), when: `debugType == ${host.debugType} && ${ctx("memoryTracking")}`, group: "navigation@1" },
      { command: cmd("memoryDiff"), when: `debugType == ${host.debugType} && ${ctx("memoryTracking")}`, group: "navigation@2" },
      { command: cmd("memoryStop"), when: `debugType == ${host.debugType} && ${ctx("memoryTracking")}`, group: "navigation@3" },
    ],
    "view/title": [
      { command: cmd("profileCurrentFileCpu"), when: `view == ${view} && !${ctx("cpuBusy")}`, group: "navigation@1" },
      { command: cmd("trackMemoryCurrentFile"), when: `view == ${view} && !${ctx("memoryBusy")}`, group: "navigation@2" },
      { command: cmd("profileStop"), when: `view == ${view} && ${ctx("profiling")}`, group: "navigation@1" },
      { command: cmd("memorySnapshot"), when: `view == ${view} && ${ctx("memoryTracking")}`, group: "navigation@1" },
      { command: cmd("memoryDiff"), when: `view == ${view} && ${ctx("memoryTracking")}`, group: "navigation@2" },
      { command: cmd("memoryStop"), when: `view == ${view} && ${ctx("memoryTracking")}`, group: "navigation@3" },
      { command: cmd("refreshProcesses"), when: `view == ${view}`, group: "navigation@3" },
      { command: cmd("sortProcesses"), when: `view == ${view}`, group: "1_order@1" },
      { command: cmd("groupProcesses"), when: `view == ${view}`, group: "1_order@2" },
      { command: cmd("filterProcesses"), when: `view == ${view}`, group: "1_order@3" },
    ],
    "view/item/context": [
      { command: cmd("profileStop"), when: `view == ${view} && viewItem == processProfiling`, group: "inline@0" },
      { command: cmd("profileProcess"), when: `view == ${view} && ${rowRe} && !${ctx("cpuBusy")}`, group: "inline@1" },
      { command: cmd("memoryTrackProcess"), when: `view == ${view} && viewItem == processDebuggee && !${ctx("memoryBusy")}`, group: "inline@2" },
      { command: cmd("profileStop"), when: `view == ${view} && viewItem == processProfiling`, group: "1_profile@0" },
      { command: cmd("profileProcess"), when: `view == ${view} && ${rowRe} && !${ctx("cpuBusy")}`, group: "1_profile@1" },
      { command: cmd("memoryTrackProcess"), when: `view == ${view} && viewItem == processDebuggee && !${ctx("memoryBusy")}`, group: "1_profile@2" },
      { command: cmd("copyProcessPid"), when: `view == ${view} && viewItem =~ /^(process|blockedProcess)/`, group: "9_copy@1" },
      { command: cmd("revealProcessScript"), when: `view == ${view} && viewItem =~ /^(process|blockedProcess)/`, group: "9_copy@2" },
      { command: cmd("killProcess"), when: `view == ${view} && viewItem =~ /^(process|blockedProcess)/`, group: "9_danger@1" },
    ],
  };
  const key = (name) => `${host.prefix}.profiler.${name}`;
  const configuration = {
    [key("sampleRate")]: { type: "number", default: 100, description: "Sampling rate in Hz for CPU profiling (1-1000)." },
    [key("includeNative")]: { type: "boolean", default: false, description: "Include native frames in profile results." },
    [key("lineThreshold")]: { type: "number", default: 1, description: "Minimum percentage of total samples for a line to appear as a hotspot (0.1-100)." },
    [key("functionThreshold")]: { type: "number", default: 2, description: "Minimum percentage of total samples for a function to appear as a hotspot (0.1-100)." },
    [key("maxDiagnosticsPerFile")]: { type: "number", default: 20, description: "Maximum number of profiling diagnostics per file." },
    [key("showInlineHeatMap")]: { type: "boolean", default: true, description: "Show inline heat map decorations for hot lines after profiling." },
    [key("profileOnLaunch")]: { type: "boolean", default: false, description: "Automatically start CPU profiling when a debug session launches." },
    [key("processRefreshMs")]: { type: "number", default: 2000, minimum: 250, description: "Poll interval (ms) for refreshing the Processes panel while it is visible." },
    [key("preset")]: {
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
    [key("autoSnapshotOnPause")]: { type: "boolean", default: true, description: "Memory autopilot: while memory tracking is active, automatically take and compare a snapshot every time the debugger pauses." },
    [key("autoSnapshot")]: { type: "boolean", default: false, description: "Memory autopilot: while memory tracking is active, automatically take a memory snapshot every few seconds even when the program never pauses." },
    [key("autoSnapshotInterval")]: { type: "number", default: 30, minimum: 1, description: "Seconds between automatic memory snapshots when autoSnapshot is enabled." },
  };
  const keybindings = [
    { command: cmd("profileStart"), key: "ctrl+alt+p", mac: "cmd+alt+p" },
    { command: cmd("profileStop"), key: "ctrl+alt+shift+p", mac: "cmd+alt+shift+p" },
  ];
  return { commands, views, viewsWelcome, menus, configuration, keybindings };
}
