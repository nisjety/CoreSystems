using System.Windows;
using System.Windows.Input;
using System.Windows.Interop;

namespace Teachme.Windows;

public partial class MainWindow : Window
{
    private GlobalShortcut? shortcut;

    public MainWindow()
    {
        InitializeComponent();
        SourceInitialized += (_, _) =>
        {
            shortcut = new GlobalShortcut(new WindowInteropHelper(this).Handle, OpenRequest);
            if (!shortcut.IsRegistered)
                ShortcutText.Text = "Shortcut unavailable (it may be in use). Open Teachme from the taskbar.";
        };
        Closed += (_, _) => { shortcut?.Dispose(); RequestText.Clear(); };
    }

    private void OpenRequest()
    {
        if (WindowState == WindowState.Minimized) WindowState = WindowState.Normal;
        Activate();
        RequestText.Focus();
    }

    private void RequestGuidance_Click(object sender, RoutedEventArgs e)
    {
        StatusText.Text = string.IsNullOrWhiteSpace(RequestText.Text)
            ? "Write a learning request first. Guidance is not connected in this foundation."
            : "Guidance is not connected yet. Nothing was sent; your request remains local until you clear or close it.";
    }

    private void Window_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key != Key.Escape) return;
        RequestText.Clear();
        WindowState = WindowState.Minimized;
        e.Handled = true;
    }
}
