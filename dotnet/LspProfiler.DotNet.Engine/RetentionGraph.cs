// Who keeps objects of one type alive. Implements [PROF-DOTNET-MEMORY] refs step.
//
// The marker's parent links are the shortest retention path from a root to
// every live object, so the graph for a type is the union of its instances'
// paths, walked holder by holder until a root or the depth limit.
namespace LspProfiler.DotNet.Engine;

internal static class RetentionGraph
{
    /// <summary>Targets take at most this fraction of the node budget so their holders stay visible.</summary>
    private const int TargetShare = 4;

    public static Refs Build(LiveHeap live, string typeName, int maxDepth, int maxNodes)
    {
        var builder = new Builder(live, maxDepth, maxNodes);
        var targets = live.Parents.Keys
            .Where(node => string.Equals(live.Graph.TypeOf(node).Name, typeName, StringComparison.Ordinal))
            .OrderByDescending(live.Graph.SizeOf)
            .ToList();
        var shown = Math.Max(1, maxNodes / TargetShare);
        foreach (var target in targets.Take(shown))
        {
            if (!builder.AddTarget(target))
            {
                break;
            }
        }
        // Instances past the targets' share are left out: the budget cut the walk short.
        return builder.Finish(omittedTargets: targets.Count > shown);
    }

    private sealed class Builder(LiveHeap live, int maxDepth, int maxNodes)
    {
        private readonly Dictionary<int, RefNodeRecord> _nodes = [];
        private readonly HashSet<(int, int)> _seenEdges = [];
        private readonly List<RefEdgeRecord> _edges = [];
        private bool _truncated;

        /// <summary>Adds one target and its retention path; false once the node budget is spent.</summary>
        public bool AddTarget(int target)
        {
            if (!Place(target, 0, isTarget: true))
            {
                return false;
            }
            var node = target;
            for (var depth = 1; depth <= maxDepth; depth++)
            {
                var holder = live.Parents[node].Holder;
                if (holder < 0 || !Connect(holder, node, depth))
                {
                    return !_truncated;
                }
                node = holder;
            }
            return true;
        }

        public Refs Finish(bool omittedTargets) =>
            new("refs", [.. _nodes.Values], _edges, _truncated || omittedTargets);

        private bool Connect(int holder, int held, int depth)
        {
            var fresh = !_nodes.ContainsKey(holder);
            if (!Place(holder, depth, isTarget: false))
            {
                return false;
            }
            if (_seenEdges.Add((holder, held)))
            {
                _edges.Add(new RefEdgeRecord(Id(holder), Id(held), null));
            }
            return fresh;
        }

        private bool Place(int node, int depth, bool isTarget)
        {
            if (_nodes.ContainsKey(node))
            {
                return true;
            }
            if (_nodes.Count >= maxNodes)
            {
                _truncated = true;
                return false;
            }
            var link = live.Parents[node];
            var rootKind = live.RootKinds.TryGetValue(node, out var kind) ? kind : null;
            _nodes[node] = new RefNodeRecord(
                Id(node),
                live.Graph.TypeOf(node).Name,
                link.Holder < 0 ? live.LabelOf(link) : Id(node),
                live.Graph.SizeOf(node),
                depth,
                isTarget,
                link.Holder < 0,
                rootKind);
            return true;
        }

        private string Id(int node) => HeapSummary.Id(live.Graph, node);
    }
}
