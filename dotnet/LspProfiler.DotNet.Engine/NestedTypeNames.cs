// The full name of a nested type. Implements [PROF-DOTNET-HEAP-GRAPH].
//
// The runtime names a nested type by its simple name alone (`Small`, not
// `App.Shape+Small`), which hides every F# union case and every C# nested class
// behind a bare word that no source location can be found for. The type's
// metadata token and module recover the declaring chain from the assembly.
using System.Reflection.Metadata;
using System.Reflection.Metadata.Ecma335;
using System.Reflection.PortableExecutable;

namespace LspProfiler.DotNet.Engine;

internal sealed class NestedTypeNames : IDisposable
{
    private readonly Dictionary<string, (PEReader Pe, MetadataReader Metadata)?> _modules = new(StringComparer.Ordinal);

    /// <summary>`Ns.Outer+Inner` and the runtime name's generic arguments; any other name as the runtime gave it.</summary>
    public string Qualify(string runtimeName, string? module, int token)
    {
        var metadata = module is null ? null : Open(module)?.Metadata;
        if (metadata is null || Nested(metadata, token) is not { } definition)
        {
            return runtimeName;
        }
        var bracket = runtimeName.IndexOf('[', StringComparison.Ordinal);
        var bare = bracket < 0 ? runtimeName : runtimeName[..bracket];
        return metadata.StringComparer.Equals(definition.Name, bare)
            ? Chain(metadata, definition) + runtimeName[bare.Length..]
            : runtimeName;
    }

    public void Dispose()
    {
        foreach (var module in _modules.Values)
        {
            module?.Pe.Dispose();
        }
    }

    /// <summary>The nested type definition `token` names in this module, if it is one.</summary>
    private static TypeDefinition? Nested(MetadataReader metadata, int token)
    {
        var row = token & 0xFFFFFF;
        if (token >>> 24 != (int)TableIndex.TypeDef || row == 0 || row > metadata.GetTableRowCount(TableIndex.TypeDef))
        {
            return null;
        }
        var definition = metadata.GetTypeDefinition(MetadataTokens.TypeDefinitionHandle(row));
        return definition.GetDeclaringType().IsNil ? null : definition;
    }

    private static string Chain(MetadataReader metadata, TypeDefinition definition)
    {
        var name = metadata.GetString(definition.Name);
        var outer = definition.GetDeclaringType();
        if (!outer.IsNil)
        {
            return $"{Chain(metadata, metadata.GetTypeDefinition(outer))}+{name}";
        }
        var space = metadata.GetString(definition.Namespace);
        return space.Length == 0 ? name : $"{space}.{name}";
    }

    private (PEReader Pe, MetadataReader Metadata)? Open(string module)
    {
        if (!_modules.TryGetValue(module, out var opened))
        {
            opened = Read(module);
            _modules[module] = opened;
        }
        return opened;
    }

    private static (PEReader, MetadataReader)? Read(string module)
    {
        if (!File.Exists(module))
        {
            return null;
        }
        var pe = new PEReader(File.OpenRead(module));
        try
        {
            return pe.HasMetadata ? (pe, pe.GetMetadataReader()) : throw new BadImageFormatException(module);
        }
        catch (Exception error) when (error is BadImageFormatException or IOException or InvalidOperationException)
        {
            pe.Dispose();
            return null;
        }
    }
}
