// A deterministic profiling target. Prints "READY <pid>" once running, then
// burns CPU in Hot.spin and grows LeakCache.items until stdin closes.
namespace HotLeakAppFs

open System
open System.Threading

type PayloadKind =
    | Small of size: int
    | Large of size: int

type Tagged<'T> = { Value: 'T; Tag: string }

type LeakedPayload = { Data: byte array; Label: string; Kind: PayloadKind; Order: Tagged<int> }

module LeakCache =
    let items = ResizeArray<LeakedPayload>()

    let grow count =
        for _ in 1..count do
            let kind = Small items.Count
            let order = { Value = items.Count; Tag = "order" }
            items.Add { Data = Array.zeroCreate 1024; Label = $"payload-{items.Count}"; Kind = kind; Order = order }

module Hot =
    let spin iterations =
        let mutable total = 0.0

        for i in 0 .. iterations - 1 do
            total <- total + sqrt (float i) * sin (float i)

        total

/// A single-case union: every id is its own heap object, never a struct.
type CustomerId = CustomerId of int

/// A union with a nullary case (no runtime class) and a case with fields (a nested class).
type Priority =
    | Standard
    | Express of days: int

/// A struct record: stored inline, so it is never a heap type of its own.
[<Struct>]
type Money = { Cents: int64 }

type OrderLine = { Sku: string; Quantity: int; Price: Money }

type Order =
    { Id: int
      Customer: CustomerId
      Lines: OrderLine list
      Priority: Priority
      Data: byte array }

/// A memoized quote: built once per SKU and retained by the cache below.
type Quote = { Sku: string; Price: decimal; Tags: Set<string> }

/// Two nodes can point at each other, so the captured heap contains a real cycle.
type Node = { Label: string; mutable Next: Node option }

/// The F# heap zoo: every shape is retained by a module value so it stays reachable.
/// The line protocol grows one shape at a time: `grow`, `payload`, `blob`, `release all`,
/// `work on|off`, `classify` and `stats`; `stop` closes the program.
module Shapes =
    let orders = ResizeArray<Order>()
    let subscriptions = Event<Order>()
    let cycles = ResizeArray<Node>()
    let blobs = ResizeArray<byte array>()
    let private cache = System.Collections.Generic.Dictionary<string, Quote>(StringComparer.Ordinal)
    let mutable private subscribed = 0

    let private makeOrder (id: int) =
        let customer = CustomerId(id % 1_000)
        let price = { Cents = int64 (100 + id % 900) }
        let line = { Sku = sprintf "SKU-%05d" (id % 400); Quantity = 1 + id % 4; Price = price }
        let priority = if id % 3 = 0 then Express(1 + id % 5) else Standard
        { Id = id; Customer = customer; Lines = [ line ]; Priority = priority; Data = Array.zeroCreate 256 }

    let private makeQuote (sku: string) =
        let price = 10M + decimal (sku.Length % 17) * 1.5M
        let tags = if price > 20M then set [ "retail"; "premium" ] else set [ "retail" ]
        { Sku = sku; Price = price; Tags = tags }

    /// Appends `count` orders, one never-removed subscription closure each and one
    /// cached quote each; the first call also builds a two-node reference cycle.
    let grow count =
        if cycles.Count = 0 then
            let first = { Label = "first"; Next = None }
            let second = { Label = "second"; Next = Some first }
            first.Next <- Some second
            cycles.Add first
            cycles.Add second

        for _ in 1..count do
            let created = makeOrder orders.Count
            orders.Add created
            subscriptions.Publish.Add(fun published -> ignore (published.Id + created.Id))
            |> ignore
            subscribed <- subscribed + 1
            let quoted = makeQuote(sprintf "SKU-Q%06d" orders.Count)
            cache.[quoted.Sku] <- quoted
            cycles.Add { Label = quoted.Sku; Next = None }

    /// Retains one 1 MiB array per megabyte.
    let retain mebibytes =
        for _ in 1..mebibytes do
            blobs.Add(Array.zeroCreate<byte> (1024 * 1024))

    /// Drops every retained shape. The event's handler closures stay: a handler is never
    /// unsubscribed, which is the leak this fixture exists to show.
    let release () =
        LeakCache.items.Clear()
        orders.Clear()
        cycles.Clear()
        blobs.Clear()
        cache.Clear()

    let counts () =
        sprintf
            "orders=%d subscriptions=%d cycles=%d blobs=%d quotes=%d payloads=%d"
            orders.Count
            subscribed
            cycles.Count
            blobs.Count
            cache.Count
            LeakCache.items.Count

module Program =
    let mutable private working = true
    let mutable private busy = false

    let private work (stop: CancellationToken) () =
        let mutable sink = 0.0

        while not stop.IsCancellationRequested do
            if Volatile.Read(&working) then
                Volatile.Write(&busy, true)
                sink <- sink + Hot.spin 200_000
                LeakCache.grow 50
                Volatile.Write(&busy, false)
            else
                // Idle must still allocate: the runtime only performs the snapshot's
                // induced collection when it is running managed code.
                sink <- sink + Hot.spin 200_000
                GC.KeepAlive(Array.zeroCreate<byte> 4_096)
                Thread.Sleep 5

        GC.KeepAlive sink

    let private describe () = Shapes.counts ()

    let private reply (line: string) =
        stdout.WriteLine line
        stdout.Flush()

    let private positive (name: string) (text: string) (action: int -> unit) =
        match Int32.TryParse text with
        | true, count when count > 0 ->
            action count
            sprintf "OK %s %s %s" name text (describe ())
        | _ -> sprintf "ERR %s %s" name text

    let private apply (line: string) : string =
        match
            line.Split([| ' ' |], StringSplitOptions.RemoveEmptyEntries) |> List.ofArray
        with
        | [ "grow"; count ] -> positive "grow" count Shapes.grow
        | [ "payload"; count ] -> positive "payload" count LeakCache.grow
        | [ "blob"; count ] -> positive "blob" count Shapes.retain
        | [ "classify"; count ] -> positive "classify" count (Matching.classifyAll >> ignore)
        | [ "release"; "all" ] ->
            Shapes.release ()
            sprintf "OK release all %s" (describe ())
        | [ "work"; "on" ] ->
            Volatile.Write(&working, true)
            sprintf "OK work on %s" (describe ())
        | [ "work"; "off" ] ->
            Volatile.Write(&working, false)
            while Volatile.Read(&busy) do
                Thread.Sleep 1
            sprintf "OK work off %s" (describe ())
        | [ "stats" ] -> sprintf "OK stats %s" (describe ())
        | _ -> sprintf "ERR unknown %s" line

    let private handle (line: string) : string = apply line

    [<EntryPoint>]
    let main _ =
        use stop = new CancellationTokenSource()
        let worker = Thread(work stop.Token, Name = "hot-worker", IsBackground = true)
        worker.Start()
        reply (sprintf "READY %d" Environment.ProcessId)
        let mutable reading = true

        while reading do
            match stdin.ReadLine() with
            | null -> reading <- false
            | "stop" -> reading <- false
            | line -> reply (handle line)

        stop.Cancel()
        worker.Join()
        0
