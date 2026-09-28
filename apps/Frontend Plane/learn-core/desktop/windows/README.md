# Teachme Windows companion

Native C#/.NET 10 WPF foundation. The shell opens a local request window, registers **Ctrl+Alt+Space** and dismisses with **Escape**. Closing exits the process and unregisters the shortcut; minimizing keeps the application available from the taskbar and shortcut. If the shortcut is already taken, the UI explains that registration failed and the taskbar remains usable.

The request field is local and is cleared on Escape or close. There is no screen/microphone capture, model request, credential collection, teacher connection, hidden service, device enrollment or input injection in this pass. Buttons report unavailable guidance honestly.

## Build

Install a .NET 10 SDK, or use the ignored workspace-local SDK prepared during this task:

```powershell
cd 'C:\dev\CoresSystem\apps\Frontend Plane\learn-core'
.\.tools\dotnet\dotnet.exe build desktop/windows/Teachme.Windows/Teachme.Windows.csproj -c Release
.\.tools\dotnet\dotnet.exe desktop/windows/Teachme.Windows/bin/Release/net10.0-windows/Teachme.Windows.dll --smoke-test
.\.tools\dotnet\dotnet.exe run --project desktop/windows/Teachme.Windows/Teachme.Windows.csproj
```

`--smoke-test` loads the actual WPF resources/window and exits without displaying the window or registering a global shortcut. It is a runtime-load check, not interactive desktop validation.

The Release build creates a framework-dependent developer executable. It requires the .NET 10 Windows Desktop Runtime. It is **not** a signed installer or a school-ready download. Signing, MSI/MSIX packaging, managed rollout/update/rollback, accessibility checks and device enforcement are later gates.

The Windows adapter will consume versioned Learning/Model/remote guidance contracts. It will not directly reference TypeScript classes or include teacher/student passwords. See the Learning Plane device policy before adding privileged operations.
