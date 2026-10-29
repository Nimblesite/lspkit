// Induced garbage collection. Implements [PROF-DOTNET-GC].
//
// Enabling the runtime's GCHeapCollect keyword on an EventPipe session makes
// the runtime perform a full blocking collection, and the GC keyword reports
// the heap statistics that follow it. The result is what the heap looks like
// after everything unreachable is gone, which turns "big" into "leaking".
using System.Diagnostics.Tracing;
using Microsoft.Diagnostics.NETCore.Client;
using Microsoft.Diagnostics.Tracing;
using Microsoft.Diagnostics.Tracing.Parsers;

namespace LspProfiler.DotNet.Engine;

internal static class GcCommand
{
    private const string RuntimeProvider = "Microsoft-Windows-DotNETRuntime";
    private static readonly TimeSpan Deadline = TimeSpan.FromSeconds(30);

    private const ClrTraceEventParser.Keywords Keywords =
        ClrTraceEventParser.Keywords.GC | ClrTraceEventParser.Keywords.GCHeapCollect;

    public static int Run(int pid)
    {
        var (client, failure) = Errors.Connect(pid);
        if (client is null)
        {
            Emit.Message(failure!);
            return 1;
        }
        EventPipeProvider[] providers = [new(RuntimeProvider, EventLevel.Informational, (long)Keywords)];
        using var session = client.StartEventPipeSession(providers, requestRundown: false);
        using var source = new EventPipeEventSource(session.EventStream);
        using var watch = new GcWatch();
        watch.Attach(source);
        // A stream that ended took the target with it: that is a failure, not an
        // unobserved collection ([PROF-DOTNET-GC]).
        if (SessionRun.Until(session, source, watch, Deadline) == SessionEnd.StreamEnded && !watch.Collected)
        {
            throw SessionRun.Unobserved(SessionEnd.StreamEnded, "the collection");
        }
        var generations = watch.Generations();
        Emit.Message(new GcDone("gc", watch.Collected, generations.Sum(), generations));
        return 0;
    }
}
