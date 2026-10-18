// Heap snapshot over EventPipe. Implements [PROF-DOTNET-HEAP-CAPTURE].
//
// Enabling the GCHeapSnapshot keywords makes the runtime perform a full
// collection and report every surviving object, every reference and every
// root as bulk events. No dump is written and no debugger permission is
// needed, which is what keeps this identical on macOS, Linux and Windows.
//
// The runtime emits the whole heap in one burst during that collection, faster
// than any reader drains it, so the session buffer must hold the burst. When it
// cannot, EventPipe drops events. Lost edges cut objects off from their roots,
// so a partial graph would report a leaking heap as nearly empty. It is refused.
using System.Diagnostics.Tracing;
using Microsoft.Diagnostics.NETCore.Client;
using Microsoft.Diagnostics.Tracing;
using Microsoft.Diagnostics.Tracing.Parsers;
using Microsoft.Diagnostics.Tracing.Parsers.Clr;

namespace LspProfiler.DotNet.Engine;

internal static class HeapCapture
{
    private const string RuntimeProvider = "Microsoft-Windows-DotNETRuntime";
    private static readonly TimeSpan Deadline = TimeSpan.FromSeconds(90);

    /// <summary>The session buffer. 256 MB lost events past about six million objects; this holds about fourteen million.</summary>
    public const int DefaultBufferMB = 1024;

    private const ClrTraceEventParser.Keywords Keywords =
        ClrTraceEventParser.Keywords.GC
        | ClrTraceEventParser.Keywords.Loader
        | ClrTraceEventParser.Keywords.Type
        | ClrTraceEventParser.Keywords.GCHeapDump
        | ClrTraceEventParser.Keywords.GCHeapCollect
        | ClrTraceEventParser.Keywords.GCHeapAndTypeNames;

    public static HeapGraph Capture(DiagnosticsClient client, int bufferMB)
    {
        EventPipeProvider[] providers = [new(RuntimeProvider, EventLevel.Verbose, (long)Keywords)];
        using var session = client.StartEventPipeSession(providers, requestRundown: true, circularBufferMB: bufferMB);
        using var source = new EventPipeEventSource(session.EventStream);
        using var watch = new GcWatch();
        var events = new HeapEvents();
        watch.Attach(source);
        events.Attach(source);
        Emit.Trace("session started; processing events");
        SessionRun.Until(session, source, watch, Deadline);
        Emit.Trace($"event stream ended; lost={source.EventsLost} collected={watch.Collected} nodes={events.NodeCount} edges={events.EdgeCount} roots={events.RootCount} types={events.TypeCount}");
        if (!watch.Collected)
        {
            throw new TimeoutException("the runtime did not complete the heap snapshot collection");
        }
        if (source.EventsLost > 0)
        {
            throw new InvalidOperationException(
                $"The heap is too large to capture completely: the runtime dropped {source.EventsLost} heap events because the {bufferMB} MB capture buffer filled. A partial snapshot would misreport what is live, so none was kept.");
        }
        return new HeapGraph(events.Assemble(watch.Generations()));
    }
}

/// <summary>Accumulates the bulk heap events of one snapshot in arrival order.</summary>
internal sealed class HeapEvents
{
    private readonly Dictionary<ulong, TypeRecord> _types = [];
    private readonly Dictionary<long, string> _modules = [];
    private readonly List<NodeRecord> _nodes = [];
    private readonly List<ulong> _edges = [];
    private readonly List<RootRecord> _roots = [];
    private readonly Dictionary<ulong, (ulong Type, string Field)> _statics = [];

    public int NodeCount => _nodes.Count;

    public int EdgeCount => _edges.Count;

    public int RootCount => _roots.Count;

    public int TypeCount => _types.Count;

    public void Attach(EventPipeEventSource source)
    {
        source.Clr.TypeBulkType += OnTypes;
        source.Clr.GCBulkNode += OnNodes;
        source.Clr.GCBulkEdge += OnEdges;
        source.Clr.GCBulkRootEdge += OnRoots;
        source.Clr.GCBulkRootStaticVar += OnStatics;
        source.Clr.LoaderModuleLoad += OnModule;
        new ClrRundownTraceEventParser(source).LoaderModuleDCStop += OnModule;
    }

    public HeapGraphFile Assemble(IReadOnlyList<long> generations)
    {
        using var names = new NestedTypeNames();
        return new GraphAssembler(this, names).Build(generations);
    }

    private void OnTypes(GCBulkTypeTraceData data)
    {
        for (var i = 0; i < data.Count; i++)
        {
            var value = data.Values(i);
            var parameters = Enumerable.Range(0, value.TypeParameterCount).Select(value.TypeParameterID).ToList();
            _types[value.TypeID] = new TypeRecord(value.TypeName, value.ModuleID, value.CorElementType, parameters, value.TypeNameID);
        }
    }

    private void OnNodes(GCBulkNodeTraceData data)
    {
        for (var i = 0; i < data.Count; i++)
        {
            var value = data.Values(i);
            _nodes.Add(new NodeRecord(value.Address, value.Size, value.TypeID, value.EdgeCount));
        }
    }

    private void OnEdges(GCBulkEdgeTraceData data)
    {
        for (var i = 0; i < data.Count; i++)
        {
            _edges.Add(data.Values(i).Target);
        }
    }

