// Unit tests for the engine's endpoint decision. Implements
// [PROF-DOTNET-ENGINE-ENDPOINT]: an absent endpoint is UnsupportedRuntime, a
// present or unenumerable endpoint lets the attach itself classify the error.
using LspProfiler.DotNet.Engine;
using Xunit;

namespace LspProfiler.DotNet.Engine.Tests;

public sealed class EndpointDecisionTests
{
    [Fact]
    public void AnAbsentEndpointIsUnsupportedRuntime()
    {
        var refusal = Errors.MissingEndpoint(42, false);
        Assert.NotNull(refusal);
        Assert.Equal(ErrorKind.UnsupportedRuntime, refusal!.ErrorKind);
        Assert.Contains("42", refusal.Message, StringComparison.Ordinal);
        Assert.Contains("diagnostics endpoint", refusal.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void APresentEndpointAllowsAttaching()
    {
        Assert.Null(Errors.MissingEndpoint(42, true));
    }

    [Fact]
    public void AnUnenumerableEndpointNamespaceFailsOpen()
    {
        Assert.Null(Errors.MissingEndpoint(42, null));
    }
}
