// Live-heap summaries by type. Implements [PROF-DOTNET-MEMORY] snapshot and
// objects steps.
//
// Only nodes the marker reached count, so the totals here are what the
// collector keeps, not what the runtime happened to report.
namespace LspProfiler.DotNet.Engine;

internal static class HeapSummary
{
    public static Snapshot Snapshot(string graphPath, LiveHeap live, SourceLocator locator, int maxTypes)
    {
        var tally = Tally(live);
        return new Snapshot(
            "snapshot",
            graphPath,
            tally.Values.Sum(t => t.Bytes),
            tally.Values.Sum(t => t.Count),
            live.Graph.HeapBytes,
            live.Graph.GenerationBytes,
            Rows(tally, locator, maxTypes));
    }

    public static Objects Objects(LiveHeap live, SourceLocator locator, string typeName, int limit, int summaryTypes)
    {
        var matching = live.Parents.Keys
            .Where(node => string.Equals(live.Graph.TypeOf(node).Name, typeName, StringComparison.Ordinal))
            .OrderByDescending(live.Graph.SizeOf)
            .ToList();
        return new Objects(
            "objects",
            typeName,
            matching.Count,
            matching.Sum(live.Graph.SizeOf),
            matching.Take(limit).Select(node => Describe(live, node)).ToList(),
            Summary(live, locator, matching, typeName, summaryTypes));
    }

    /// <summary>The largest types, then the requested type's own row when it is too small to rank.</summary>
    private static List<TypeRow> Summary(LiveHeap live, SourceLocator locator, List<int> matching, string typeName, int maxTypes)
    {
        var rows = Rows(Tally(live), locator, maxTypes);
        if (matching.Count == 0 || rows.Exists(row => string.Equals(row.TypeName, typeName, StringComparison.Ordinal)))
        {
            return rows;
        }
        var type = live.Graph.TypeOf(matching[0]);
        return [.. rows, Row(type, new TypeTally(matching.Count, matching.Sum(live.Graph.SizeOf)), locator.Locate(type))];
    }

    public static string Id(HeapGraph graph, int node) => $"0x{graph.AddressOf(node):x}";

    private static ObjectRecord Describe(LiveHeap live, int node) =>
        new(Id(live.Graph, node), live.Graph.TypeOf(node).Name, live.Graph.SizeOf(node), Id(live.Graph, node));

    private static Dictionary<HeapType, TypeTally> Tally(LiveHeap live)
    {
        var tally = new Dictionary<HeapType, TypeTally>();
        foreach (var node in live.Parents.Keys)
        {
            var type = live.Graph.TypeOf(node);
            var size = live.Graph.SizeOf(node);
            tally[type] = tally.TryGetValue(type, out var seen) ? seen.Add(size) : new TypeTally(1, size);
        }
        return tally;
    }

    private static List<TypeRow> Rows(Dictionary<HeapType, TypeTally> tally, SourceLocator locator, int maxTypes) =>
        tally.OrderByDescending(entry => entry.Value.Bytes)
            .ThenBy(entry => entry.Key.Name, StringComparer.Ordinal)
            .Take(maxTypes)
            .Select(entry => Row(entry.Key, entry.Value, locator.Locate(entry.Key)))
            .ToList();

    private static TypeRow Row(HeapType type, TypeTally tally, (string File, int Line)? source) =>
        new(type.Name, tally.Count, tally.Bytes, source?.File, source?.Line);

    private readonly record struct TypeTally(long Count, long Bytes)
    {
        public TypeTally Add(long size) => new(Count + 1, Bytes + size);
    }
}