    private void OnRoots(GCBulkRootEdgeTraceData data)
    {
        for (var i = 0; i < data.Count; i++)
        {
            var value = data.Values(i);
            _roots.Add(new RootRecord(value.RootedNodeAddress, value.GCRootID, value.GCRootKind.ToString()));
        }
    }

    private void OnStatics(GCBulkRootStaticVarTraceData data)
    {
        for (var i = 0; i < data.Count; i++)
        {
            var value = data.Values(i);
            _statics[value.GCRootID] = (value.TypeID, value.FieldName);
            _roots.Add(new RootRecord(value.ObjectID, value.GCRootID, "Static"));
        }
    }

    private void OnModule(ModuleLoadUnloadTraceData data)
    {
        if (!string.IsNullOrEmpty(data.ModuleILPath))
        {
            _modules[data.ModuleID] = data.ModuleILPath;
        }
    }

    /// <summary>A runtime type; `Token` is its metadata token in its module.</summary>
    private sealed record TypeRecord(string Name, ulong ModuleId, byte ElementType, IReadOnlyList<ulong> Parameters, int Token);

    private readonly record struct NodeRecord(ulong Address, ulong Size, ulong TypeId, long EdgeCount);

    private readonly record struct RootRecord(ulong Object, ulong RootId, string Kind);

    /// <summary>Turns the accumulated records into the indexed graph.</summary>
    private sealed class GraphAssembler(HeapEvents events, NestedTypeNames names)
    {
        private const byte ArrayElementType = 0x1D;
        private readonly Dictionary<ulong, int> _typeIds = [];
        private readonly List<HeapType> _types = [];
        private readonly Dictionary<ulong, int> _nodeIndex = [];

        public HeapGraphFile Build(IReadOnlyList<long> generations)
        {
            for (var i = 0; i < events._nodes.Count; i++)
            {
                _nodeIndex.TryAdd(events._nodes[i].Address, i);
            }
            var (starts, counts, targets) = Edges();
            return new HeapGraphFile(
                _types,
                events._nodes.Select(n => (long)n.Address).ToList(),
                events._nodes.Select(n => (long)n.Size).ToList(),
                events._nodes.Select(n => TypeId(n.TypeId)).ToList(),
                starts,
                counts,
                targets,
                Roots(),
                generations);
        }

        private (List<int>, List<int>, List<int>) Edges()
        {
            var starts = new List<int>(events._nodes.Count);
            var counts = new List<int>(events._nodes.Count);
            var targets = new List<int>(events._edges.Count);
            var cursor = 0;
            foreach (var node in events._nodes)
            {
                starts.Add(targets.Count);
                var kept = 0;
                for (var e = 0; e < node.EdgeCount && cursor < events._edges.Count; e++, cursor++)
                {
                    if (_nodeIndex.TryGetValue(events._edges[cursor], out var target))
                    {
                        targets.Add(target);
                        kept++;
                    }
                }
                counts.Add(kept);
            }
            return (starts, counts, targets);
        }

        private List<HeapRoot> Roots()
        {
            var seen = new HashSet<(int, ulong)>();
            var roots = new List<HeapRoot>();
            foreach (var root in events._roots.OrderBy(r => r.Kind == "Static" ? 0 : 1))
            {
                if (_nodeIndex.TryGetValue(root.Object, out var node) && seen.Add((node, root.RootId)))
                {
                    roots.Add(new HeapRoot(node, root.Kind, RootLabel(root)));
                }
            }
            return roots;
        }

        /// <summary>The runtime reports a static root's field name and the value's type, not its declaring type.</summary>
        private string RootLabel(RootRecord root) =>
            events._statics.TryGetValue(root.RootId, out var field) ? $"static {SourceName(field.Field)}" : root.Kind;

        /// <summary>The field's name in source: F# keeps a module value `name` in a `name@line` field.</summary>
        private static string SourceName(string field)
        {
            var at = field.LastIndexOf('@');
            var line = at > 0 ? field.AsSpan(at + 1) : [];
            return !line.IsEmpty && !line.ContainsAnyExceptInRange('0', '9') ? field[..at] : field;
        }

        private int TypeId(ulong runtimeId)
        {
            if (_typeIds.TryGetValue(runtimeId, out var id))
            {
                return id;
            }
            id = _types.Count;
            _typeIds[runtimeId] = id;
            _types.Add(new HeapType(TypeName(runtimeId), ModuleOf(runtimeId)));
            return id;
        }

        private string? ModuleOf(ulong runtimeId) =>
            events._types.TryGetValue(runtimeId, out var type) && events._modules.TryGetValue((long)type.ModuleId, out var path) ? path : null;

        private string TypeName(ulong runtimeId)
        {
            if (!events._types.TryGetValue(runtimeId, out var type))
            {
                return $"<type 0x{runtimeId:x}>";
            }
            var parameters = type.Parameters.Select(TypeName).ToList();
            if (type.ElementType == ArrayElementType && parameters.Count == 1)
            {
                return $"{parameters[0]}[]";
            }
            var name = names.Qualify(type.Name, ModuleOf(runtimeId), type.Token);
            var alreadyClosed = name.Contains('[', StringComparison.Ordinal);
            return parameters.Count == 0 || alreadyClosed ? name : $"{name}[{string.Join(",", parameters)}]";
        }
    }
}
