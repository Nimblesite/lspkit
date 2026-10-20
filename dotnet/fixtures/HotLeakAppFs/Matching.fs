// Hot pattern matching. F# compiles a match's decision tree, here the active-pattern
// tests, to hidden sequence points: a sample there has no line of its own, yet it ran
// in this file, inside `classify`. Tests [PROF-DOTNET-CPU-SYMBOLS].
namespace HotLeakAppFs

open System.Runtime.CompilerServices

module Matching =
    /// The quotient when `divisor` divides `n`.
    [<return: Struct>]
    [<MethodImpl(MethodImplOptions.NoInlining)>]
    let (|Multiple|_|) (divisor: int) (n: int) =
        if n % divisor = 0 then ValueSome(n / divisor) else ValueNone

    /// Each active-pattern test runs in the decision tree, between the arms' lines.
    [<MethodImpl(MethodImplOptions.NoInlining)>]
    let classify (n: int) =
        match n with
        | Multiple 15 quotient -> quotient % 4
        | Multiple 5 quotient -> quotient % 3
        | Multiple 3 quotient -> quotient % 2
        | _ -> 0

    /// The sum of the classes of 0 .. count - 1.
    let classifyAll (count: int) =
        let mutable total = 0

        for n in 0 .. count - 1 do
            total <- total + classify n

        total
