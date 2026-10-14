import * as assert from "assert";
import {
  AdapterResolutionRequest,
  findExecutableOnPath,
  looksLikePath,
  missingAdapterMessage,
  resolveAdapterCommand,
  resolveAdapterPath,
} from "../src/adapter-resolution";

function req(overrides: Partial<AdapterResolutionRequest>): AdapterResolutionRequest {
  return {
    commandName: "lldb-dap",
    legacyNames: ["lldb-vscode"],
    candidates: ["/opt/llvm/bin/lldb-dap"],
    platform: "linux",
    env: { PATH: "" },
    existsSync: () => false,
    ...overrides,
  };
}

suite("looksLikePath", () => {
  test("treats slashes and backslashes as paths", () => {
    assert.ok(looksLikePath("/usr/bin/lldb-dap"));
    assert.ok(looksLikePath("./lldb-dap"));
    assert.ok(looksLikePath("C:\\LLVM\\lldb-dap.exe"));
  });
  test("treats a bare command as not a path", () => {
    assert.ok(!looksLikePath("lldb-dap"));
    assert.ok(!looksLikePath("netcoredbg"));
  });
});

suite("findExecutableOnPath", () => {
  test("returns the first PATH dir that holds the command", () => {
    const found = findExecutableOnPath(
      "lldb-dap",
      { PATH: ["/a", "/b"].join(":") },
      (p) => p === "/b/lldb-dap",
    );
    assert.strictEqual(found, "/b/lldb-dap");
  });
  test("skips empty PATH segments and returns undefined when absent", () => {
    const found = findExecutableOnPath(
      "lldb-dap",
      { PATH: ":/x:" },
      () => false,
    );
    assert.strictEqual(found, undefined);
  });
  test("tolerates an unset PATH", () => {
    assert.strictEqual(findExecutableOnPath("x", {}, () => true), undefined);
  });
});

suite("resolveAdapterPath", () => {
  test("config override that is an existing path wins", () => {
    const r = req({ configValue: "/custom/lldb-dap", existsSync: (p) => p === "/custom/lldb-dap" });
    assert.strictEqual(resolveAdapterPath(r), "/custom/lldb-dap");
  });
  test("config override that is a missing path resolves to undefined", () => {
    const r = req({ configValue: "/missing/lldb-dap", existsSync: () => false });
    assert.strictEqual(resolveAdapterPath(r), undefined);
  });
  test("setting used when no config override; bare name resolved on PATH", () => {
    const r = req({
      settingValue: "lldb-dap",
      env: { PATH: "/bin" },
      existsSync: (p) => p === "/bin/lldb-dap",
    });
    assert.strictEqual(resolveAdapterPath(r), "/bin/lldb-dap");
  });
  test("falls through to PATH on the primary name", () => {
    const r = req({ env: { PATH: "/usr/bin" }, existsSync: (p) => p === "/usr/bin/lldb-dap" });
    assert.strictEqual(resolveAdapterPath(r), "/usr/bin/lldb-dap");
  });
  test("falls through to a legacy name on PATH", () => {
    const r = req({ env: { PATH: "/usr/bin" }, existsSync: (p) => p === "/usr/bin/lldb-vscode" });
    assert.strictEqual(resolveAdapterPath(r), "/usr/bin/lldb-vscode");
  });
  test("uses the platform probe when PATH misses", () => {
    const r = req({
      probe: () => "/probed/lldb-dap",
      existsSync: (p) => p === "/probed/lldb-dap",
    });
    assert.strictEqual(resolveAdapterPath(r), "/probed/lldb-dap");
  });
  test("ignores a probe result that does not exist", () => {
    const r = req({ probe: () => "/ghost", existsSync: () => false });
    assert.strictEqual(resolveAdapterPath(r), undefined);
  });
  test("swallows a throwing probe and falls through to candidates", () => {
    const r = req({
      probe: () => {
        throw new Error("xcrun missing");
      },
      candidates: ["/opt/llvm/bin/lldb-dap"],
      existsSync: (p) => p === "/opt/llvm/bin/lldb-dap",
    });
    assert.strictEqual(resolveAdapterPath(r), "/opt/llvm/bin/lldb-dap");
  });
  test("returns the first existing common candidate", () => {
    const r = req({
      candidates: ["/a/lldb-dap", "/b/lldb-dap"],
      existsSync: (p) => p === "/b/lldb-dap",
    });
    assert.strictEqual(resolveAdapterPath(r), "/b/lldb-dap");
  });
  test("returns undefined when nothing resolves", () => {
    assert.strictEqual(resolveAdapterPath(req({})), undefined);
  });
  test("tolerates an omitted legacyNames list", () => {
    const r: AdapterResolutionRequest = {
      commandName: "netcoredbg",
      candidates: ["/x/netcoredbg"],
      platform: "linux",
      env: { PATH: "" },
      existsSync: (p) => p === "/x/netcoredbg",
    };
    assert.strictEqual(resolveAdapterPath(r), "/x/netcoredbg");
  });
  test("a probe is not consulted when undefined", () => {
    const r = req({ candidates: [], existsSync: () => false });
    assert.strictEqual(resolveAdapterPath(r), undefined);
  });
});

suite("resolveAdapterCommand", () => {
  test("returns the resolved path when found", () => {
    const r = req({ configValue: "/x/lldb-dap", existsSync: (p) => p === "/x/lldb-dap" });
    assert.strictEqual(resolveAdapterCommand(r), "/x/lldb-dap");
  });
  test("falls back to the bare command name when unresolved", () => {
    assert.strictEqual(resolveAdapterCommand(req({})), "lldb-dap");
  });
});

suite("missingAdapterMessage", () => {
  test("includes the setting key, install hint, and checked sources", () => {
    const msg = missingAdapterMessage({
      adapterName: "lldb-dap",
      settingKey: "osprey.debug.lldbDapPath",
      installHint: "Install LLVM/LLDB",
      checkedSources: ["xcrun", "common LLVM paths"],
    });
    assert.ok(msg.includes("lldb-dap was not found"));
    assert.ok(msg.includes("osprey.debug.lldbDapPath"));
    assert.ok(msg.includes("Install LLVM/LLDB"));
    assert.ok(msg.includes("xcrun"));
    assert.ok(msg.includes("common LLVM paths"));
  });
  test("echoes a configured value when provided", () => {
    const msg = missingAdapterMessage({
      adapterName: "netcoredbg",
      settingKey: "sharplsp.debug.netcoredbgPath",
      installHint: "Install netcoredbg",
      configuredValue: "/missing/netcoredbg",
    });
    assert.ok(msg.includes("Configured sharplsp.debug.netcoredbgPath: /missing/netcoredbg."));
  });
  test("omits the configured clause when absent", () => {
    const msg = missingAdapterMessage({
      adapterName: "x",
      settingKey: "k",
      installHint: "h",
    });
    assert.ok(!msg.includes("Configured"));
  });
});
