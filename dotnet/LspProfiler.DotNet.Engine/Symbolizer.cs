// Turns each recorded segment's .nettrace into symbolized sampling ticks.
// Implements [PROF-DOTNET-CPU-SYMBOLS]. Frame IDs and the tick count carry
// across segments, so the language server sees one stream.
//
// Method names come from the JIT rundown; source lines come from the IL-to-
// native maps and the portable PDBs that sit beside the application's
// assemblies. The symbol path is the set of directories the trace's modules
// were loaded from, so lookup never leaves the machine.
using Microsoft.Diagnostics.Symbols;
using Microsoft.Diagnostics.Tracing;
using Microsoft.Diagnostics.Tracing.Etlx;
using Microsoft.Diagnostics.Tracing.EventPipe;

namespace LspProfiler.DotNet.Engine;

internal sealed class Symbolizer(bool includeNative)
{
    private const string SampleProvider = "Microsoft-DotNETCore-SampleProfiler";
    private const int TicksPerMessage = 256;
    private const double MaxTickWeightMs = 10.0;

    private readonly FrameTable _frames = new(includeNative);

    /// <summary>Ticks emitted over every segment so far.</summary>
    public long TickCount { get; private set; }

    /// <summary>Symbolize one segment's <paramref name="trace"/> and emit its ticks.</summary>
    public void Stream(string trace)
    {
        var etlx = TraceLog.CreateFromEventPipeDataFile(trace, null, new TraceLogOptions { ContinueOnError = true });
        using var log = new TraceLog(etlx);
        using var reader = new SymbolReader(TextWriter.Null, SymbolPath(log)) { SecurityCheck = _ => true };
        _frames.Begin(reader);
        var output = new TickWriter(_frames);
        foreach (var sample in log.Events.Where(e => e.ProviderName == SampleProvider))
        {
            output.Add(sample);
        }
        TickCount += output.Finish();
    }

    private static string SymbolPath(TraceLog log) =>
        string.Join(';', log.ModuleFiles
            .Select(module => Path.GetDirectoryName(module.FilePath))
            .Where(dir => !string.IsNullOrEmpty(dir) && Directory.Exists(dir))
            .Distinct(StringComparer.Ordinal));

    /// <summary>Groups per-thread samples into ticks and flushes them in batches.</summary>
    private sealed class TickWriter(FrameTable frames)
    {
        private readonly List<TickRecord> _pending = [];
        private readonly List<TickThread> _current = [];
        private readonly HashSet<int> _threadsInTick = [];
        private double _tickStart = double.NaN;
        private double _weight = 1.0;
        private long _count;

        public void Add(TraceEvent sample)
        {
            var at = sample.TimeStampRelativeMSec;
            if (!_threadsInTick.Add(sample.ThreadID) || at - _tickStart > 0.5)
            {
                Close(at);
                _ = _threadsInTick.Add(sample.ThreadID);
            }
            var stack = frames.Resolve(sample.CallStack());
            if (stack.Count > 0)
            {
                _current.Add(new TickThread(sample.ThreadID, IsManaged(sample), stack));
            }
        }

        public long Finish()
        {
            Close(double.NaN);
            Flush();
            return _count;
        }

        private void Close(double nextStart)
        {
            if (_current.Count > 0)
            {
                _pending.Add(new TickRecord(_weight, [.. _current]));
                _count++;
            }
            _current.Clear();
            _threadsInTick.Clear();
            var gap = nextStart - _tickStart;
            _weight = double.IsNaN(gap) || gap <= 0 ? 1.0 : Math.Min(gap, MaxTickWeightMs);
            _tickStart = nextStart;
            if (_pending.Count >= TicksPerMessage)
            {
                Flush();
            }
        }

        private void Flush()
        {
            var fresh = frames.TakeFresh();
            if (fresh.Count > 0)
            {
                Emit.Message(new Frames("frames", fresh));
            }
            if (_pending.Count > 0)
            {
                Emit.Message(new Ticks("ticks", [.. _pending]));
                _pending.Clear();
            }
        }

        // A thread waiting in native code (a lock, a sleep, a console read) is
        // sampled as External and is idle. TraceLog parses every sample as a
        // ClrThreadSampleTraceData, whose Type is an enum, never a boxed int.
        private static bool IsManaged(TraceEvent sample) =>
            sample is ClrThreadSampleTraceData { Type: ClrThreadSampleType.Managed };
    }
}

/// <summary>
/// Deduplicates symbolized frames and remembers which are not yet sent. IDs are
/// stable for the whole recording; code addresses only within one segment's log.
/// </summary>
internal sealed class FrameTable(bool includeNative)
{
    private readonly Dictionary<CodeAddressIndex, int?> _byAddress = [];
    private readonly Dictionary<FrameRecord, int> _ids = [];
    private readonly List<FrameRecord> _fresh = [];
    private SymbolReader? _reader;

    /// <summary>Start resolving a new segment's addresses with <paramref name="reader"/>.</summary>
    public void Begin(SymbolReader reader)
    {
        _reader = reader;
        _byAddress.Clear();
    }

    /// <summary>Frame IDs of <paramref name="stack"/>, leaf first.</summary>
    public List<int> Resolve(TraceCallStack? stack)
    {
        var ids = new List<int>();
        for (var frame = stack; frame is not null; frame = frame.Caller)
        {
            if (Lookup(frame.CodeAddress) is int id)
            {
                ids.Add(id);
            }
        }
        return ids;
    }

    public List<FrameRecord> TakeFresh()
    {
        var fresh = new List<FrameRecord>(_fresh);
        _fresh.Clear();
        return fresh;
    }

    private int? Lookup(TraceCodeAddress address)
    {
        if (_byAddress.TryGetValue(address.CodeAddressIndex, out var known))
        {
            return known;
        }
        var id = Describe(address) is FrameRecord record ? Intern(record) : (int?)null;
        _byAddress[address.CodeAddressIndex] = id;
        return id;
    }

    private int Intern(FrameRecord record)
    {
        if (_ids.TryGetValue(record, out var id))
        {
            return id;
        }
        id = _ids.Count;
        _ids[record] = id;
        _fresh.Add(record);
        return id;
    }

    private FrameRecord? Describe(TraceCodeAddress address)
    {
        var managed = address.Method is not null;
        if (!managed && !includeNative)
        {
            return null;
        }
        var name = string.IsNullOrEmpty(address.FullMethodName) ? $"{address.ModuleName}!?" : address.FullMethodName;
        var source = managed ? SourceOf(address) : null;
        var line = source?.LineNumber is > 0 and var known ? known : (int?)null;
        return new FrameRecord(name, line is null ? null : source?.SourceFile?.BuildTimeFilePath, line, NullIfEmpty(address.ModuleName));
    }

    private SourceLocation? SourceOf(TraceCodeAddress address)
    {
        try
        {
            return _reader is null ? null : address.GetSourceLine(_reader);
        }
        catch (Exception error) when (error is IOException or InvalidOperationException or BadImageFormatException)
        {
            return null;
        }
    }

    private static string? NullIfEmpty(string? text) => string.IsNullOrEmpty(text) ? null : text;
}
