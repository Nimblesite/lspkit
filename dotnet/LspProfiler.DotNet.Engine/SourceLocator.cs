// Type → declaration location through portable PDBs. Implements [PROF-DOTNET-SOURCE].
//
// A heap knows types, not lines. The first sequence point among a type's
// methods sits at its first member, which is where an editor should anchor a
// memory finding about the type. A type with no visible point of its own, such
// as an F# record or union, anchors where the assembly's code constructs it
// (ConstructionSites). Types from assemblies without a PDB (the framework) have
// no location and are reported by name alone, as are those whose PDB names
// documents this machine doesn't have: FSharp.Core embeds a PDB pointing at its
// build server's checkout, an anchor no editor here can open. Neither has an array: `Order[]`
// is the runtime's type, grown wherever a collection resizes, so anchoring it at
// Order's line would paint that line long after the last Order died.
//
// The same PDBs place a CPU sample that landed in code the compiler hid
// ([PROF-DOTNET-CPU-SYMBOLS]): it belongs to its method's visible line before it.
using System.Reflection.Metadata;
using System.Reflection.Metadata.Ecma335;
using System.Reflection.PortableExecutable;

namespace LspProfiler.DotNet.Engine;

internal sealed class SourceLocator : IDisposable
{
    private readonly Dictionary<string, ModuleSymbols?> _modules = new(StringComparer.Ordinal);
    private readonly Dictionary<HeapType, (string File, int Line)?> _located = [];

    public (string File, int Line)? Locate(HeapType type)
    {
        if (_located.TryGetValue(type, out var known))
        {
            return known;
        }
        var located = type.Module is { } path && File.Exists(path) && !TypeNames.IsArray(type.Name)
            ? Module(path)?.Locate(TypeNames.Definition(type.Name))
            : null;
        _located[type] = located;
        return located;
    }

    /// <summary>
    /// The line a sample at <paramref name="ilOffset"/> of method <paramref name="methodToken"/> ran:
    /// the nearest visible sequence point at or before it, else the method's first visible point.
    /// </summary>
    public (string File, int Line)? Line(string module, int methodToken, int ilOffset) =>
        File.Exists(module) ? Module(module)?.Line(methodToken, ilOffset) : null;

    public void Dispose()
    {
        foreach (var symbols in _modules.Values)
        {
            symbols?.Dispose();
        }
    }

    private ModuleSymbols? Module(string path)
    {
        if (!_modules.TryGetValue(path, out var symbols))
        {
            symbols = ModuleSymbols.Open(path);
            _modules[path] = symbols;
        }
        return symbols;
    }

    /// <summary>The metadata and PDB readers of one assembly.</summary>
    private sealed class ModuleSymbols(PEReader pe, MetadataReaderProvider pdbProvider) : IDisposable
    {
        private readonly MetadataReader _metadata = pe.GetMetadataReader();
        private readonly MetadataReader _pdb = pdbProvider.GetMetadataReader();
        private readonly Dictionary<DocumentHandle, string?> _documents = [];
        private Dictionary<TypeDefinitionHandle, (string File, int Line)>? _constructed;
        private bool? _onThisMachine;

        public static ModuleSymbols? Open(string assembly)
        {
            var pe = new PEReader(File.OpenRead(assembly));
            try
            {
                var found = pe.TryOpenAssociatedPortablePdb(assembly, OpenIfPresent, out var provider, out _);
                if (found && provider is not null)
                {
                    return new ModuleSymbols(pe, provider);
                }
            }
            catch (Exception error) when (error is BadImageFormatException or IOException or InvalidOperationException)
            {
            }
            pe.Dispose();
            return null;
        }

        public (string File, int Line)? Locate(IReadOnlyList<string> nesting) =>
            OnThisMachine && Find(nesting) is { } definition ? Declared(definition) ?? Constructed(definition) : null;

        /// <summary>An F# match's decision tree is hidden code: it belongs to the visible point before it.</summary>
        public (string File, int Line)? Line(int methodToken, int ilOffset)
        {
            var visible = MethodPoints(methodToken).Where(point => !point.IsHidden).ToList();
            var before = visible.FindLastIndex(point => point.Offset <= ilOffset);
            return visible.Count == 0 ? null : Location(visible[Math.Max(before, 0)]);
        }

