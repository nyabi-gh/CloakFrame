# M0 checks on Windows, run from the bundle made by build_bundle.ps1. Works in Windows PowerShell 5.1
# (Windows 10 1903) without network or anything installed. See docs/M0_PLATFORM_RUNBOOK.md.
#
#   powershell -ExecutionPolicy Bypass -File .\run_m0_windows.ps1 [-FallbackSeconds 60] [-ForceTdr] [-SkipGui] [-SkipFfmpeg]
#
# -ForceTdr runs "dxcap -forcetdr" during the fallback test (needs an elevated window and the
# optional "Graphics Tools" feature). Without it the script waits and tells you when to run it.
param(
    [int]$FallbackSeconds = 60,
    [switch]$ForceTdr,
    [switch]$SkipGui,
    [switch]$SkipFfmpeg
)
$ErrorActionPreference = "Stop"
$bundle = $PSScriptRoot
$stamp = Get-Date -Format "yyyyMMdd-HHmm"
$out = Join-Path $bundle ("results-{0}-{1}" -f $env:COMPUTERNAME, $stamp)
New-Item -ItemType Directory -Force $out | Out-Null
$summary = Join-Path $out "summary.txt"
function Log([string]$text) { $text | Tee-Object -FilePath $summary -Append | Write-Host }

$ort = Join-Path $bundle "onnxruntime.dll"
$spike = Join-Path $bundle "ort-spike.exe"
$models = Join-Path $bundle "models"
$fixtures = Join-Path $bundle "fixtures"
$decoded = Join-Path $bundle "ref\decoded"

Log "== system"
$os = Get-CimInstance Win32_OperatingSystem
Log ("{0} {1} build {2}" -f $os.Caption, $os.Version, $os.BuildNumber)
Log ("CPU: {0}" -f (Get-CimInstance Win32_Processor | Select-Object -First 1).Name)
foreach ($gpu in Get-CimInstance Win32_VideoController) {
    Log ("GPU: {0}, driver {1}" -f $gpu.Name, $gpu.DriverVersion)
}
$vc = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64" -ErrorAction SilentlyContinue
Log ("VC++ 2015-2022 runtime installed: {0}" -f $(if ($vc) { $vc.Version } else { "no" }))
$online = Test-Connection -ComputerName 1.1.1.1 -Count 1 -Quiet -ErrorAction SilentlyContinue
Log ("network reachable: {0}" -f $online)

function Run-Spike([string]$Label, [string[]]$Extra) {
    $file = Join-Path $out "$Label.txt"
    & $spike $models $fixtures $decoded $ort @Extra *> $file
    $code = $LASTEXITCODE
    Log ("-- {0} (exit {1})" -f $Label, $code)
    Get-Content $file | Select-String -Pattern "^ORT|SUMMARY|FALLBACK|event|sessions|latency|Error|differing bytes [1-9]" |
        ForEach-Object { Log $_.Line }
}

Log "== INTER_LINEAR port vs OpenCV 5.0 (x86)"
foreach ($ipp in "on", "off") {
    $line = & $spike resize-check (Join-Path $bundle "ref\resize-ipp-$ipp")
    Log $line
}

Log "== ORT (Windows ML 2.4.89) vs 1.12.1 fixtures"
Run-Spike "ort-cpu" @()
Run-Spike "ort-dml" @("--dml")

Log "== loaded runtime and network activity (DirectML session alive for 30 s)"
$proc = Start-Process -FilePath $spike -ArgumentList @("`"$models`"", "`"$fixtures`"", "`"$decoded`"", "`"$ort`"", "--dml", "--linger", "30") `
    -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $out "ort-dml-linger.txt") -RedirectStandardError (Join-Path $out "ort-dml-linger.err")
$sockets = @()
for ($i = 0; $i -lt 60 -and -not $proc.HasExited; $i++) {
    Start-Sleep -Milliseconds 500
    $sockets += @(Get-NetTCPConnection -OwningProcess $proc.Id -ErrorAction SilentlyContinue | ForEach-Object { "tcp {0}:{1} {2}" -f $_.RemoteAddress, $_.RemotePort, $_.State })
    $sockets += @(Get-NetUDPEndpoint -OwningProcess $proc.Id -ErrorAction SilentlyContinue | ForEach-Object { "udp {0}:{1}" -f $_.LocalAddress, $_.LocalPort })
    if ($i -eq 20) {
        $mods = (Get-Process -Id $proc.Id).Modules | Where-Object { $_.ModuleName -match "onnxruntime|DirectML" }
        foreach ($m in $mods) { Log ("module: {0}" -f $m.FileName) }
    }
}
$proc.WaitForExit()
$unique = $sockets | Sort-Object -Unique
Log ("sockets owned by the process during the run: {0}" -f @($unique).Count)
$unique | ForEach-Object { Log "  $_" }

