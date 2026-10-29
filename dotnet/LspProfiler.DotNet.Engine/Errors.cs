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

    /// <summary>
    /// The failure a command reports for <paramref name="error"/>. When the process it works
    /// on, <paramref name="target"/>, is gone, that is the cause whatever broke first: a stream
    /// cut off mid-read or a session that ended early ([PROF-DOTNET-HEAP-CAPTURE]).
    /// </summary>
    public static Failure Describe(Exception error, int? target) =>
        target is { } pid && !IsRunning(pid)
            ? NotRunning(pid)
            : new("failure", Classify(error), error.Message);

    /// <summary>A client for <paramref name="pid"/>, or the reason there cannot be one.</summary>
    public static (DiagnosticsClient? Client, Failure? Failure) Connect(int pid)
    {
        if (!IsRunning(pid))
        {
            return (null, NotRunning(pid));
        }
        // [PROF-DOTNET-ENGINE] Resolve only the requested endpoint. Enumerating published
        // processes can fail when an unrelated /proc entry disappears during attachment.
        // [PROF-DOTNET-ENGINE-ENDPOINT] Decide "no endpoint" before building a client: on
        // Windows a missing named pipe otherwise blocks until the client's connect timeout.
        // Endpoint and permission failures stay on the attach path, where Classify keeps
        // their classification.
        if (MissingEndpoint(pid, DiagnosticsEndpoint.Exists(pid)) is { } refusal)
        {
            return (null, refusal);
        }
        return (new DiagnosticsClient(pid), null);
    }

    /// <summary>
    /// The classified refusal when the target has no diagnostics endpoint, or
    /// <see langword="null"/> when attaching may proceed: an endpoint exists, or
    /// the endpoint namespace could not be enumerated so the attach itself must
    /// classify the real error.
    /// </summary>
    internal static Failure? MissingEndpoint(int pid, bool? endpointExists) =>
        endpointExists == false
            ? new Failure("failure", ErrorKind.UnsupportedRuntime, $"process {pid} has no .NET diagnostics endpoint")
            : null;

    private static Failure NotRunning(int pid) =>
        new("failure", ErrorKind.ProcessNotFound, $"process {pid} is not running");

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