        /// <summary>A method's sequence points; none for a token that names no method of this PDB.</summary>
        private IEnumerable<SequencePoint> MethodPoints(int methodToken)
        {
            var row = methodToken & 0xFFFFFF;
            return methodToken >>> 24 == (int)TableIndex.MethodDef && row > 0 && row <= _pdb.MethodDebugInformation.Count
                ? SequencePoints(MetadataTokens.MethodDefinitionHandle(row))
                : [];
        }

        /// <summary>Whether any document the PDB names is here; when none is, no IL is scanned for sites.</summary>
        private bool OnThisMachine => _onThisMachine ??= _pdb.Documents.Any(document => Document(document) is not null);

        public void Dispose()
        {
            pdbProvider.Dispose();
            pe.Dispose();
        }

        private (string File, int Line)? Declared(TypeDefinitionHandle definition) =>
            _metadata.GetTypeDefinition(definition).GetMethods()
                .SelectMany(SequencePoints)
                .Where(point => !point.IsHidden)
                .Select(Location)
                .Where(site => site is not null)
                .MinBy(site => site!.Value.Line);

        private (string File, int Line)? Constructed(TypeDefinitionHandle definition)
        {
            _constructed ??= ConstructionSites.Index(pe, _metadata, _pdb, Location);
            return _constructed.TryGetValue(definition, out var site) ? site : null;
        }

        /// <summary>A sequence point's document and line, when the document is on this machine.</summary>
        private (string File, int Line)? Location(SequencePoint point) =>
            Document(point.Document) is { } file ? (file, point.StartLine) : null;

        private string? Document(DocumentHandle handle)
        {
            if (!_documents.TryGetValue(handle, out var file))
            {
                var named = _pdb.GetString(_pdb.GetDocument(handle).Name);
                file = File.Exists(named) ? named : null;
                _documents[handle] = file;
            }
            return file;
        }

        private TypeDefinitionHandle? Find(IReadOnlyList<string> nesting)
        {
            var (space, name) = TypeNames.Split(nesting[0]);
            TypeDefinitionHandle? current = _metadata.TypeDefinitions
                .Where(h => Matches(_metadata.GetTypeDefinition(h), space, name))
                .Select(h => (TypeDefinitionHandle?)h)
                .FirstOrDefault();
            foreach (var inner in nesting.Skip(1))
            {
                current = current is { } outer ? Nested(outer, inner) : null;
            }
            return current;
        }

        private TypeDefinitionHandle? Nested(TypeDefinitionHandle outer, string name) =>
            _metadata.GetTypeDefinition(outer).GetNestedTypes()
                .Where(h => _metadata.StringComparer.Equals(_metadata.GetTypeDefinition(h).Name, name))
                .Select(h => (TypeDefinitionHandle?)h)
                .FirstOrDefault();

        private bool Matches(TypeDefinition definition, string space, string name) =>
            _metadata.StringComparer.Equals(definition.Name, name)
            && _metadata.StringComparer.Equals(definition.Namespace, space)
            && definition.GetDeclaringType().IsNil;

        private IEnumerable<SequencePoint> SequencePoints(MethodDefinitionHandle method) =>
            _pdb.GetMethodDebugInformation(method.ToDebugInformationHandle()).GetSequencePoints();

        private static Stream? OpenIfPresent(string path) => File.Exists(path) ? File.OpenRead(path) : null;
    }
}

/// <summary>Runtime type-name conventions: generic arguments in brackets, nesting with '+'.</summary>
internal static class TypeNames
{
    /// <summary>The definition's nesting chain: `Ns.Outer+Inner[T]` → [`Ns.Outer`, `Inner`].</summary>
    public static IReadOnlyList<string> Definition(string runtimeName)
    {
        var bracket = runtimeName.IndexOf('[', StringComparison.Ordinal);
        var bare = bracket < 0 ? runtimeName : runtimeName[..bracket];
        return bare.Split('+');
    }

    /// <summary>An array, `Element[]` or `Element[,]`: its last bracket holds only commas, never a type argument.</summary>
    public static bool IsArray(string runtimeName)
    {
        var open = runtimeName.LastIndexOf('[');
        return open >= 0 && runtimeName.EndsWith(']') && !runtimeName.AsSpan(open + 1, runtimeName.Length - open - 2).ContainsAnyExcept(',');
    }

    public static (string Namespace, string Name) Split(string qualified)
    {
        var dot = qualified.LastIndexOf('.');
        return dot < 0 ? ("", qualified) : (qualified[..dot], qualified[(dot + 1)..]);
    }
}
