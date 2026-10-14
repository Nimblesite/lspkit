// Pre-launch lifecycle: save dirty buffers (or reject) and run the native build
// hook before the adapter is handed the artifact. Language-neutral and fully
// injectable so it unit-tests without VS Code.

/** The dirty-buffer surface the save-or-reject step needs. */
export interface SaveableDocument {
  isDirty: boolean;
  save: () => PromiseLike<boolean>;
}

/**
 * Save a dirty document before launching. Returns true when the buffer is clean
 * (either already saved or saved successfully); on a failed save it invokes
 * `onReject` with a message and returns false so the provider aborts the launch.
 * A missing document is treated as clean.
 */
export async function saveDirtyOrReject(
  document: SaveableDocument | undefined,
  onReject: (message: string) => void,
  rejectMessage = "Save the source file before debugging.",
): Promise<boolean> {
  if (!document || !document.isDirty) {
    return true;
  }
  const saved = await document.save();
  if (!saved) {
    onReject(rejectMessage);
    return false;
  }
  return true;
}

/** Arguments handed to a pre-launch native build. */
export interface PreLaunchBuildArgs {
  /** Resolved source program path. */
  sourceProgram: string;
  /** Output path for the compiled debug artifact. */
  debugOutput: string;
  /** Working directory for the build. */
  cwd: string;
}

/** A pre-launch native build step (e.g. `osprey <src> --debug --compile`). */
export type PreLaunchBuild = (args: PreLaunchBuildArgs) => Promise<void>;

/**
 * Run the pre-launch build, mapping any failure to a single user-facing error
 * via `onError` and resolving false; a clean build resolves true. Keeps the
 * try/catch shape out of every provider.
 */
export async function runPreLaunchBuild(
  build: PreLaunchBuild,
  args: PreLaunchBuildArgs,
  onError: (message: string) => void,
): Promise<boolean> {
  try {
    await build(args);
    return true;
  } catch (error) {
    onError(error instanceof Error ? error.message : String(error));
    return false;
  }
}
