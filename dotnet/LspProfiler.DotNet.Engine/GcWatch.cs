// Watches one EventPipe session for a completed collection and the heap
// statistics that follow it. Shared by the gc and snapshot commands.
//
// The watcher never stops the session itself: stopping from inside an event
// callback deadlocks the stream, so it signals <see cref="Done"/> and the
// command stops the session from its own thread.
using Microsoft.Diagnostics.Tracing;
using Microsoft.Diagnostics.Tracing.Parsers.Clr;

namespace LspProfiler.DotNet.Engine;

internal sealed class GcWatch : IDisposable
{
    private static readonly TimeSpan StatsGrace = TimeSpan.FromSeconds(2);
    private int _induced = -1;
    private bool _collected;
    private Timer? _grace;

    /// <summary>Signalled once the collection and its statistics (or the grace period) are in.</summary>
    public ManualResetEventSlim Done { get; } = new();

    /// <summary>Set once the collection's heap statistics arrived.</summary>
    public GCHeapStatsTraceData? Stats { get; private set; }

    /// <summary>Whether a full collection completed while watching.</summary>
    public bool Collected => _collected;

    /// <summary>Generation sizes after the collection: gen0, gen1, gen2, large, pinned.</summary>
    public List<long> Generations() => Stats is null
        ? []
        : [Stats.GenerationSize0, Stats.GenerationSize1, Stats.GenerationSize2, Stats.GenerationSize3, Stats.GenerationSize4];

    public void Dispose()
    {
        _grace?.Dispose();
        Done.Dispose();
    }

    public void Attach(EventPipeEventSource source)
    {
        source.Clr.GCStart += OnStart;
        source.Clr.GCStop += OnStop;
        source.Clr.GCHeapStats += OnStats;
    }

    private void OnStart(GCStartTraceData start)
    {
        Emit.Trace($"gc start #{start.Count} depth {start.Depth} reason {start.Reason}");
        if (_induced < 0 && start.Depth == 2)
        {
            _induced = start.Count;
        }
    }

    private void OnStop(GCEndTraceData stop)
    {
        Emit.Trace($"gc stop #{stop.Count}");
        if (stop.Count != _induced || _collected)
        {
            return;
        }
        _collected = true;
        _grace = new Timer(_ => Done.Set(), null, StatsGrace, Timeout.InfiniteTimeSpan);
    }

    private void OnStats(GCHeapStatsTraceData stats)
    {
        Emit.Trace($"gc heap stats total {stats.TotalHeapSize}");
        if (_collected && Stats is null)
        {
            Stats = (GCHeapStatsTraceData)stats.Clone();
            Done.Set();
        }
    }
}

/// <summary>How a watched session ended.</summary>
internal enum SessionEnd
{
    /// <summary>The watcher saw what it waits for.</summary>
    Watched,

    /// <summary>The event stream ended first, so the target took the session with it.</summary>
    StreamEnded,

    /// <summary>Neither happened before the deadline.</summary>
    DeadlinePassed,
}

/// <summary>Runs an event source until the watcher is done, the stream ends or the deadline passes.</summary>
internal static class SessionRun
{
    public static SessionEnd Until(Microsoft.Diagnostics.NETCore.Client.EventPipeSession session, EventPipeEventSource source, GcWatch watch, TimeSpan deadline)
    {
        var processing = Task.Run(source.Process);
        var end = Await(watch.Done.WaitHandle, processing, session.Stop, deadline);
        Emit.Trace(processing.IsFaulted
            ? $"session ended: {end}; the stream failed: {processing.Exception?.GetBaseException().Message}"
            : $"session ended: {end}");
        return end;
    }

    /// <summary>
    /// Wait for <paramref name="done"/> or the end of the stream <paramref name="processing"/>
    /// reads, whichever comes first. A stream that ended took its session with it: the target
    /// exited, and waiting on would only sit out the deadline. Only a session still streaming
    /// is stopped, then drained.
    /// </summary>
    public static SessionEnd Await(WaitHandle done, Task processing, Action stop, TimeSpan deadline)
    {
        var end = WaitHandle.WaitAny([done, ((IAsyncResult)processing).AsyncWaitHandle], deadline) switch
        {
            0 => SessionEnd.Watched,
            1 => SessionEnd.StreamEnded,
            _ => SessionEnd.DeadlinePassed,
        };
        if (!processing.IsCompleted)
        {
            stop();
            if (!processing.Wait(deadline))
            {
                throw new TimeoutException("the event stream did not end after the session was stopped");
            }
        }
        return end;
    }

    /// <summary>Why <paramref name="collection"/> was not seen, as the command fails with it.</summary>
    public static Exception Unobserved(SessionEnd end, string collection) => end == SessionEnd.StreamEnded
        ? new InvalidOperationException($"the event stream ended before {collection} completed")
        : new TimeoutException($"the runtime did not complete {collection}");
}
