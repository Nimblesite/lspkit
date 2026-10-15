// Live-object marking over a heap graph. Implements [PROF-DOTNET-HEAP-MARK].
//
// The runtime only reports objects that survived the snapshot's collection,
// but marking from the roots still matters: the breadth-first parent links it
// leaves behind are the shortest retention path of every live object — the
// answer to "who keeps this alive". Static roots are claimed first so a
// `Type.field` label wins over an anonymous handle.
namespace LspProfiler.DotNet.Engine;

/// <summary>How a node was reached: its holder (-1 for a root) and the label index.</summary>
internal readonly record struct Link(int Holder, int Label);

internal sealed class LiveHeap
{
    private readonly Dictionary<string, int> _labelIds = new(StringComparer.Ordinal);

    private LiveHeap(HeapGraph graph) => Graph = graph;

    public HeapGraph Graph { get; }

    /// <summary>Every live node and how it was first reached.</summary>
    public Dictionary<int, Link> Parents { get; } = [];

    /// <summary>Root labels, indexed by <see cref="Link.Label"/>.</summary>
    public List<string> Labels { get; } = [];

    /// <summary>Root kind per root node.</summary>
    public Dictionary<int, string> RootKinds { get; } = [];

    public static LiveHeap Mark(HeapGraph graph)
    {
        var live = new LiveHeap(graph);
        var queue = new Queue<int>();
        foreach (var root in graph.Roots)
        {
            live.Claim(root.Node, -1, root.Label, queue, root.Kind);
        }
        live.Drain(queue);
        return live;
    }

    public string LabelOf(Link link) => Labels[link.Label];

    private void Drain(Queue<int> queue)
    {
        while (queue.TryDequeue(out var node))
        {
            foreach (var child in Graph.Children(node))
            {
                Claim(child, node, "", queue, null);
            }
        }
    }

    private void Claim(int node, int holder, string label, Queue<int> queue, string? rootKind)
    {
        if (Parents.ContainsKey(node))
        {
            return;
        }
        Parents[node] = new Link(holder, Intern(label));
        if (rootKind is not null)
        {
            RootKinds[node] = rootKind;
        }
        queue.Enqueue(node);
    }

    private int Intern(string label)
    {
        if (!_labelIds.TryGetValue(label, out var id))
        {
            id = Labels.Count;
            Labels.Add(label);
            _labelIds[label] = id;
        }
        return id;
    }
}
