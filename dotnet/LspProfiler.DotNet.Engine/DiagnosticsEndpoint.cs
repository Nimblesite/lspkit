// Whether a process advertises the .NET diagnostics IPC endpoint the engine
// attaches to. Implements [PROF-DOTNET-ENGINE-ENDPOINT]: a live target whose
// endpoint is absent is answered UnsupportedRuntime before any connection
// attempt, so a missing named pipe never blocks on a connect timeout.
namespace LspProfiler.DotNet.Engine;

/// <summary>The target PID's diagnostics endpoint, checked before attach.</summary>
internal static class DiagnosticsEndpoint
{
    /// <summary>
    /// <see langword="true"/> when <paramref name="pid"/> advertises an endpoint,
    /// <see langword="false"/> when its endpoint namespace was enumerated and held
    /// none, and <see langword="null"/> when the namespace could not be enumerated —
    /// callers then fail open and let the attach classify the real error.
    /// </summary>
    public static bool? Exists(int pid) => OperatingSystem.IsWindows()
        ? WindowsPipeExists(pid)
        : UnixSocketExists(pid);

    /// <summary>Match the target's CoreCLR named pipe: its exact name or a -suffix.</summary>
    private static bool? WindowsPipeExists(int pid)
    {
        var exact = $"dotnet-diagnostic-{pid}";
        var prefixed = exact + "-";
        try
        {
            foreach (var entry in Directory.EnumerateFileSystemEntries(@"\\.\pipe\"))
            {
                var name = Path.GetFileName(entry);
                if (name == exact || name.StartsWith(prefixed, StringComparison.Ordinal))
                {
                    return true;
                }
            }
            return false;
        }
        catch (IOException) { return null; }
        catch (UnauthorizedAccessException) { return null; }
    }

    /// <summary>Match the target's socket in the platform temp directory, then /tmp.</summary>
    private static bool? UnixSocketExists(int pid)
    {
        var prefix = $"dotnet-diagnostic-{pid}-";
        var temp = Path.GetTempPath();
        var directories = temp == "/tmp" ? new[] { temp } : new[] { temp, "/tmp" };
        var enumerable = false;
        foreach (var directory in directories)
        {
            try
            {
                var entries = Directory.EnumerateFileSystemEntries(directory);
                enumerable = true;
                foreach (var entry in entries)
                {
                    if (Path.GetFileName(entry).StartsWith(prefix, StringComparison.Ordinal))
                    {
                        return true;
                    }
                }
            }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
        }
        return enumerable ? false : null;
    }
}
