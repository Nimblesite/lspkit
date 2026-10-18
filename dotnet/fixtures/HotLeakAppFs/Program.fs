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

module Program =
    let private work (stop: CancellationToken) () =
        let mutable sink = 0.0

        while not stop.IsCancellationRequested do
            sink <- sink + Hot.spin 200_000
            LeakCache.grow 50

        GC.KeepAlive sink

    [<EntryPoint>]
    let main _ =
        use stop = new CancellationTokenSource()
        let worker = Thread(work stop.Token, Name = "hot-worker", IsBackground = true)
        worker.Start()
        stdout.WriteLine $"READY {Environment.ProcessId}"
        stdout.Flush()
        stdin.ReadToEnd() |> ignore
        stop.Cancel()
        worker.Join()
        0
