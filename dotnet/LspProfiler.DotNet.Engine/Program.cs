// Command dispatch for the .NET profiling engine. Implements [PROF-DOTNET-ENGINE].
//
//   cpu      --pid N [--native]                  sample until a line (or EOF) arrives on stdin
//   snapshot --pid N --graph PATH [--max-types K] [--buffer-mb M]
//                                                 capture the live heap graph and summarise it
//   analyze  --graph PATH [--max-types K]         summarise a kept heap graph
//   refs     --graph PATH --type T [--max-depth D] [--max-nodes N]
//   objects  --graph PATH --type T [--limit L]
//   gc       --pid N                              induce a full blocking collection
using LspProfiler.DotNet.Engine;

return Cli.Run(args);

namespace LspProfiler.DotNet.Engine
{
    internal static class Cli
    {
        public static int Run(string[] args)
        {
            var options = Options.Parse(args);
            try
            {
                return Dispatch(options);
            }
            catch (Exception error) when (error is not OutOfMemoryException)
            {
                return Emit.Fail(Errors.Classify(error), error.Message);
            }
        }

        private static int Dispatch(Options options) => options.Command switch
        {
            "cpu" => CpuCommand.Run(options.Int("pid"), options.Flag("native")),
            "snapshot" => HeapCommands.Snapshot(options.Int("pid"), options.Text("graph"), options.Int("max-types", 50), options.Int("buffer-mb", HeapCapture.DefaultBufferMB)),
            "analyze" => HeapCommands.Analyze(options.Text("graph"), options.Int("max-types", 50)),
            "refs" => HeapCommands.References(options.Text("graph"), options.Text("type"), options.Int("max-depth", 8), options.Int("max-nodes", 200)),
            "objects" => HeapCommands.ObjectsOfType(options.Text("graph"), options.Text("type"), options.Int("limit", 50)),
            "gc" => GcCommand.Run(options.Int("pid")),
            _ => Emit.Fail(ErrorKind.Failed, $"unknown command '{options.Command}'"),
        };
    }

    /// <summary>`--name value` and `--flag` options after the command word.</summary>
    internal sealed class Options
    {
        private readonly Dictionary<string, string> _values;

        private Options(string command, Dictionary<string, string> values)
        {
            Command = command;
            _values = values;
        }

        public string Command { get; }

        public static Options Parse(string[] args)
        {
            var values = new Dictionary<string, string>(StringComparer.Ordinal);
            for (var i = 1; i < args.Length; i++)
            {
                var name = args[i].TrimStart('-');
                var hasValue = i + 1 < args.Length && !args[i + 1].StartsWith("--", StringComparison.Ordinal);
                values[name] = hasValue ? args[++i] : "true";
            }
            return new Options(args.Length > 0 ? args[0] : "", values);
        }

        public string Text(string name) =>
            _values.TryGetValue(name, out var value) ? value : throw new ArgumentException($"missing --{name}");

        public int Int(string name) => int.Parse(Text(name), System.Globalization.CultureInfo.InvariantCulture);

        public int Int(string name, int fallback) => _values.ContainsKey(name) ? Int(name) : fallback;

        public bool Flag(string name) => _values.ContainsKey(name);
    }
}
