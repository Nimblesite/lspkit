// Type → declaration location through portable PDBs. Implements [PROF-DOTNET-SOURCE].
//
// A heap knows types, not lines. The first sequence point among a type's
// methods sits at its first member, which is where an editor should anchor a
// memory finding about the type. A type with no visible point of its own, such
// as an F# record or union, anchors where the assembly's code constructs it
// (ConstructionSites). Types from assemblies without a PDB (the framework) have
// no location and are reported by name alone. Neither has an array: `Order[]`
// is the runtime's type, grown wherever a collection resizes, so anchoring it at
// Order's line would paint that line long after the last Order died.
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
        private Dictionary<TypeDefinitionHandle, (string File, int Line)>? _constructed;

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
            Find(nesting) is { } definition ? Declared(definition) ?? Constructed(definition) : null;

        public void Dispose()
        {
            pdbProvider.Dispose();
            pe.Dispose();
        }

        private (string File, int Line)? Declared(TypeDefinitionHandle definition)
        {
            var points = _metadata.GetTypeDefinition(definition).GetMethods()
                .SelectMany(SequencePoints)
                .Where(point => !point.IsHidden)
                .OrderBy(point => point.StartLine)
                .Take(1)
                .ToList();
            return points.Count == 0 ? null : ConstructionSites.Location(_pdb, points[0]);
        }

        private (string File, int Line)? Constructed(TypeDefinitionHandle definition)
        {
            _constructed ??= ConstructionSites.Index(pe, _metadata, _pdb);
            return _constructed.TryGetValue(definition, out var site) ? site : null;
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
