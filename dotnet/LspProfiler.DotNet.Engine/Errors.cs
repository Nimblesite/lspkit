// Maps attach and analysis failures to the protocol's error kinds.
// Implements [PROF-ERRORS] for the .NET engine.
using System.Diagnostics;
using Microsoft.Diagnostics.NETCore.Client;

namespace LspProfiler.DotNet.Engine;

internal static class Errors
{
    public static ErrorKind Classify(Exception error) => error switch
    {
        ServerNotAvailableException => ErrorKind.UnsupportedRuntime,
        UnauthorizedAccessException => ErrorKind.PermissionDenied,
        UnsupportedCommandException => ErrorKind.UnsupportedRuntime,
        DiagnosticsClientException => ErrorKind.AttachFailed,
        _ => ErrorKind.Failed,
    };

    /// <summary>A client for <paramref name="pid"/>, or the reason there cannot be one.</summary>
    public static (DiagnosticsClient? Client, Failure? Failure) Connect(int pid)
    {
        if (!IsRunning(pid))
        {
            return (null, new Failure("failure", ErrorKind.ProcessNotFound, $"process {pid} is not running"));
        }
        var published = DiagnosticsClient.GetPublishedProcesses().Contains(pid);
        return published
            ? (new DiagnosticsClient(pid), null)
            : (null, new Failure("failure", ErrorKind.UnsupportedRuntime, $"process {pid} exposes no .NET diagnostics endpoint"));
    }

    private static bool IsRunning(int pid)
    {
        try
        {
            using var process = Process.GetProcessById(pid);
            return !process.HasExited;
        }
        catch (ArgumentException)
        {
            return false;
        }
        catch (InvalidOperationException)
        {
            return false;
        }
    }
}
