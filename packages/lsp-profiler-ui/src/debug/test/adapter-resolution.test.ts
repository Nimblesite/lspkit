import * as assert from "node:assert";
import path from "node:path";
import type { AdapterResolutionRequest } from "../adapter-resolution";
import { looksLikePath } from "../../paths";
import {
  findExecutableOnPath,
  findOnPath,
  missingAdapterMessage,
  resolveAdapterCommand,
  resolveAdapterPath,
} from "../adapter-resolution";

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
    assert.ok(looksLikePath(String.raw`C:\LLVM\lldb-dap.exe`));
  });
  test("treats a bare command as not a path", () => {
    assert.ok(!looksLikePath("lldb-dap"));
    assert.ok(!looksLikePath("netcoredbg"));
  });
});

suite("findOnPath", () => {
  test("returns the first PATH dir that holds the command", () => {
    const found = findOnPath("lldb-dap", { env: { PATH: ["/a", "/b"].join(":") }, existsSync: (candidate) => candidate === "/b/lldb-dap", platform: "linux" });
    assert.strictEqual(found, "/b/lldb-dap");
  });
  test("skips empty PATH segments and returns undefined when absent", () => {
    const found = findOnPath("lldb-dap", { env: { PATH: ":/x:" }, existsSync: () => false, platform: "linux" });
    assert.strictEqual(found, undefined);
  });
  test("tolerates an unset PATH", () => {
    assert.strictEqual(findOnPath("x", { env: {}, existsSync: () => true, platform: "linux" }), undefined);
    assert.strictEqual(findOnPath("x", { env: {}, existsSync: () => true, platform: "win32" }), undefined);
  });
  test("follows the requested platform's rules, never the host's", () => {
    const posix = findOnPath("lldb-dap", { env: { PATH: "/a:/b" }, existsSync: (candidate) => candidate === "/b/lldb-dap", platform: "darwin" });
    assert.strictEqual(posix, "/b/lldb-dap");
  });
  test("on Windows, splits on ';', reads Path in any case and adds PATHEXT", () => {
    const env = { Path: String.raw`C:\a;;C:\tools`, PATHEXT: ".COM;.EXE" };
    const found = findOnPath("netcoredbg", { env, existsSync: (candidate) => candidate === String.raw`C:\tools\netcoredbg.exe`, platform: "win32" });
    assert.strictEqual(found, String.raw`C:\tools\netcoredbg.exe`);
  });
  test("on Windows, a bare name without an extension never matches", () => {
    const found = findOnPath("netcoredbg", { env: { PATH: String.raw`C:\tools` }, existsSync: (candidate) => candidate === String.raw`C:\tools\netcoredbg`, platform: "win32" });
    assert.strictEqual(found, undefined);
  });
  test("on Windows, unset PATHEXT falls back to .COM .EXE .BAT .CMD", () => {
    const found = findOnPath("lldb-dap", { env: { PATH: String.raw`C:\llvm\bin` }, existsSync: (candidate) => candidate === String.raw`C:\llvm\bin\lldb-dap.cmd`, platform: "win32" });
    assert.strictEqual(found, String.raw`C:\llvm\bin\lldb-dap.cmd`);
  });
  test("on Windows, a command that already has an extension is used as given", () => {
    const found = findOnPath("lldb-dap.exe", { env: { PATH: String.raw`C:\llvm\bin`, PATHEXT: ".EXE" }, existsSync: (candidate) => candidate === String.raw`C:\llvm\bin\lldb-dap.exe`, platform: "win32" });
    assert.strictEqual(found, String.raw`C:\llvm\bin\lldb-dap.exe`);
  });
});

suite("findExecutableOnPath", () => {
  test("looks up with this host's separator and executable names", () => {
    const tool = process.platform === "win32" ? "tool.exe" : "tool";
    const dirs = [path.join("a", "bin"), path.join("b", "bin")];
    const found = findExecutableOnPath(
      "tool",
      { PATH: dirs.join(path.delimiter), PATHEXT: ".EXE" },
      (candidate) => candidate === path.join("b", "bin", tool),
    );
    assert.strictEqual(found, path.join("b", "bin", tool));
  });
});

