using System;
using System.Drawing;
using System.IO;
using System.Windows.Forms;
using System.Web.Script.Serialization;

public sealed class InputEvidence : IMessageFilter {
    public int MouseDowns;
    public int KeyMessages;
    public bool PreFilterMessage(ref Message message) {
        if (message.Msg == 0x201 || message.Msg == 0x204 || message.Msg == 0x207) MouseDowns++;
        if (message.Msg == 0x100 || message.Msg == 0x102 || message.Msg == 0x104) KeyMessages++;
        return false;
    }
}

public sealed class ComputerUseFixture : Form {
    private readonly InputEvidence evidence = new InputEvidence();
    private Form sentinel;
    private readonly ListBox choices = new ListBox();
    private int fallbackCount;
    private readonly TextBox input = new TextBox();
    private readonly Label countLabel = new Label();
    private readonly CheckBox option = new CheckBox();
    private readonly Panel scroll = new Panel();
    private readonly string output;
    private readonly string control;
    private int count;
    private long sequence;
    private readonly JavaScriptSerializer json = new JavaScriptSerializer();

    public ComputerUseFixture(string path, string runId) {
        Application.AddMessageFilter(evidence);
        output = path;
        control = path + ".control";
        Text = "Computer Use Fixture " + runId;
        AccessibleName = "Computer Use Fixture";
        ClientSize = new Size(660, 440);
        StartPosition = FormStartPosition.CenterScreen;
        TopMost = true;
        var description = new Label { Text = "Local automation fixture — no personal data or network", Left = 20, Top = 18, Width = 610, Height = 24 };
        input.SetBounds(20, 62, 500, 32);
        input.AccessibleName = "Fixture Input";
        input.TextChanged += delegate { sequence++; Save(); };
        var increment = new Button { Text = "Increment", AccessibleName = "Fixture Increment", Left = 20, Top = 112, Width = 150, Height = 38 };
        countLabel.SetBounds(190, 118, 300, 28);
        countLabel.AccessibleName = "Fixture Count";
        countLabel.Text = "Count: 0";
        increment.Click += delegate { count++; sequence++; countLabel.Text = "Count: " + count; Save(); };
        option.SetBounds(20, 170, 300, 28);
        option.Text = "Fixture Option";
        option.AccessibleName = "Fixture Option";
        option.CheckedChanged += delegate { sequence++; Save(); };
        var password = new TextBox { Left = 20, Top = 216, Width = 300, UseSystemPasswordChar = true, Text = "FIXTURE-PASSWORD-DO-NOT-EXPOSE", AccessibleName = "Fixture Password" };
        scroll.SetBounds(20, 270, 600, 140);
        scroll.AutoScroll = true;
        scroll.AccessibleName = "Fixture Scroll Region";
        scroll.BorderStyle = BorderStyle.FixedSingle;
        scroll.Controls.Add(new Label { Text = "Scroll start", Left = 10, Top = 10, Width = 200 });
        scroll.Controls.Add(new Label { Text = "Scroll end", Left = 10, Top = 550, Width = 200 });
        choices.SetBounds(540, 62, 100, 80);
        choices.AccessibleName = "Fixture Choices";
        choices.Items.AddRange(new object[] { "Choice A", "Choice B" });
        choices.SelectedIndexChanged += delegate { sequence++; Save(); };
        var fallback = new Panel { Left = 540, Top = 170, Width = 100, Height = 60, AccessibleName = "Fixture Physical Surface", BorderStyle = BorderStyle.FixedSingle };
        fallback.Click += delegate { fallbackCount++; sequence++; Save(); };
        Controls.AddRange(new Control[] { description, input, increment, countLabel, option, password, scroll, choices, fallback });
        Shown += delegate { Activate(); input.Focus(); Save(); };
        var timer = new Timer { Interval = 100 };
        timer.Tick += delegate {
            if (File.Exists(control)) {
                try {
                    var command = File.ReadAllText(control);
                    File.Delete(control);
                    if (command == "move") { Left += 40; Top += 30; sequence++; }
                    if (command == "background") {
                        TopMost = false;
                        sentinel = new Form { Text = "Fixture Foreground Sentinel", ClientSize = new Size(220, 80), TopMost = true, StartPosition = FormStartPosition.Manual, Location = new Point(20, 20) };
                        sentinel.Show(); sentinel.Activate();
                    }
                    if (command == "maximize-background" || command == "normal-background") {
                        WindowState = command == "maximize-background" ? FormWindowState.Maximized : FormWindowState.Normal;
                        sentinel.Activate();
                    }
                    if (command == "untitled") sentinel.Text = "";
                    if (command == "close") { if (sentinel != null) sentinel.Close(); Close(); return; }
                } catch (IOException) { }
            }
            Save();
        };
        timer.Start();
        FormClosed += delegate { timer.Stop(); Save(); };
    }
    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern short GetAsyncKeyState(int key);
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    private struct CursorInfo { public int size; public int flags; public IntPtr cursor; public Point position; }
    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool GetPhysicalCursorPos(out Point point);
    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool GetCursorInfo(ref CursorInfo info);
    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern IntPtr GetForegroundWindow();
    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr handle, out uint pid);
    private void Save() {
        try {
            Point cursor; GetPhysicalCursorPos(out cursor);
            var info = new CursorInfo { size = System.Runtime.InteropServices.Marshal.SizeOf(typeof(CursorInfo)) };
            var visible = GetCursorInfo(ref info) && (info.flags & 1) != 0;
            var foreground = GetForegroundWindow(); uint foregroundPid; GetWindowThreadProcessId(foreground, out foregroundPid);
            var focused = ActiveControl;
            while (focused is ContainerControl && ((ContainerControl)focused).ActiveControl != null) focused = ((ContainerControl)focused).ActiveControl;
            var snapshot = json.Serialize(new { maximized = WindowState == FormWindowState.Maximized, width = Width, height = Height, sentinel_hwnd = sentinel == null || sentinel.IsDisposed ? 0 : sentinel.Handle.ToInt64(), fallback_count = fallbackCount, selected = choices.SelectedIndex, cursor_x = cursor.X, cursor_y = cursor.Y, cursor_visible = visible, foreground_hwnd = foreground.ToInt64(), foreground_pid = foregroundPid, focus_name = focused == null ? null : focused.AccessibleName, mouse_messages = evidence.MouseDowns, key_messages = evidence.KeyMessages, pid = System.Diagnostics.Process.GetCurrentProcess().Id, text = input.Text, count = count, option = option.Checked, scroll_y = -scroll.AutoScrollPosition.Y, sequence = sequence, x = Left, y = Top, mouse_down = GetAsyncKeyState(1) < 0, modifiers_down = GetAsyncKeyState(16) < 0 || GetAsyncKeyState(17) < 0 || GetAsyncKeyState(18) < 0 });
            File.WriteAllText(output, snapshot);
        } catch (IOException) { }
    }
    [STAThread]
    public static void Main(string[] args) {
        if (args.Length != 2) throw new ArgumentException("Expected oracle path and run ID");
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        Application.Run(new ComputerUseFixture(args[0], args[1]));
    }
}
