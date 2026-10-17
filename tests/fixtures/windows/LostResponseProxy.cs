using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;
using System.Web.Script.Serialization;

// Test-only stdio proxy: forward real native work, lose exactly the first act response.
public static class LostResponseProxy {
    public static void Main(string[] args) {
        string helper = Environment.GetEnvironmentVariable("CU_TEST_REAL_HELPER");
        if (String.IsNullOrEmpty(helper) || args.Length != 2 || args[0] != "--policy") throw new ArgumentException("Test helper and policy required");
        Console.InputEncoding = new System.Text.UTF8Encoding(false);
        Console.OutputEncoding = new System.Text.UTF8Encoding(false);
        var config = new ProcessStartInfo(helper, "--policy \"" + args[1] + "\"") { UseShellExecute = false, CreateNoWindow = true, RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true, StandardOutputEncoding = new System.Text.UTF8Encoding(false), StandardErrorEncoding = new System.Text.UTF8Encoding(false) };
        using (var native = Process.Start(config)) {
            var dropped = new HashSet<string>();
            var gate = new object();
            bool first = true;
            var output = new Thread(delegate() {
                string line;
                var parser = new JavaScriptSerializer { MaxJsonLength = 16 * 1024 * 1024 };
                while ((line = native.StandardOutput.ReadLine()) != null) {
                    var response = parser.Deserialize<Dictionary<string, object>>(line);
                    bool discard;
                    lock (gate) { discard = dropped.Remove((string)response["id"]); }
                    if (!discard) { Console.Out.WriteLine(line); Console.Out.Flush(); }
                }
            });
            var errors = new Thread(delegate() { string line; while ((line = native.StandardError.ReadLine()) != null) Console.Error.WriteLine(line); });
            output.Start(); errors.Start();
            string request;
            var json = new JavaScriptSerializer { MaxJsonLength = 16 * 1024 * 1024 };
            while ((request = Console.ReadLine()) != null) {
                var message = json.Deserialize<Dictionary<string, object>>(request);
                var command = (Dictionary<string, object>)message["command"];
                lock (gate) {
                    if (first && (string)command["method"] == "act") { dropped.Add((string)message["id"]); first = false; }
                }
                native.StandardInput.WriteLine(request); native.StandardInput.Flush();
            }
            native.StandardInput.Close(); native.WaitForExit(); output.Join(); errors.Join();
        }
    }
}
