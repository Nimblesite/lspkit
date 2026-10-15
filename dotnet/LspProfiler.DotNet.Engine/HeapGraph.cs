// The object graph of one heap snapshot and its on-disk form.
// Implements [PROF-DOTNET-HEAP-GRAPH].
//
// Nodes are objects, edges are references in declaration order, roots are the
// runtime's own root reports. The file is the snapshot artifact the language
// server keeps between steps, so `refs` and `objects` never touch the target
// again.
using System.Text.Json;
using System.Text.Json.Serialization;

namespace LspProfiler.DotNet.Engine;

/// <summary>A type seen in the snapshot and the assembly it came from.</summary>
internal sealed record HeapType(string Name, string? Module);

/// <summary>A root pointing at one object: the root's kind and a human label.</summary>
internal sealed record HeapRoot(int Node, string Kind, string Label);

/// <summary>Serialized graph: parallel arrays keep the file small and the load fast.</summary>
internal sealed record HeapGraphFile(
    IReadOnlyList<HeapType> Types,
    IReadOnlyList<long> Addresses,
    IReadOnlyList<long> Sizes,
    IReadOnlyList<int> TypeIds,
    IReadOnlyList<int> EdgeStarts,
    IReadOnlyList<int> EdgeCounts,
    IReadOnlyList<int> EdgeTargets,
    IReadOnlyList<HeapRoot> Roots,
    IReadOnlyList<long> GenerationBytes);

[JsonSourceGenerationOptions(PropertyNamingPolicy = JsonKnownNamingPolicy.SnakeCaseLower)]
[JsonSerializable(typeof(HeapGraphFile))]
internal sealed partial class HeapGraphJson : JsonSerializerContext;

internal sealed class HeapGraph(HeapGraphFile file)
{
    public IReadOnlyList<HeapType> Types => file.Types;

    public IReadOnlyList<HeapRoot> Roots => file.Roots;

    public IReadOnlyList<long> GenerationBytes => file.GenerationBytes;

    public int NodeCount => file.Addresses.Count;

    public long HeapBytes => file.Sizes.Sum();

    public long AddressOf(int node) => file.Addresses[node];

    public long SizeOf(int node) => file.Sizes[node];

    public HeapType TypeOf(int node) => file.Types[file.TypeIds[node]];

    /// <summary>Nodes referenced by <paramref name="node"/>; unknown targets were dropped at capture.</summary>
    public IEnumerable<int> Children(int node)
    {
        var start = file.EdgeStarts[node];
        var end = start + file.EdgeCounts[node];
        for (var i = start; i < end; i++)
        {
            yield return file.EdgeTargets[i];
        }
    }

    public static HeapGraph Load(string path)
    {
        if (!File.Exists(path))
        {
            throw new FileNotFoundException($"heap graph '{path}' does not exist", path);
        }
        using var stream = File.OpenRead(path);
        var parsed = JsonSerializer.Deserialize(stream, HeapGraphJson.Default.HeapGraphFile)
            ?? throw new InvalidDataException($"heap graph '{path}' is empty");
        return new HeapGraph(parsed);
    }

    public void Save(string path)
    {
        using var stream = File.Create(path);
        JsonSerializer.Serialize(stream, file, HeapGraphJson.Default.HeapGraphFile);
    }
}
