// Implements [PROF-WIRE-METHODS]. See docs/specs/LSP-PROFILER-SPEC.md
/** Every profiler request is one `workspace/executeCommand` with one argument object. */

import type { LanguageClient } from "vscode-languageclient/node";

/** Run the server command `command` with `args`; a `null` reply is `undefined`. */
export async function executeServerCommand<Reply>(
  client: LanguageClient,
  command: string,
  args: object,
): Promise<Reply | undefined> {
  const reply = await client.sendRequest<Reply | null | undefined>("workspace/executeCommand", {
    command,
    arguments: [args],
  });
  return reply ?? undefined;
}
