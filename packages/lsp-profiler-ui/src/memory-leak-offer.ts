/**
 * Which suspected leak the autopilot offers, and how the offer names it.
 *
 * A runtime reports a leak's type, its source line, both or neither: a .NET
 * `byte[]` grown by user code has a type and no line of its own. The offer
 * named every leak by `file:line`, so a type-only leak read "Suspected memory
 * leak at :0". It now names what the runtime reported, and among equally
 * confident leaks it offers one the user can open at its line.
 *
 * Implements [PROF-UI-AUTOPILOT].
 */

import path from "node:path";
import type { SuspectedLeak } from "./memory-decorations";
import { confidenceRank } from "./memory-decorations";

/** Whether the runtime placed `leak` on a source line. */
export function isLocated(leak: SuspectedLeak): boolean {
  return leak.file !== "" && leak.line > 0;
}

/** The leak to offer: the most confident, and among equals one with a source line. */
export function leakToOffer(leaks: readonly SuspectedLeak[]): SuspectedLeak | undefined {
  let best: SuspectedLeak | undefined = undefined;
  for (const leak of leaks) {
    if (best === undefined || offerRank(leak) > offerRank(best)) {
      best = leak;
    }
  }
  return best;
}

/** Confidence first; a source line only breaks a tie. */
function offerRank(leak: SuspectedLeak): number {
  return confidenceRank(leak.confidence) * 2 + (isLocated(leak) ? 1 : 0);
}

/** How an offer names `leak`: its type and its source line, as far as the runtime reported them. */
export function describeLeak(leak: SuspectedLeak): string {
  const at = isLocated(leak) ? `${path.basename(leak.file)}:${String(leak.line)}` : "";
  if (leak.typeName !== "" && at !== "") { return `${leak.typeName} at ${at}`; }
  if (leak.typeName !== "") { return leak.typeName; }
  return at === "" ? "an allocation the runtime did not name" : at;
}
