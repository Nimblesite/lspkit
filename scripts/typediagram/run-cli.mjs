// Implements [PROF-WIRE-GEN]. Runs the typeDiagram CLI and returns what it printed.

import { execFileSync } from "node:child_process";
import { closeSync, mkdtempSync, openSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/**
 * Run `command` and return its whole stdout as UTF-8.
 *
 * The CLI's stdout is a file, never a pipe: Node writes to a macOS pipe
 * asynchronously, and a CLI that exits straight after writing (typeDiagram
 * does) loses everything past the pipe buffer. Writes to a file are
 * synchronous on every platform, so the capture is always whole.
 * @param {string} command
 * @param {string[]} args
 */
export function captureStdout(command, args) {
  const dir = mkdtempSync(join(tmpdir(), "typediagram-"));
  const file = join(dir, "stdout");
  try {
    runInto(command, args, file);
    return readFileSync(file, "utf8");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function runInto(command, args, file) {
  const stdout = openSync(file, "w");
  // On Windows the npm shim is a `.cmd`, which only resolves through cmd.exe.
  // A shell joins arguments into one command line unquoted, so each argument
  // is quoted for it; a real executable runs directly, with Node's own quoting.
  const shell = process.platform === "win32" && !/\.(exe|com)$/i.test(command);
  try {
    const [program, argv] = shell ? [quoteForCmd(command), args.map(quoteForCmd)] : [command, args];
    execFileSync(program, argv, {
      stdio: ["ignore", stdout, "inherit"],
      shell,
    });
  } finally {
    closeSync(stdout);
  }
}

/**
 * One argument as the Windows C runtime parses it back from a command line:
 * wrapped in quotes, with embedded quotes and the backslashes before them escaped.
 * @param {string} arg
 */
export function quoteForCmd(arg) {
  const escaped = arg.replace(/(\\*)"/g, '$1$1\\"').replace(/(\\+)$/, "$1$1");
  return `"${escaped}"`;
}
