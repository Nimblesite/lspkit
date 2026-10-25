// Implements [PROF-CPU-SNAPSHOT] and [PROF-DOTNET-ENGINE-PROTOCOL].
namespace LspProfiler.DotNet.Engine;

/// <summary>Why the current recording segment should end.</summary>
internal enum CpuEnd { Continue, Snapshot, Stop }

/// <summary>Reads CPU commands while the current EventPipe segment records.</summary>
internal sealed class CpuControl
{
    private Task<string?> _next = Task.Run(Console.In.ReadLine);

    /// <summary>A snapshot closes one segment; stop or EOF ends the recording.</summary>
    public CpuEnd WaitForEnd(Task copy, TimeSpan length)
    {
        _ = Task.WaitAny([_next, copy], length);
        if (copy.IsCompleted) return CpuEnd.Stop;
        if (!_next.IsCompleted) return CpuEnd.Continue;
        if (_next.GetAwaiter().GetResult() != "snapshot") return CpuEnd.Stop;
        _next = Task.Run(Console.In.ReadLine);
        return CpuEnd.Snapshot;
    }
}
