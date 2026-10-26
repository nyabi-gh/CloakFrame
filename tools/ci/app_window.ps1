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
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path

function Save-Screen([string] $path) {
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
    $graphics.Dispose()
    # A blank or half-drawn screen has few colours; a drawn window has many.
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
    Wait-Until {
        $process.Refresh()
        $process.HasExited -or $process.MainWindowHandle -ne 0
    } 120 "$name's window"
    if ($process.HasExited) {
        Get-Content $log | Write-Host
        throw "${name}: the app ended with $($process.ExitCode) before it opened a window"
    }
    $shot = Join-Path $Out "$name.png"
    Wait-Until { (Save-Screen $shot) -gt 20 } 60 "$name's window to draw"
    Get-Content $log | Write-Host
    $kind = if ($process.MainWindowTitle -eq 'CloakFrame') { 'app' } else { 'message' }
    Write-Host "${name}: window '$($process.MainWindowTitle)'"
    return @($process, $kind)
}

$process, $kind = Open-App 'default' @{}
if ($kind -ne 'app') { throw 'default: the app window did not open' }
if (-not (Select-String -Quiet -Pattern '^graphics: wgpu, ' (Join-Path $Out 'default.log'))) {
    throw 'default: expected wgpu'
}
$null = $process.CloseMainWindow()
if (-not $process.WaitForExit(30000)) { throw 'default: the app did not close' }
if ($process.ExitCode -ne 0) { throw "default: the app closed with $($process.ExitCode)" }
$settings = Join-Path $env:LOCALAPPDATA 'CloakFrame\settings.toml'
if (-not (Select-String -Quiet -Pattern '^\[window\]' $settings)) {
    throw "default: closing did not save the window's place in $settings"
}

$process, $kind = Open-App 'no-wgpu' @{ WGPU_BACKEND = 'vulkan' }
$log = Join-Path $Out 'no-wgpu.log'
if (Select-String -Quiet -Pattern '^graphics: wgpu, ' $log) {
    Write-Output 'no-wgpu: the runner has a Vulkan driver, so wgpu started'
} elseif ($kind -eq 'app') {
    if (-not (Select-String -Quiet -Pattern '^graphics: OpenGL, ' $log)) { throw 'no-wgpu: expected OpenGL' }
} elseif ($process.MainWindowTitle -ne 'CloakFrame cannot start') {
    throw "no-wgpu: unexpected window '$($process.MainWindowTitle)'"
}
$null = $process.CloseMainWindow()
if (-not $process.WaitForExit(30000)) { throw 'no-wgpu: the app did not close' }
Write-Output "no-wgpu: ended with $($process.ExitCode)"