suite("resolveAdapterPath on Windows", () => {
  test("a setting's bare name resolves through Path and PATHEXT", () => {
    const request = req({
      platform: "win32",
      settingValue: "lldb-dap",
      env: { Path: String.raw`C:\llvm\bin`, PATHEXT: ".EXE" },
      existsSync: (candidate) => candidate === String.raw`C:\llvm\bin\lldb-dap.exe`,
    });
    assert.strictEqual(resolveAdapterPath(request), String.raw`C:\llvm\bin\lldb-dap.exe`);
  });
});

suite("resolveAdapterPath", () => {
  test("config override that is an existing path wins", () => {
    const request = req({ configValue: "/custom/lldb-dap", existsSync: (candidate) => candidate === "/custom/lldb-dap" });
    assert.strictEqual(resolveAdapterPath(request), "/custom/lldb-dap");
  });
  test("config override that is a missing path resolves to undefined", () => {
    const request = req({ configValue: "/missing/lldb-dap", existsSync: () => false });
    assert.strictEqual(resolveAdapterPath(request), undefined);
  });
  test("setting used when no config override; bare name resolved on PATH", () => {
    const request = req({
      settingValue: "lldb-dap",
      env: { PATH: "/bin" },
      existsSync: (candidate) => candidate === "/bin/lldb-dap",
    });
    assert.strictEqual(resolveAdapterPath(request), "/bin/lldb-dap");
  });
  test("falls through to PATH on the primary name", () => {
    const request = req({ env: { PATH: "/usr/bin" }, existsSync: (candidate) => candidate === "/usr/bin/lldb-dap" });
    assert.strictEqual(resolveAdapterPath(request), "/usr/bin/lldb-dap");
  });
  test("falls through to a legacy name on PATH", () => {
    const request = req({ env: { PATH: "/usr/bin" }, existsSync: (candidate) => candidate === "/usr/bin/lldb-vscode" });
    assert.strictEqual(resolveAdapterPath(request), "/usr/bin/lldb-vscode");
  });
  test("uses the platform probe when PATH misses", () => {
    const request = req({
      probe: () => "/probed/lldb-dap",
      existsSync: (candidate) => candidate === "/probed/lldb-dap",
    });
    assert.strictEqual(resolveAdapterPath(request), "/probed/lldb-dap");
  });
  test("ignores a probe result that does not exist", () => {
    const request = req({ probe: () => "/ghost", existsSync: () => false });
    assert.strictEqual(resolveAdapterPath(request), undefined);
  });
  test("swallows a throwing probe and falls through to candidates", () => {
    const request = req({
      probe: () => {
        throw new Error("xcrun missing");
      },
      candidates: ["/opt/llvm/bin/lldb-dap"],
      existsSync: (candidate) => candidate === "/opt/llvm/bin/lldb-dap",
    });
    assert.strictEqual(resolveAdapterPath(request), "/opt/llvm/bin/lldb-dap");
  });
  test("returns the first existing common candidate", () => {
    const request = req({
      candidates: ["/a/lldb-dap", "/b/lldb-dap"],
      existsSync: (candidate) => candidate === "/b/lldb-dap",
    });
    assert.strictEqual(resolveAdapterPath(request), "/b/lldb-dap");
  });
  test("returns undefined when nothing resolves", () => {
    assert.strictEqual(resolveAdapterPath(req({})), undefined);
  });
  test("tolerates an omitted legacyNames list", () => {
    const request: AdapterResolutionRequest = {
      commandName: "netcoredbg",
      candidates: ["/x/netcoredbg"],
      platform: "linux",
      env: { PATH: "" },
      existsSync: (candidate) => candidate === "/x/netcoredbg",
    };
    assert.strictEqual(resolveAdapterPath(request), "/x/netcoredbg");
  });
  test("a probe is not consulted when undefined", () => {
    const request = req({ candidates: [], existsSync: () => false });
    assert.strictEqual(resolveAdapterPath(request), undefined);
  });
});

suite("resolveAdapterCommand", () => {
  test("returns the resolved path when found", () => {
    const request = req({ configValue: "/x/lldb-dap", existsSync: (candidate) => candidate === "/x/lldb-dap" });
    assert.strictEqual(resolveAdapterCommand(request), "/x/lldb-dap");
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
