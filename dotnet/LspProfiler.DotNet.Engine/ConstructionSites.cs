// Where an assembly's own code constructs its types. Implements [PROF-DOTNET-SOURCE].
//
// Every member F# generates for a record or a union carries only hidden
// sequence points, so such a type has no declaration to anchor at. The code
// that builds it does: a `newobj` of one of its constructors, or for a union
// case the static `New<Case>` factory F# emits on the union. Only code with
// visible sequence points counts, which leaves out the generated factories.
using System.Reflection;
using System.Reflection.Emit;
using System.Reflection.Metadata;
using System.Reflection.Metadata.Ecma335;
using System.Reflection.PortableExecutable;

namespace LspProfiler.DotNet.Engine;

internal static class ConstructionSites
{
    private static readonly Dictionary<ILOpCode, OperandType> Operands = typeof(OpCodes)
        .GetFields(BindingFlags.Public | BindingFlags.Static)
        .Select(field => (OpCode)field.GetValue(null)!)
        .DistinctBy(code => code.Value)
        .ToDictionary(code => (ILOpCode)(ushort)code.Value, code => code.OperandType);

    /// <summary>Each constructed type's lowest construction site that `locate` places, over every method body.</summary>
    public static Dictionary<TypeDefinitionHandle, (string File, int Line)> Index(
        PEReader pe, MetadataReader metadata, MetadataReader pdb, Func<SequencePoint, (string File, int Line)?> locate)
    {
        var sites = new Dictionary<TypeDefinitionHandle, (string File, int Line)>();
        foreach (var (type, point) in metadata.MethodDefinitions.SelectMany(method => InMethod(pe, metadata, pdb, method)))
        {
            if (locate(point) is { } site && (!sites.TryGetValue(type, out var known) || site.Line < known.Line))
            {
                sites[type] = site;
            }
        }
        return sites;
    }

    private static List<(TypeDefinitionHandle Type, SequencePoint Point)> InMethod(PEReader pe, MetadataReader metadata, MetadataReader pdb, MethodDefinitionHandle method)
    {
        var points = pdb.GetMethodDebugInformation(method.ToDebugInformationHandle()).GetSequencePoints()
            .Where(point => !point.IsHidden)
            .OrderBy(point => point.Offset)
            .ToList();
        var rva = metadata.GetMethodDefinition(method).RelativeVirtualAddress;
        if (points.Count == 0 || rva == 0)
        {
            return [];
        }
        return [.. Constructions(metadata, pe.GetMethodBody(rva).GetILReader())
            .Select(found => (found.Type, Point: points.LastOrDefault(point => point.Offset <= found.Offset)))
            .Where(found => found.Point.Document is { IsNil: false })];
    }

    private static List<(TypeDefinitionHandle Type, int Offset)> Constructions(MetadataReader metadata, BlobReader il)
    {
        var found = new List<(TypeDefinitionHandle, int)>();
        while (il.RemainingBytes > 0)
        {
            var offset = il.Offset;
            var code = ReadOpCode(ref il);
            if (code is ILOpCode.Newobj or ILOpCode.Call)
            {
                var constructed = Constructed(metadata, code, MetadataTokens.EntityHandle(il.ReadInt32()));
                if (constructed is { } type)
                {
                    found.Add((type, offset));
                }
                continue;
            }
            Skip(ref il, Operands.GetValueOrDefault(code, OperandType.InlineNone));
        }
        return found;
    }

    private static ILOpCode ReadOpCode(ref BlobReader il)
    {
        var first = il.ReadByte();
        return (ILOpCode)(first == 0xFE ? 0xFE00 | il.ReadByte() : first);
    }

    /// <summary>Step over an operand. A switch's count is read first, so its targets follow it.</summary>
    private static void Skip(ref BlobReader il, OperandType operand)
    {
        var size = operand switch
        {
            OperandType.InlineNone => 0,
            OperandType.ShortInlineBrTarget or OperandType.ShortInlineI or OperandType.ShortInlineVar => 1,
            OperandType.InlineVar => 2,
            OperandType.InlineI8 or OperandType.InlineR => 8,
            OperandType.InlineSwitch => 4 * il.ReadInt32(),
            _ => 4,
        };
        il.Offset += size;
    }

    /// <summary>The type a `newobj` or a `New&lt;Case&gt;` call builds, when this module defines it.</summary>
    private static TypeDefinitionHandle? Constructed(MetadataReader metadata, ILOpCode code, EntityHandle target)
    {
        if (Callee(metadata, target) is not { } callee)
        {
            return null;
        }
        if (code == ILOpCode.Newobj)
        {
            return callee.Type;
        }
        return callee.Name.StartsWith("New", StringComparison.Ordinal) ? CaseOf(metadata, callee.Type, callee.Name[3..]) : null;
    }

    /// <summary>A multi-case union builds the nested `Case` class; a single-case union builds itself.</summary>
    private static TypeDefinitionHandle CaseOf(MetadataReader metadata, TypeDefinitionHandle union, string caseName) =>
        metadata.GetTypeDefinition(union).GetNestedTypes()
            .Where(nested => metadata.StringComparer.Equals(metadata.GetTypeDefinition(nested).Name, caseName))
            .Select(nested => (TypeDefinitionHandle?)nested)
            .FirstOrDefault() ?? union;

    /// <summary>A called method's declaring type and name; a generic type's members arrive as member references.</summary>
    private static (TypeDefinitionHandle Type, string Name)? Callee(MetadataReader metadata, EntityHandle target) => target.Kind switch
    {
        HandleKind.MethodDefinition => Defined(metadata, metadata.GetMethodDefinition((MethodDefinitionHandle)target)),
        HandleKind.MemberReference => Referenced(metadata, metadata.GetMemberReference((MemberReferenceHandle)target)),
        _ => null,
    };

    private static (TypeDefinitionHandle Type, string Name) Defined(MetadataReader metadata, MethodDefinition method) =>
        (method.GetDeclaringType(), metadata.GetString(method.Name));

    private static (TypeDefinitionHandle Type, string Name)? Referenced(MetadataReader metadata, MemberReference member) =>
        member.Parent.Kind == HandleKind.TypeSpecification && Instantiated(metadata, (TypeSpecificationHandle)member.Parent) is { } type
            ? (type, metadata.GetString(member.Name))
            : null;

    /// <summary>The definition a `Generic&lt;Args&gt;` instantiation closes, when this module defines it.</summary>
    private static TypeDefinitionHandle? Instantiated(MetadataReader metadata, TypeSpecificationHandle spec)
    {
        var signature = metadata.GetBlobReader(metadata.GetTypeSpecification(spec).Signature);
        if (signature.ReadSignatureTypeCode() != SignatureTypeCode.GenericTypeInstance
            || signature.ReadSignatureTypeCode() != SignatureTypeCode.TypeHandle)
        {
            return null;
        }
        var definition = signature.ReadTypeHandle();
        return definition.Kind == HandleKind.TypeDefinition ? (TypeDefinitionHandle)definition : null;
    }
}
