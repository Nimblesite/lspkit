// Unit tests for the failure a command reports. Implements [PROF-ERRORS]: a
// command whose target is gone fails PROCESS_NOT_FOUND whatever broke first,
// because the target exiting is the cause; every other failure keeps its kind
// and its own message.
using LspProfiler.DotNet.Engine;
using Microsoft.Diagnostics.NETCore.Client;
using Xunit;

namespace LspProfiler.DotNet.Engine.Tests;

public sealed class FailureTests
{
    /// <summary>No process has this id: Windows ids are multiples of four and no Unix pid_max reaches it.</summary>
    private const int Gone = int.MaxValue;

    [Theory]
    [InlineData("end-of-stream")]
    [InlineData("broken-pipe")]
    [InlineData("no-collection")]
    [InlineData("stream-ended")]
    [InlineData("server-not-available")]
    [InlineData("unsupported-command")]
    [InlineData("access-denied")]
    [InlineData("client")]
    public void AnyFailureOfACommandWhoseTargetIsGoneIsProcessNotFound(string cause)
    {
        var failure = Errors.Describe(Raised(cause), Gone);

        Assert.Equal("failure", failure.Kind);
        Assert.Equal(ErrorKind.ProcessNotFound, failure.ErrorKind);
        Assert.Equal($"process {Gone} is not running", failure.Message);
    }

    [Theory]
    [InlineData("end-of-stream", "Failed")]
    [InlineData("broken-pipe", "Failed")]
    [InlineData("no-collection", "Failed")]
    [InlineData("stream-ended", "Failed")]
    [InlineData("server-not-available", "UnsupportedRuntime")]
    [InlineData("unsupported-command", "UnsupportedRuntime")]
    [InlineData("access-denied", "PermissionDenied")]
    [InlineData("client", "AttachFailed")]
    public void AFailureWhileTheTargetRunsKeepsItsKindAndMessage(string cause, string kind)
    {
        var error = Raised(cause);

        var failure = Errors.Describe(error, Environment.ProcessId);

        Assert.Equal("failure", failure.Kind);
        Assert.Equal(kind, failure.ErrorKind.ToString());
        Assert.Equal(error.Message, failure.Message);
    }

    [Theory]
    [InlineData("end-of-stream", "Failed")]
    [InlineData("server-not-available", "UnsupportedRuntime")]
    [InlineData("access-denied", "PermissionDenied")]
    [InlineData("client", "AttachFailed")]
    public void AFailureOfACommandWithNoTargetKeepsItsKindAndMessage(string cause, string kind)
    {
        var error = Raised(cause);

        var failure = Errors.Describe(error, null);

        Assert.Equal(kind, failure.ErrorKind.ToString());
        Assert.Equal(error.Message, failure.Message);
    }

    /// <summary>What each way of breaking throws, as the engine's commands meet it.</summary>
    private static Exception Raised(string cause) => cause switch
    {
        "end-of-stream" => new EndOfStreamException("Unable to read beyond the end of the stream."),
        "broken-pipe" => new IOException("Pipe is broken."),
        "no-collection" => new TimeoutException("the runtime did not complete the heap snapshot collection"),
        "stream-ended" => new InvalidOperationException("the event stream ended before the collection completed"),
        "server-not-available" => new ServerNotAvailableException("Process 7 not running compatible .NET runtime."),
        "unsupported-command" => new UnsupportedCommandException("unsupported"),
        "access-denied" => new UnauthorizedAccessException("Access to the path is denied."),
        "client" => new DiagnosticsClientException("connect failed"),
        _ => throw new ArgumentOutOfRangeException(nameof(cause), cause, "no such cause"),
    };
}
