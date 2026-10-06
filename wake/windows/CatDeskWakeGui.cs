// LEGACY/RETIRED TEST ARTIFACT ONLY: normal CatDesk operation no longer builds, installs, or launches this standalone Wake GUI. The supported user-facing surface is CatDesk Binagotchy; WakeHost remains the independent backend runtime.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
using System.Web.Script.Serialization;
using System.Windows.Forms;

// Independent GUI process. Closing this window never terminates WakeHost.
internal sealed class WakeWindow : Form
{
    const string Title = "CatDesk Wake";
    readonly TextBox target = new TextBox();
    readonly Label configured = new Label();
    readonly TextBox details = new TextBox();
    readonly Label result = new Label();
    readonly JavaScriptSerializer json = new JavaScriptSerializer();
    readonly System.Windows.Forms.Timer timer = new System.Windows.Forms.Timer();
    long generation;
    bool busy;
    readonly string host = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "CatDeskWakeHost.exe");

    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr window);

    [STAThread]
    static void Main()
    {
        bool created;
        using (var mutex = new Mutex(true, "Local\\CatDeskWakeGui-" + Environment.UserName, out created))
        {
            if (!created)
            {
                for (int i = 0; i < 50; i++)
                {
                    var window = FindWindow(null, Title);
                    if (window != IntPtr.Zero) { ShowWindow(window, 9); SetForegroundWindow(window); return; }
                    Thread.Sleep(100);
                }
                return;
            }
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);
            Application.Run(new WakeWindow());
            mutex.ReleaseMutex();
        }
    }

    WakeWindow()
    {
        Text = Title; ClientSize = new Size(880, 610); MinimumSize = new Size(800, 600);
        Font = new Font("Segoe UI", 10); StartPosition = FormStartPosition.CenterScreen;
        var heading = new Label { Text = "ChatGPT Wake Target", Font = new Font("Segoe UI", 17, FontStyle.Bold), Left = 24, Top = 20, Width = 600, Height = 38 };
        Controls.Add(heading);
        target.SetBounds(24, 70, 655, 30); target.MaxLength = 512; target.AccessibleName = "ChatGPT Wake Target"; Controls.Add(target);
        AddButton("Apply", 693, 68, async delegate {
            string url = target.Text;
            await RunAction(async delegate {
                await Call("validate-target " + Quote(url));
                var response = await Call("set-target " + generation + " " + Quote(url));
                generation = Convert.ToInt64(response["generation"]);
                await RefreshStatus(true); result.Text = "Target applied. Exact readback: " + Convert.ToString(response["url"]);
            });
        });
        AddButton("Refresh", 779, 68, async delegate { await RunAction(async delegate { await RefreshStatus(true); result.Text = "Readback refreshed. You can retry Apply after a conflict."; }); });
        configured.SetBounds(24, 112, 825, 56); configured.AutoEllipsis = true; Controls.Add(configured);
        AddButton("Start Wake", 24, 179, async delegate { await Lifecycle("start"); }, 122);
        AddButton("Pause Wake", 157, 179, async delegate { await Lifecycle("pause"); }, 122);
        AddButton("Resume Wake", 290, 179, async delegate { await Lifecycle("resume"); }, 133);
        AddButton("Stop Wake", 434, 179, async delegate { await Lifecycle("stop"); }, 122);
        AddButton("Validate Target", 24, 226, async delegate { await RunAction(async delegate { await Call("validate-target " + Quote(target.Text)); result.Text = "Exact conversation URL syntax is valid. Browser/login readiness is observed during delivery."; }); }, 155);
        AddButton("Open Target", 191, 226, async delegate { await RunAction(async delegate {
            var snapshot = await Call("status"); var targets = (Dictionary<string, object>)snapshot["targets"];
            if (!targets.ContainsKey("catdesk")) throw new Exception("TARGET_NOT_CONFIGURED");
            var active = (Dictionary<string, object>)targets["catdesk"]; string url = Convert.ToString(active["url"]);
            await Call("validate-target " + Quote(url)); Process.Start(new ProcessStartInfo(url) { UseShellExecute = true });
            result.Text = "Opened the configured target. This does not send a wake.";
        }); }, 133);
        details.SetBounds(24, 282, 830, 238); details.Multiline = true; details.ReadOnly = true; details.ScrollBars = ScrollBars.Vertical;
        details.BackColor = Color.White; details.Font = new Font("Consolas", 10); details.Anchor = AnchorStyles.Top | AnchorStyles.Bottom | AnchorStyles.Left | AnchorStyles.Right;
        Controls.Add(details);
        result.SetBounds(24, 538, 830, 55); result.ForeColor = Color.DarkBlue; result.Anchor = AnchorStyles.Bottom | AnchorStyles.Left | AnchorStyles.Right; Controls.Add(result);
        Shown += async delegate { await RunAction(async delegate { await RefreshStatus(true); }); };
        timer.Interval = 2000; timer.Tick += async delegate { if (!busy) await RunAction(async delegate { await RefreshStatus(false); }); }; timer.Start();
        FormClosed += delegate { timer.Stop(); timer.Dispose(); };
    }
    void AddButton(string text, int left, int top, EventHandler action, int width = 78)
    {
        var button = new Button { Text = text, Left = left, Top = top, Width = width, Height = 34 };
        button.Click += action; Controls.Add(button);
    }
    static string Quote(string value)
    {
        // Accepted URLs have no quotes/backslashes; reject them before shell-free argv.
        if (value.IndexOfAny(new [] {'"', '\\', '\r', '\n'}) >= 0) throw new Exception("TARGET_INVALID_URL");
        return "\"" + value + "\"";
    }
    async Task RunAction(Func<Task> action)
    {
        // A timer refresh must not silently swallow a real button click.
        while (busy && !IsDisposed) await Task.Delay(25);
        if (IsDisposed) return;
        busy = true;
        try { await action(); }
        catch (Exception error) { if (!IsDisposed) result.Text = error.Message; }
        finally { busy = false; }
    }
    async Task Lifecycle(string command)
    {
        await RunAction(async delegate { await Call(command); await Task.Delay(400); await RefreshStatus(false); result.Text = "Wake command: " + command; });
    }
    async Task<Dictionary<string, object>> Call(string arguments)
    {
        return await Task.Run(delegate {
            using (var process = new Process())
            {
                process.StartInfo = new ProcessStartInfo(host, arguments) { UseShellExecute = false, CreateNoWindow = true, RedirectStandardOutput = true, RedirectStandardError = true };
                process.Start(); var output = process.StandardOutput.ReadToEndAsync(); var error = process.StandardError.ReadToEndAsync();
                if (!process.WaitForExit(10000)) { process.Kill(); throw new Exception("HOST_CONTROL_TIMEOUT"); }
                var response = json.Deserialize<Dictionary<string, object>>(output.Result);
                if (response.ContainsKey("error")) throw new Exception(Convert.ToString(response["error"]));
                if (process.ExitCode != 0) throw new Exception("HOST_CONTROL_FAILED");
                return response;
            }
        });
    }
    async Task RefreshStatus(bool replaceEditor)
    {
        var snapshot = await Call("status"); if (IsDisposed) return;
        var targets = (Dictionary<string, object>)snapshot["targets"];
        if (targets.ContainsKey("catdesk"))
        {
            var active = (Dictionary<string, object>)targets["catdesk"];
            if (replaceEditor) { target.Text = Convert.ToString(active["url"]); generation = Convert.ToInt64(active["generation"]); }
            configured.Text = "Configured: " + active["url"] + "\r\nGeneration " + active["generation"] + "  |  SHA-256 " + active["digest"];
        }
        else { configured.Text = "Target is not configured. Enter the exact conversation URL and Apply."; if (replaceEditor) generation = 0; }
        var lines = new List<string>();
        lines.Add("Wake " + snapshot["version"] + "  |  wake-protocol-v" + snapshot["protocolVersion"]);
        lines.Add("HOST: " + snapshot["host"] + "  PID: " + snapshot["pid"]);
        var catdesk = Process.GetProcessesByName("catdesk"); lines.Add("CatDesk: " + (catdesk.Length > 0 ? "RUNNING" : "STOPPED")); foreach (var p in catdesk) p.Dispose();
        lines.Add("QUEUE: " + snapshot["queueDepth"] + "  STALE: " + snapshot["staleCount"]);
        foreach (string key in new [] { "browser", "login", "submission", "lastEventId", "lastAttemptUtc", "lastSuccessUtc", "attention" })
            lines.Add(key + ": " + (snapshot[key] ?? "none"));
        lines.Add("RECEIPT: " + (snapshot["lastReceipt"] == null ? "none" : json.Serialize(snapshot["lastReceipt"])));
        details.Text = string.Join("\r\n", lines);
    }
}
