// CPU sampling over EventPipe. Implements [PROF-DOTNET-CPU].
//
// The runtime's sample profiler walks every managed thread each millisecond.
// Recording runs in segments: each one is a session written to a temporary
// .nettrace, ended with a rundown so its frames can be named, and symbolized
// while the next segment already records. Ticks therefore stream after every
// segment instead of only at stop, so a mid-session snapshot has data and a
// target that exits keeps every segment it finished. The language server ends
// the recording with a line or EOF on stdin.
using System.Diagnostics.Tracing;
using Microsoft.Diagnostics.NETCore.Client;

namespace LspProfiler.DotNet.Engine;

internal static class CpuCommand
{
    /// <summary>The first segment is short so the first ticks arrive quickly.</summary>
    private static readonly TimeSpan FirstSegment = TimeSpan.FromMilliseconds(500);
    /// <summary>Segments double up to this length, bounding the rundown overhead of a long session.</summary>
    private static readonly TimeSpan LongestSegment = TimeSpan.FromSeconds(10);

    public static int Run(int pid, bool includeNative)
    {
        var (client, failure) = Errors.Connect(pid);
        if (client is null)
        {
            Emit.Message(failure!);
            return 1;
        }
        var stop = Task.Run(Console.In.ReadLine);
        var segment = CpuSegment.Start(client, pid);
        // The public diagnostics client exposes no process-info command; the version stays unknown.
        Emit.Message(new Attached("attached", pid, null));
        var symbolizer = new Symbolizer(includeNative);
        Record(client, pid, segment, stop, symbolizer);
        Emit.Message(new Stopped("stopped", symbolizer.TickCount));
        return 0;
    }

    /// <summary>Record segment after segment until stop is asked for or the target is gone.</summary>
    private static void Record(DiagnosticsClient client, int pid, CpuSegment segment, Task stop, Symbolizer symbolizer)
    {
        var length = FirstSegment;
        for (CpuSegment? current = segment; current is not null; length = Next(length))
        {
            var ended = current.WaitForEnd(stop, length);
            current.Stop();
            var next = ended ? null : CpuSegment.TryStart(client, pid);
            current.Symbolize(symbolizer);
            current = next;
        }
    }

    private static TimeSpan Next(TimeSpan length) => length * 2 < LongestSegment ? length * 2 : LongestSegment;
}

/// <summary>One EventPipe recording: a session copied to a temporary trace file.</summary>
internal sealed class CpuSegment
{
    /// <summary>The runtime events dotnet-trace's cpu-sampling profile enables: loader, JIT, IL maps, stacks.</summary>
    private const long RuntimeKeywords = 0x14C14FCCBD;

    private const string SampleProvider = "Microsoft-DotNETCore-SampleProfiler";
    private const string RuntimeProvider = "Microsoft-Windows-DotNETRuntime";

    private readonly EventPipeSession _session;
    private readonly FileStream _file;
    private readonly Task _copy;
    private readonly string _trace;

    private CpuSegment(EventPipeSession session, string trace)
    {
        _session = session;
        _trace = trace;
        _file = File.Create(trace);
        _copy = session.EventStream.CopyToAsync(_file);
    }

    /// <summary>Open a segment on <paramref name="client"/>; a failure to attach propagates.</summary>
    public static CpuSegment Start(DiagnosticsClient client, int pid)
    {
        EventPipeProvider[] providers =
        [
            new(SampleProvider, EventLevel.Informational),
            new(RuntimeProvider, EventLevel.Informational, RuntimeKeywords),
        ];
        var trace = Path.Combine(Path.GetTempPath(), $"lsp-profiler-dotnet-{pid}-{Guid.NewGuid():N}.nettrace");
        return new CpuSegment(client.StartEventPipeSession(providers, requestRundown: true, circularBufferMB: 256), trace);
    }

    /// <summary>The next segment, or null when the target has gone between segments.</summary>
    public static CpuSegment? TryStart(DiagnosticsClient client, int pid)
    {
        try
        {
            return Start(client, pid);
        }
        catch (Exception error) when (error is ServerNotAvailableException or EndOfStreamException or IOException)
        {
            return null;
        }
    }

    /// <summary>
    /// Wait out this segment. True when the recording is over: stop was asked
    /// for, or the target exited and ended the stream.
    /// </summary>
    public bool WaitForEnd(Task stop, TimeSpan length)
    {
        _ = Task.WaitAny([stop, _copy], length);
        return stop.IsCompleted || _copy.IsCompleted;
    }

    /// <summary>
    /// End the recording. A target that exited mid-segment already ended its
    /// stream, so there is nothing to stop; its rundown never came, so the
    /// frames of that last segment cannot be named.
    /// </summary>
    public void Stop()
    {
        try
        {
            _session.Stop();
        }
        catch (ServerNotAvailableException)
        {
        }
        _copy.GetAwaiter().GetResult();
        _file.Dispose();
        _session.Dispose();
    }

    /// <summary>Emit this segment's ticks, then remove its scratch files.</summary>
    public void Symbolize(Symbolizer symbolizer)
    {
        try
        {
            symbolizer.Stream(_trace);
        }
        finally
        {
            TempFiles.Delete(_trace);
            TempFiles.Delete(Path.ChangeExtension(_trace, ".etlx"));
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
