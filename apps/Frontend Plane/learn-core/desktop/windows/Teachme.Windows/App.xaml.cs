using System.Windows;

namespace Teachme.Windows;

public partial class App : Application
{
    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
        var window = new MainWindow();
        MainWindow = window;
        if (e.Args.Contains("--smoke-test", StringComparer.Ordinal))
        {
            // Load real XAML and resource references without opening a desktop surface.
            window.ApplyTemplate();
            Console.WriteLine("Teachme Windows: resources and native request window loaded; no capture or input control enabled.");
            Shutdown(0);
            return;
        }
        window.Show();
    }
}
