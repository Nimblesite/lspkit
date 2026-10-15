// CPU sampling over EventPipe. Implements [PROF-DOTNET-CPU].
//
// The runtime's sample profiler walks every managed thread each millisecond.
// The session records to a temporary .nettrace until the language server asks
// it to stop (a line or EOF on stdin); then the rundown arrives, the trace is
// symbolized with source lines, and the ticks stream out. A target that exits
// first keeps what it recorded.
using System.Diagnostics.Tracing;
using Microsoft.Diagnostics.NETCore.Client;

namespace LspProfiler.DotNet.Engine;

internal static class CpuCommand
{
    /// <summary>The runtime events dotnet-trace's cpu-sampling profile enables: loader, JIT, IL maps, stacks.</summary>
    private const long RuntimeKeywords = 0x14C14FCCBD;

    private const string SampleProvider = "Microsoft-DotNETCore-SampleProfiler";
    private const string RuntimeProvider = "Microsoft-Windows-DotNETRuntime";

    public static int Run(int pid, bool includeNative)
    {
        var (client, failure) = Errors.Connect(pid);
        if (client is null)
        {
            Emit.Message(failure!);
            return 1;
        }
        var trace = Path.Combine(Path.GetTempPath(), $"lsp-profiler-dotnet-{pid}-{Guid.NewGuid():N}.nettrace");
        try
        {
            Record(client, pid, trace);
            var ticks = Symbolizer.Stream(trace, includeNative);
            Emit.Message(new Stopped("stopped", ticks));
            return 0;
        }
        finally
        {
            TempFiles.Delete(trace);
            TempFiles.Delete(Path.ChangeExtension(trace, ".etlx"));
        }
    }

    private static void Record(DiagnosticsClient client, int pid, string trace)
    {
        EventPipeProvider[] providers =
        [
            new(SampleProvider, EventLevel.Informational),
            new(RuntimeProvider, EventLevel.Informational, RuntimeKeywords),
        ];
        using var session = client.StartEventPipeSession(providers, requestRundown: true, circularBufferMB: 256);
        using (var file = File.Create(trace))
        {
            var copy = session.EventStream.CopyToAsync(file);
            // The public diagnostics client exposes no process-info command; the version stays unknown.
            Emit.Message(new Attached("attached", pid, null));
            _ = Console.In.ReadLine();
            StopRecording(session);
            copy.GetAwaiter().GetResult();
        }
    }

    /// <summary>
    /// End the recording. A target that exited mid-session already ended its
    /// stream, so there is nothing to stop and every sample it recorded stands.
    /// </summary>
    private static void StopRecording(EventPipeSession session)
    {
        try
        {
            session.Stop();
        }
        catch (ServerNotAvailableException)
        {
        }
    }
}

/// <summary>Best-effort removal of scratch files.</summary>
internal static class TempFiles
{
    public static void Delete(string path)
    {
        try
        {
            File.Delete(path);
        }
        catch (IOException)
        {
        }
        catch (UnauthorizedAccessException)
        {
        }
    }
}
