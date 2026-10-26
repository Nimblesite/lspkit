// Implements [PROF-WIRE-METHODS]. See docs/specs/LSP-PROFILER-SPEC.md
/** Every profiler request is one `workspace/executeCommand` with one argument object. */

import type { LanguageClient } from "vscode-languageclient/node";

/** Run the server command `command` with `args`; a `null` reply is `undefined`. */
export async function executeServerCommand<T>(
  client: LanguageClient,
  command: string,
  args: object,
): Promise<T | undefined> {
  const reply = await client.sendRequest<T | null | undefined>("workspace/executeCommand", {
    command,
    arguments: [args],
  });
  return reply ?? undefined;
}
