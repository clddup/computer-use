// Dedicated screenshot fixture: commands use a private file, never injected input.
// Compile using the in-box .NET Framework csc.exe (C# 5 compatible).
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Web.Script.Serialization;
using System.Windows.Forms;

internal sealed class CaptureColorForm : Form
{
    internal string ColorName;
    internal int PaintCount;
    internal CaptureColorForm(string title, Point location, string color)
    {
        Text = title;
        StartPosition = FormStartPosition.Manual;
        Location = location;
        ClientSize = new Size(420, 300);
        FormBorderStyle = FormBorderStyle.FixedSingle;
        MaximizeBox = false;
        AutoScaleMode = AutoScaleMode.None;
        DoubleBuffered = true;
        SetColor(color);
    }
    internal void SetColor(string name)
    {
        ColorName = name;
        switch (name)
        {
            case "red": BackColor = Color.FromArgb(220, 30, 30); break;
            case "green": BackColor = Color.FromArgb(30, 210, 40); break;
            case "blue": BackColor = Color.FromArgb(25, 55, 220); break;
            case "magenta": BackColor = Color.FromArgb(210, 35, 200); break;
            default: throw new ArgumentException("Unknown fixture color");
        }
        Invalidate();
        Update();
    }
    protected override void OnPaint(PaintEventArgs e)
    {
        base.OnPaint(e);
        PaintCount++;
    }
}

internal static class SharedCaptureFixture
{
    [STAThread]
    private static void Main(string[] args)
    {
        if (args.Length != 2) return;
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        string oracle = args[0];
        string command = oracle + ".control";
        var json = new JavaScriptSerializer();
        var a = new CaptureColorForm("Shared Capture A " + args[1], new Point(60, 80), "red");
        var b = new CaptureColorForm("Shared Capture B " + args[1], new Point(520, 80), "blue");
        a.Show();
        b.Show();
        long aHandle = a.Handle.ToInt64();
        long bHandle = b.Handle.ToInt64();
        long sequence = 0;
        string lastError = null;
        var lifetime = Stopwatch.StartNew();
        var timer = new System.Windows.Forms.Timer { Interval = 50 };
        timer.Tick += delegate
        {
            try
            {
                if (File.Exists(command))
                {
                    // If the writer is still publishing, leave the file for next tick.
                    var request = json.Deserialize<Dictionary<string, object>>(File.ReadAllText(command));
                    long next = Convert.ToInt64(request["sequence"]);
                    if (next > sequence)
                    {
                        string action = Convert.ToString(request["action"]);
                        var target = Convert.ToString(request["target"]) == "a" ? a : b;
                        if (action == "color") target.SetColor(Convert.ToString(request["color"]));
                        else if (action == "close") target.Close();
                        else if (action == "quit") { Application.Exit(); return; }
                        else throw new ArgumentException("Unknown fixture command");
                        sequence = next;
                    }
                    File.Delete(command);
                }
            }
            catch (Exception error) { lastError = error.GetType().Name + ": " + error.Message; }
            try
            {
                string snapshot = json.Serialize(new {
                    pid = Process.GetCurrentProcess().Id,
                    sequence = sequence,
                    a_hwnd = aHandle.ToString(), b_hwnd = bHandle.ToString(),
                    a_closed = a.IsDisposed, b_closed = b.IsDisposed,
                    a_color = a.ColorName, b_color = b.ColorName,
                    a_paints = a.PaintCount, b_paints = b.PaintCount,
                    error = lastError
                });
                File.WriteAllText(oracle + ".tmp", snapshot);
                if (File.Exists(oracle)) File.Delete(oracle);
                File.Move(oracle + ".tmp", oracle);
            }
            catch (IOException) { }
            // Safety net if the test driver disappears. Does not close any other app.
            if (lifetime.Elapsed.TotalSeconds > 120) Application.Exit();
        };
        timer.Start();
        Application.Run();
        timer.Dispose();
        a.Dispose();
        b.Dispose();
    }
}
