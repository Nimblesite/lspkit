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

/// <summary>Runs an event source until the watcher is done or the deadline passes, then stops the session.</summary>
internal static class SessionRun
{
    public static void Until(Microsoft.Diagnostics.NETCore.Client.EventPipeSession session, EventPipeEventSource source, GcWatch watch, TimeSpan deadline)
    {
        var processing = Task.Run(source.Process);
        var finished = watch.Done.Wait(deadline);
        Emit.Trace(finished ? "watch done; stopping session" : "deadline passed; stopping session");
        session.Stop();
        if (!processing.Wait(deadline))
        {
            throw new TimeoutException("the event stream did not end after the session was stopped");
        }
    }
}
