// Heap snapshot commands. Implements [PROF-DOTNET-MEMORY].
//
// `snapshot` captures the live heap over EventPipe and keeps the graph file;
// `analyze`, `refs` and `objects` re-open a graph the language server kept.
// Every command marks the live heap first so retention paths are available.
namespace LspProfiler.DotNet.Engine;

internal static class HeapCommands
{
    private const int SummaryTypes = 20;

    public static int Snapshot(int pid, string graphPath, int maxTypes, int bufferMB)
    {
        var (client, failure) = Errors.Connect(pid);
        if (client is null)
        {
            Emit.Message(failure!);
            return 1;
        }
        var graph = HeapCapture.Capture(client, bufferMB);
        graph.Save(graphPath);
        return Summarize(graphPath, graph, maxTypes);
    }

    public static int Analyze(string graphPath, int maxTypes) => Summarize(graphPath, HeapGraph.Load(graphPath), maxTypes);

    public static int References(string graphPath, string type, int maxDepth, int maxNodes)
    {
        var live = LiveHeap.Mark(HeapGraph.Load(graphPath));
        Emit.Message(RetentionGraph.Build(live, type, maxDepth, maxNodes));
        return 0;
    }

    public static int ObjectsOfType(string graphPath, string type, int limit)
    {
        using var locator = new SourceLocator();
        var live = LiveHeap.Mark(HeapGraph.Load(graphPath));
        Emit.Message(HeapSummary.Objects(live, locator, type, limit, SummaryTypes));
        return 0;
    }

    private static int Summarize(string graphPath, HeapGraph graph, int maxTypes)
    {
        using var locator = new SourceLocator();
        var live = LiveHeap.Mark(graph);
        Emit.Message(HeapSummary.Snapshot(graphPath, live, locator, maxTypes));
        return 0;
    }
}
