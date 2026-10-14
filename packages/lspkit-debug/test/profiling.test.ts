import * as assert from "assert";
import {
  buildMetricLaunchConfig,
  isProfilingLaunch,
  launchMetric,
  suppressBreakpointsForProfiling,
} from "../src/profiling";
import { DapMessage } from "../src/dap-messages";

suite("isProfilingLaunch / launchMetric", () => {
  test("plain debug launch is not profiling", () => {
    assert.ok(!isProfilingLaunch({}));
    assert.strictEqual(launchMetric({}), "debug");
  });
  test("cpu flag selects profiling/cpu", () => {
    assert.ok(isProfilingLaunch({ profileOnLaunch: true }));
    assert.strictEqual(launchMetric({ profileOnLaunch: true }), "cpu");
  });
  test("memory flag selects profiling/memory and wins over cpu", () => {
    assert.ok(isProfilingLaunch({ memoryTrackOnLaunch: true }));
    assert.strictEqual(launchMetric({ memoryTrackOnLaunch: true }), "memory");
    assert.strictEqual(
      launchMetric({ profileOnLaunch: true, memoryTrackOnLaunch: true }),
      "memory",
    );
  });
});

suite("suppressBreakpointsForProfiling", () => {
  const setBp: DapMessage = {
    type: "request",
    command: "setBreakpoints",
    arguments: { source: { path: "/a" }, breakpoints: [{ line: 1 }] },
  };
  test("clears setBreakpoints during a profiling launch", () => {
    const out = suppressBreakpointsForProfiling(setBp, true);
    assert.deepStrictEqual(out.arguments?.breakpoints, []);
    assert.deepStrictEqual((out.arguments as { source: unknown }).source, { path: "/a" });
    // input is not mutated
    assert.strictEqual((setBp.arguments?.breakpoints as unknown[]).length, 1);
  });
  test("clears function breakpoints during profiling", () => {
    const out = suppressBreakpointsForProfiling(
      { type: "request", command: "setFunctionBreakpoints", arguments: { breakpoints: [{}] } },
      true,
    );
    assert.deepStrictEqual(out.arguments?.breakpoints, []);
  });
  test("clears exception breakpoint filters during profiling", () => {
    const out = suppressBreakpointsForProfiling(
      { type: "request", command: "setExceptionBreakpoints", arguments: { filters: ["all"] } },
      true,
    );
    assert.deepStrictEqual(out.arguments?.filters, []);
    assert.deepStrictEqual(out.arguments?.filterOptions, []);
    assert.deepStrictEqual(out.arguments?.exceptionOptions, []);
  });
  test("passes non-breakpoint requests through unchanged", () => {
    const cont: DapMessage = { type: "request", command: "continue", arguments: { threadId: 1 } };
    assert.strictEqual(suppressBreakpointsForProfiling(cont, true), cont);
  });
  test("passes everything through when not a profiling launch", () => {
    assert.strictEqual(suppressBreakpointsForProfiling(setBp, false), setBp);
  });
  test("ignores non-request messages", () => {
    const evt: DapMessage = { type: "event", event: "stopped" };
    assert.strictEqual(suppressBreakpointsForProfiling(evt, true), evt);
  });
});

suite("buildMetricLaunchConfig", () => {
  const base = { type: "osprey", program: "/a.osp" };
  test("debug metric returns a copy of the base", () => {
    const out = buildMetricLaunchConfig(base, "debug");
    assert.deepStrictEqual(out, base);
    assert.notStrictEqual(out, base);
  });
  test("memory forces stopOnEntry and the tracking flag", () => {
    const out = buildMetricLaunchConfig(base, "memory");
    assert.strictEqual(out.memoryTrackOnLaunch, true);
    assert.strictEqual(out.stopOnEntry, true);
  });
  test("cpu sets profileOnLaunch and no stopOnEntry off darwin", () => {
    const out = buildMetricLaunchConfig(base, "cpu", { platform: "linux" });
    assert.strictEqual(out.profileOnLaunch, true);
    assert.strictEqual(out.stopOnEntry, undefined);
  });
  test("cpu on darwin opts into stopOnEntry only when requested", () => {
    const off = buildMetricLaunchConfig(base, "cpu", { platform: "darwin" });
    assert.strictEqual(off.stopOnEntry, undefined);
    const on = buildMetricLaunchConfig(base, "cpu", {
      platform: "darwin",
      darwinCpuStopOnEntry: true,
    });
    assert.strictEqual(on.stopOnEntry, true);
  });
  test("defaults the platform to linux", () => {
    const out = buildMetricLaunchConfig(base, "cpu");
    assert.strictEqual(out.stopOnEntry, undefined);
  });
});