Log "== GPU failure during a run: same input again on CPU"
Run-Spike "fallback-simulated" @("--dml", "--fallback-test", "10", "--simulate-failure-after", "5")
$dxcap = Get-Command dxcap -ErrorAction SilentlyContinue
$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$file = Join-Path $out "fallback-tdr.txt"
$proc = Start-Process -FilePath $spike -ArgumentList @("`"$models`"", "`"$fixtures`"", "`"$decoded`"", "`"$ort`"", "--dml", "--fallback-test", "$FallbackSeconds") `
    -PassThru -NoNewWindow -RedirectStandardOutput $file -RedirectStandardError "$file.err"
Start-Sleep -Seconds 15
if ($ForceTdr -and $dxcap -and $admin) {
    Log "forcing a GPU reset with dxcap -forcetdr"
    & dxcap -forcetdr
} else {
    Log ("Run 'dxcap -forcetdr' in an elevated window within the next {0} s (dxcap found: {1}, elevated: {2})" -f ($FallbackSeconds - 15), [bool]$dxcap, $admin)
}
$proc.WaitForExit()
Get-Content $file | Select-String -Pattern "event|FALLBACK|Error" | ForEach-Object { Log $_.Line }

if (-not $SkipFfmpeg) {
    Log "== FFmpeg pipe throughput"
    $ffmpeg = Get-Command ffmpeg -ErrorAction SilentlyContinue
    if (-not $ffmpeg -and (Test-Path (Join-Path $bundle "ffmpeg.exe"))) {
        $env:PATH = "$bundle;$env:PATH"
        $ffmpeg = Get-Command ffmpeg
    }
    if (-not $ffmpeg) {
        Log "ffmpeg not found; skipped"
    } else {
        $encoders = (& ffmpeg -hide_banner -encoders 2>$null) -join "`n"
        $hw = @()
        foreach ($e in "h264_nvenc", "hevc_nvenc", "h264_qsv", "hevc_qsv", "h264_amf", "hevc_amf") {
            if ($encoders -match "\b$e\b") { $hw += $e }
        }
        Log ("hardware encoders listed by ffmpeg: {0}" -f ($hw -join ", "))
        $ff = Join-Path $bundle "ffmpeg-pipe-spike.exe"
        $clips = Join-Path $out "ffpipe"
        & $ff gen $clips | Out-Null
        & $ff bench $clips --hw-decode d3d11va --hw-encoders ($hw -join ",") *> (Join-Path $out "ffmpeg-pipe.jsonl")
        Get-Content (Join-Path $out "ffmpeg-pipe.jsonl") | ForEach-Object {
            try {
                $d = $_ | ConvertFrom-Json
                Log ("{0,-13} {1,-64} {2,7:N1} fps x{3:N2}" -f $d.clip, $d.case, $d.fps, $d.realtime_x)
            } catch { Log $_ }
        }
        Remove-Item -Recurse -Force (Join-Path $clips "out") -ErrorAction SilentlyContinue
    }
}

if (-not $SkipGui) {
    Log "== egui benchmarks"
    $egui = Join-Path $bundle "egui-spike.exe"
    foreach ($renderer in "wgpu", "glow") {
        foreach ($bench in "upload", "zoom") {
            $json = Join-Path $out "egui-$renderer-$bench.json"
            & $egui --renderer $renderer --bench $bench --frames 300 > $json 2> "$json.err"
            Log ("{0} {1} exit {2}: {3}" -f $renderer, $bench, $LASTEXITCODE, ((Get-Content $json -ErrorAction SilentlyContinue) -join " ").Substring(0, [Math]::Min(300, ((Get-Content $json -ErrorAction SilentlyContinue) -join " ").Length)))
        }
    }
    Log "manual egui checks: see docs/M0_PLATFORM_RUNBOOK.md (run .\egui-spike.exe)"
}

Compress-Archive -Force -Path $out -DestinationPath "$out.zip"
Log "results: $out.zip"
