# Plan M6 stage 5: opens the app's window on the runner's desktop, which has no GPU, and saves a
# screenshot of it. Once as the app starts there (closing it as a user would, so it saves where
# the window was), and once with wgpu asked for Vulkan, which the runner has no driver for, so the
# app falls back to OpenGL or, where that cannot start either, says so in a message box.
#
# Usage: app_window.ps1 <cloakframe.exe> <output folder>
param(
    [Parameter(Mandatory)] [string] $App,
    [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Win32 {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
}
'@
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$titles = @{ 'CloakFrame' = 'app'; 'CloakFrame cannot start' = 'message' }

# The window alone, as it draws itself: other windows on the runner's desktop may cover it.
function Save-Window([IntPtr] $window, [string] $path) {
    $rect = New-Object Win32+Rect
    if (-not [Win32]::GetWindowRect($window, [ref] $rect)) { return 0 }
    $bitmap = New-Object System.Drawing.Bitmap ($rect.Right - $rect.Left), ($rect.Bottom - $rect.Top)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $dc = $graphics.GetHdc()
    # PW_RENDERFULLCONTENT, without which a DirectX window comes out black.
    $null = [Win32]::PrintWindow($window, $dc, 2)
    $graphics.ReleaseHdc($dc)
    $graphics.Dispose()
    # A window that has not drawn yet has one or two colours; a drawn one has many.
    $colours = @{}
    for ($x = 0; $x -lt $bitmap.Width; $x += 8) {
        for ($y = 0; $y -lt $bitmap.Height; $y += 8) {
            $colours[$bitmap.GetPixel($x, $y).ToArgb()] = $true
        }
    }
    $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bitmap.Dispose()
    return $colours.Count
}

function Wait-Until([scriptblock] $condition, [int] $seconds, [string] $what) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while (-not (& $condition)) {
        if ((Get-Date) -gt $deadline) { throw "timed out waiting for $what" }
        Start-Sleep -Milliseconds 100
    }
}

# Returns the process once its window has drawn, and what the window is: 'app' or 'message'.
# Everything else goes to the host, as a function returns what it writes to the pipeline.
function Open-App([string] $name, [hashtable] $environment) {
    $log = Join-Path $Out "$name.log"
    foreach ($key in $environment.Keys) { Set-Item "env:$key" $environment[$key] }
    try {
        $process = Start-Process -FilePath $App -PassThru -NoNewWindow `
            -RedirectStandardError $log -RedirectStandardOutput (Join-Path $Out "$name.out")
    } finally {
        foreach ($key in $environment.Keys) { Remove-Item "env:$key" }
    }
    # The process may have other top-level windows first, which have no title.
    Wait-Until {
        $process.Refresh()
        $process.HasExited -or $titles.ContainsKey($process.MainWindowTitle)
    } 120 "$name's window"
    if ($process.HasExited) {
        Get-Content $log | Write-Host
        throw "${name}: the app ended with $($process.ExitCode) before it opened a window"
    }
    $shot = Join-Path $Out "$name.png"
    $window = $process.MainWindowHandle
    Wait-Until { (Save-Window $window $shot) -gt 20 } 60 "$name's window to draw"
    Get-Content $log | Write-Host
    Write-Host "${name}: window '$($process.MainWindowTitle)'"
    return @($process, $titles[$process.MainWindowTitle])
}

$process, $kind = Open-App 'default' @{}
if ($kind -ne 'app') { throw 'default: the app window did not open' }
if (-not (Select-String -Quiet -Pattern '^graphics: wgpu, ' (Join-Path $Out 'default.log'))) {
    throw 'default: expected wgpu'
}
$null = $process.CloseMainWindow()
if (-not $process.WaitForExit(30000)) { throw 'default: the app did not close' }
if ($process.ExitCode -ne 0) { throw "default: the app closed with $($process.ExitCode)" }
$settings = Join-Path $env:LOCALAPPDATA 'CloakFrameData\settings.toml'
if (-not (Select-String -Quiet -Pattern '^\[window\]' $settings)) {
    throw "default: closing did not save the window's place in $settings"
}

$process, $kind = Open-App 'no-wgpu' @{ WGPU_BACKEND = 'vulkan' }
$log = Join-Path $Out 'no-wgpu.log'
if (Select-String -Quiet -Pattern '^graphics: wgpu, ' $log) {
    Write-Output 'no-wgpu: the runner has a Vulkan driver, so wgpu started'
} elseif ($kind -eq 'app') {
    if (-not (Select-String -Quiet -Pattern '^graphics: OpenGL, ' $log)) { throw 'no-wgpu: expected OpenGL' }
}
$null = $process.CloseMainWindow()
if (-not $process.WaitForExit(30000)) { throw 'no-wgpu: the app did not close' }
Write-Output "no-wgpu: ended with $($process.ExitCode)"
