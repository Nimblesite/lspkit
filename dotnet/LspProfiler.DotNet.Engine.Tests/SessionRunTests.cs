// Unit tests for how an EventPipe session's run ends. Implements
// [PROF-DOTNET-HEAP-CAPTURE] and [PROF-DOTNET-GC]: the run ends when the watcher
// sees its collection, when the stream ends because the target took the session
// with it, or at the deadline, and only a session still streaming is stopped.
using LspProfiler.DotNet.Engine;
using Xunit;

namespace LspProfiler.DotNet.Engine.Tests;

public sealed class SessionRunTests
{
    /// <summary>Long enough that nothing here reaches it unless the run waits for a watcher that cannot finish.</summary>
    private static readonly TimeSpan Bounded = TimeSpan.FromSeconds(5);

    [Fact]
    public void AStreamThatEndsBeforeTheWatcherEndsTheRunWithoutStoppingTheSession()
    {
        using var done = new ManualResetEvent(false);
        var stops = new Stops();

        var end = SessionRun.Await(done, Task.CompletedTask, stops.Stop, Bounded);

        Assert.Equal(SessionEnd.StreamEnded, end);
        Assert.Equal(0, stops.Count);
        Assert.False(done.WaitOne(0));
    }

    [Fact]
    public void AStreamThatFailsBeforeTheWatcherEndsTheRunWithoutStoppingTheSession()
    {
        using var done = new ManualResetEvent(false);
        var stops = new Stops();
        var broken = Task.FromException(new EndOfStreamException("the target exited"));

        var end = SessionRun.Await(done, broken, stops.Stop, Bounded);

        Assert.Equal(SessionEnd.StreamEnded, end);
        Assert.Equal(0, stops.Count);
        Assert.True(broken.IsFaulted);
    }

    [Fact]
    public void AWatcherDoneWhileStreamingStopsTheSessionOnceAndDrainsIt()
    {
        using var done = new ManualResetEvent(true);
        var stream = new TaskCompletionSource();
        var stops = new Stops(stream);

        var end = SessionRun.Await(done, stream.Task, stops.Stop, Bounded);

        Assert.Equal(SessionEnd.Watched, end);
        Assert.Equal(1, stops.Count);
        Assert.True(stream.Task.IsCompletedSuccessfully);
    }

    [Fact]
    public void AWatcherDoneAfterTheStreamEndedLeavesNothingToStop()
    {
        using var done = new ManualResetEvent(true);
        var stops = new Stops();

        var end = SessionRun.Await(done, Task.CompletedTask, stops.Stop, Bounded);

        Assert.Equal(SessionEnd.Watched, end);
        Assert.Equal(0, stops.Count);
    }

    [Fact]
    public void ADeadlineWithNeitherStopsTheStreamingSessionAndSaysSo()
    {
        using var done = new ManualResetEvent(false);
        var stream = new TaskCompletionSource();
        var stops = new Stops(stream);

        var end = SessionRun.Await(done, stream.Task, stops.Stop, TimeSpan.Zero);

        Assert.Equal(SessionEnd.DeadlinePassed, end);
        Assert.Equal(1, stops.Count);
        Assert.True(stream.Task.IsCompletedSuccessfully);
    }

    [Fact]
    public void AStoppedSessionWhoseStreamNeverEndsIsATimeout()
    {
        using var done = new ManualResetEvent(true);
        var endless = new TaskCompletionSource();
        var stops = new Stops();

        var error = Assert.Throws<TimeoutException>(() => SessionRun.Await(done, endless.Task, stops.Stop, TimeSpan.Zero));

        Assert.Equal(1, stops.Count);
        Assert.Contains("did not end after the session was stopped", error.Message, StringComparison.Ordinal);
        Assert.False(endless.Task.IsCompleted);
    }

    /// <summary>Counts stops; stopping ends <paramref name="stream"/> when given, as stopping a live session does.</summary>
    private sealed class Stops(TaskCompletionSource? stream = null)
    {
        public int Count { get; private set; }

        public void Stop()
        {
            Count++;
            stream?.TrySetResult();
        }
    }
}
