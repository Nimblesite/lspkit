// The engine's NDJSON output: one JSON object per line on stdout, each with a
// `kind` discriminator. Implements [PROF-DOTNET-ENGINE-PROTOCOL].
using System.Text.Json;
using System.Text.Json.Serialization;

namespace LspProfiler.DotNet.Engine;

/// <summary>Why attaching failed, matching the core's attach error kinds.</summary>
internal enum ErrorKind
{
    ProcessNotFound,
    UnsupportedRuntime,
    PermissionDenied,
    AttachFailed,
    Failed,
}

/// <summary>Attached to the target; sampling has begun.</summary>
internal sealed record Attached(string Kind, int Pid, string? RuntimeVersion);

/// <summary>New entries for the frame table; IDs continue from the previous message.</summary>
internal sealed record Frames(string Kind, IReadOnlyList<FrameRecord> Items);

/// <summary>One symbolized frame.</summary>
internal sealed record FrameRecord(string Name, string? File, int? Line, string? Module);

/// <summary>A batch of sampling ticks.</summary>
internal sealed record Ticks(string Kind, IReadOnlyList<TickRecord> Items);

/// <summary>Every tick preceding a CPU snapshot request has been sent.</summary>
internal sealed record Flushed(string Kind);

/// <summary>Every thread sampled at one instant.</summary>
internal sealed record TickRecord(double WeightMs, IReadOnlyList<TickThread> Threads);

/// <summary>One thread's stack as frame IDs, leaf first.</summary>
internal sealed record TickThread(long Tid, bool Active, IReadOnlyList<int> Stack);

/// <summary>Sampling finished; every tick has been sent.</summary>
internal sealed record Stopped(string Kind, long TickCount);

/// <summary>The command failed.</summary>
internal sealed record Failure(string Kind, ErrorKind ErrorKind, string Message);

/// <summary>Live-heap summary of one snapshot; the graph file is kept for later steps.</summary>
internal sealed record Snapshot(
    string Kind,
    string GraphPath,
    long LiveBytes,
    long LiveObjects,
    long HeapBytes,
    IReadOnlyList<long> GenerationBytes,
    IReadOnlyList<TypeRow> Types);

/// <summary>Live objects of one type, with its declaration when symbols allow.</summary>
internal sealed record TypeRow(string TypeName, long Count, long SizeBytes, string? File, int? Line);

/// <summary>Who keeps objects of one type alive.</summary>
internal sealed record Refs(string Kind, IReadOnlyList<RefNodeRecord> Nodes, IReadOnlyList<RefEdgeRecord> Edges, bool Truncated);

/// <summary>One object in a retention graph.</summary>
internal sealed record RefNodeRecord(
    string Id,
    string TypeName,
    string Label,
    long SizeBytes,
    int Depth,
    bool IsTarget,
    bool IsRoot,
    string? RootKind);

/// <summary>A reference from holder to held.</summary>
internal sealed record RefEdgeRecord(string From, string To, string? Label);

/// <summary>Live objects of one type and the largest types overall.</summary>
internal sealed record Objects(
    string Kind,
    string TypeName,
    long TotalCount,
    long TotalBytes,
    IReadOnlyList<ObjectRecord> Items,
    IReadOnlyList<TypeRow> TypeSummary);

/// <summary>One live object.</summary>
internal sealed record ObjectRecord(string Id, string TypeName, long SizeBytes, string Label);

/// <summary>Result of an induced garbage collection.</summary>
internal sealed record GcDone(string Kind, bool Observed, long HeapBytesAfter, IReadOnlyList<long> GenerationBytes);

[JsonSourceGenerationOptions(
    PropertyNamingPolicy = JsonKnownNamingPolicy.SnakeCaseLower,
    UseStringEnumConverter = true,
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull)]
[JsonSerializable(typeof(Attached))]
[JsonSerializable(typeof(Frames))]
[JsonSerializable(typeof(Ticks))]
[JsonSerializable(typeof(Flushed))]
[JsonSerializable(typeof(Stopped))]
[JsonSerializable(typeof(Failure))]
[JsonSerializable(typeof(Snapshot))]
[JsonSerializable(typeof(Refs))]
[JsonSerializable(typeof(Objects))]
[JsonSerializable(typeof(GcDone))]
internal sealed partial class ProtocolJson : JsonSerializerContext;

/// <summary>Writes protocol messages to stdout, one per line.</summary>
internal static class Emit
{
    private static readonly Stream Stdout = Console.OpenStandardOutput();
    private static readonly object Gate = new();

    public static void Message<T>(T message)
    {
        var bytes = JsonSerializer.SerializeToUtf8Bytes(message, typeof(T), ProtocolJson.Default);
        lock (Gate)
        {
            Stdout.Write(bytes);
            Stdout.WriteByte((byte)'\n');
            Stdout.Flush();
        }
    }

    private static readonly bool Tracing = Environment.GetEnvironmentVariable("LSP_PROFILER_ENGINE_TRACE") is { Length: > 0 };

    /// <summary>Diagnostic line on stderr, only when LSP_PROFILER_ENGINE_TRACE is set.</summary>
    public static void Trace(string line)
    {
        if (Tracing)
        {
            Console.Error.WriteLine($"[engine] {line}");
        }
    }

    public static int Fail(ErrorKind kind, string message)
    {
        Message(new Failure("failure", kind, message));
        return 1;
    }
}
