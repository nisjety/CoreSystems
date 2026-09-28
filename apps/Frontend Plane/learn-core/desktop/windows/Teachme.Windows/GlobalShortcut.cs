using System.Runtime.InteropServices;
using System.Windows.Interop;

namespace Teachme.Windows;

// RegisterHotKey observes this one chord only; it is not a keyboard-recording hook.
internal sealed class GlobalShortcut : IDisposable
{
    private const int ShortcutId = 0x544D;
    private const int WmHotkey = 0x0312;
    private readonly nint window;
    private readonly HwndSource? source;
    private readonly Action onPressed;
    private bool disposed;
    public bool IsRegistered { get; }

    public GlobalShortcut(nint window, Action onPressed)
    {
        this.window = window;
        this.onPressed = onPressed;
        source = HwndSource.FromHwnd(window);
        // MOD_ALT | MOD_CONTROL | MOD_NOREPEAT, VK_SPACE.
        IsRegistered = RegisterHotKey(window, ShortcutId, 0x0001 | 0x0002 | 0x4000, 0x20);
        if (IsRegistered) source?.AddHook(HandleMessage);
    }

    private nint HandleMessage(nint hwnd, int message, nint wParam, nint lParam, ref bool handled)
    {
        if (message == WmHotkey && wParam == ShortcutId) { handled = true; onPressed(); }
        return 0;
    }

    public void Dispose()
    {
        if (disposed) return;
        disposed = true;
        if (IsRegistered) { UnregisterHotKey(window, ShortcutId); source?.RemoveHook(HandleMessage); }
    }

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool RegisterHotKey(nint hWnd, int id, uint modifiers, uint key);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool UnregisterHotKey(nint hWnd, int id);
}
