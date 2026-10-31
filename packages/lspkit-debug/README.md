# @nimblesite/lspkit-debug

Language-neutral VS Code **Debug Adapter Protocol (DAP)** glue and integration-test
harness for [LspKit](https://github.com/Nimblesite/lspkit)-based editors.

Every Nimblesite language extension (Osprey · `lldb-dap`, SharpLsp · `netcoredbg`,
Basilisk · `debugpy`) ships a VS Code debugger and was re-implementing the same
plumbing: finding the adapter binary, filling an empty F5 launch config, saving
dirty buffers before launch, and an end-to-end DAP test harness. This package is
that plumbing, written **once**. Only the genuinely language-specific bits — the
debug `type`, the adapter command, install paths, and the native build command —
stay in each extension.

Everything is **dependency-injected**: no static `import "vscode"`. The pure
runtime API (`.`) imports nothing from VS Code, and the test harness (`./testing`)
takes the VS Code surfaces it needs as parameters. So the whole package unit-tests
against fakes and runs unchanged inside a real VS Code Extension Host.

## Runtime API (`@nimblesite/lspkit-debug`)

| Export | Purpose |
|---|---|
| `resolveAdapterPath` / `resolveAdapterCommand` | config override → setting → PATH → platform probe → common install paths. |
| `missingAdapterMessage` | precise, actionable "adapter not found" text. |
| `applyLaunchConfigDefaults` / `defaultLaunchConfigForEditor` | fill a blank F5 config from the active editor (pure). |
| `defaultDebugOutputPath` | stable per-source path for the compiled debug artifact. |
| `saveDirtyOrReject` / `runPreLaunchBuild` | the pre-launch lifecycle, injectable. |
| `isProfilingLaunch` / `launchMetric` / `buildMetricLaunchConfig` / `suppressBreakpointsForProfiling` | room for CPU/memory profiling on the same launch. |
| `DapMessage` / `summarizeArgs` / `summarizeBody` | DAP wire shape + compact tracing. |
| `pollUntilResult` / `waitForCondition` / `sleep` | async polling with one options object. |
| `installUiStubs` | queue-backed `window.show*Message` stubs for tests. |

## Test harness (`@nimblesite/lspkit-debug/testing`)

A DAP client over any `{ customRequest }` session: `threads`, `stackTrace`,
`scopes`, `variables`, `evaluate`, `stepOver/In/Out`, `continueExecution`,
`readFrameVariables`, `findVariable`, `waitForStop`; assertions
`assertLocalVariable`, `assertWatch`, `assertCurrentLine`, `assertFunctionOnStack`;
and VS Code-bound `makeBreakpointTools` (incl. conditional breakpoints) /
`makeSessionWaiters`.

```ts
import {
  makeBreakpointTools, makeSessionWaiters, waitForStop,
  assertLocalVariable, assertWatch, stepOver, stepIn, continueExecution,
} from "@nimblesite/lspkit-debug/testing";
import * as vscode from "vscode";

const bps = makeBreakpointTools(
  vscode.debug,
  (file, line, condition) =>
    new vscode.SourceBreakpoint(
      new vscode.Location(vscode.Uri.file(file), new vscode.Position(line - 1, 0)),
      true, condition,
    ),
);
const waiters = makeSessionWaiters(vscode.debug);

bps.setSourceBreakpoints(program, [{ line: 7, condition: "n == 3" }]);
const session = await waiters.waitForDebugSessionStart();
const stop = await waitForStop(session);
await assertLocalVariable(session, stop.stack.stackFrames[0].id, { name: "n", matcher: /3/ });
await assertWatch(session, stop.stack.stackFrames[0].id, { expression: "n * 2", matcher: /6/ });
await stepIn(session, stop.threadId);
await continueExecution(session, stop.threadId);
```

## Build & test

```
npm install
npm run build   # tsc -> dist/
npm test        # c8 + mocha (unit tests against injected fakes)
```

MIT — © Nimblesite.
