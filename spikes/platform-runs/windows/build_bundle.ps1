# Builds the portable M0 bundle on a Windows development machine. See docs/M0_PLATFORM_RUNBOOK.md.
#
#   powershell -ExecutionPolicy Bypass -File spikes\platform-runs\windows\build_bundle.ps1 [-Out <dir>] [-FfmpegDir <dir>]
#
# Needs: Rust (x86_64-pc-windows-msvc), Git for Windows (its bash runs fetch_fonts.sh), Python 3.11+.
# The bundle runs on a machine with nothing installed: executables use the static CRT, and
# ONNX Runtime/DirectML come from the Windows ML 2.4.89 package next to them.
param(
    [string]$Out = (Join-Path (Get-Location) "m0-windows-bundle"),
    [string]$FfmpegDir = ""
)
$ErrorActionPreference = "Stop"
$repo = (git -C $PSScriptRoot rev-parse --show-toplevel).Trim()
$work = Join-Path $Out "_work"
New-Item -ItemType Directory -Force $Out, $work | Out-Null

function Fetch([string]$Url, [string]$Path, [string]$Sha256) {
    if (-not (Test-Path $Path)) {
        Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $Path
    }
    $actual = (Get-FileHash -Algorithm SHA256 $Path).Hash.ToLowerInvariant()
    if ($actual -ne $Sha256) { throw "hash mismatch for $Path ($actual)" }
}

Write-Host "== Windows ML 2.4.89 (ORT 1.27.1, DirectML 1.15.4)"
$nupkg = Join-Path $work "wml.nupkg"
Fetch "https://api.nuget.org/v3-flatcontainer/microsoft.windows.ai.machinelearning/2.4.89/microsoft.windows.ai.machinelearning.2.4.89.nupkg" `
    $nupkg "5c68ecfb947223267abf159a023f5192ad42725e4e9cc995e7c1470dd54dff63"
$extract = Join-Path $work "wml"
if (-not (Test-Path $extract)) {
    Copy-Item $nupkg "$nupkg.zip"
    Expand-Archive "$nupkg.zip" $extract
}
Copy-Item (Join-Path $extract "runtimes\win-x64\native\onnxruntime.dll"), (Join-Path $extract "runtimes\win-x64\native\DirectML.dll") $Out
Copy-Item (Join-Path $extract "license.txt") (Join-Path $Out "WindowsML-license.txt")

Write-Host "== models and images"
$assets = Join-Path $Out "models"
New-Item -ItemType Directory -Force $assets | Out-Null
$spec = Get-Content -Raw (Join-Path $repo "tests\fixtures\assets.json") | ConvertFrom-Json
foreach ($a in @($spec.models) + @($spec.images)) {
    Fetch $a.url (Join-Path $assets $a.name) $a.sha256
}

Write-Host "== OpenCV reference and font tools"
$venv = Join-Path $work "venv"
if (-not (Test-Path $venv)) { python -m venv $venv }
& "$venv\Scripts\python.exe" -m pip -q install "opencv-python-headless==5.0.0.93" "numpy<3" "fonttools==4.67.0"
$images = @("astronaut.png", "messi5.jpg", "lena.jpg", "car_image.webp" | ForEach-Object { Join-Path $assets $_ })
$images += Join-Path $repo "tests\fixtures\detect\inputs\astronaut-scales.png"
$images += Join-Path $repo "tests\fixtures\detect\inputs\crowd-280-faces.png"
& "$venv\Scripts\python.exe" -I (Join-Path $repo "spikes\ort\reference\opencv_reference.py") (Join-Path $Out "ref") @images
if ($LASTEXITCODE -ne 0) { throw "opencv_reference.py failed" }

Write-Host "== fixtures"
$fixtures = Join-Path $Out "fixtures"
New-Item -ItemType Directory -Force $fixtures | Out-Null
Copy-Item -Recurse -Force (Join-Path $repo "tests\fixtures\detect") $fixtures

Write-Host "== build (static CRT)"
$env:RUSTFLAGS = "-C target-feature=+crt-static"
$bash = Join-Path (Split-Path (Split-Path (Get-Command git).Source)) "bin\bash.exe"
$env:PATH = "$venv\Scripts;$env:PATH"
Push-Location (Join-Path $repo "spikes\egui"); & $bash ./fetch_fonts.sh; if ($LASTEXITCODE -ne 0) { throw "fetch_fonts.sh failed" }; Pop-Location
foreach ($spike in "ort", "ffmpeg-pipe", "egui") {
    Push-Location (Join-Path $repo "spikes\$spike")
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed in $spike" }
    Pop-Location
}
Copy-Item (Join-Path $repo "spikes\ort\target\release\ort-spike.exe") $Out
Copy-Item (Join-Path $repo "spikes\ffmpeg-pipe\target\release\ffmpeg-pipe-spike.exe") $Out
Copy-Item (Join-Path $repo "spikes\egui\target\release\egui-spike.exe") $Out
Copy-Item (Join-Path $PSScriptRoot "run_m0_windows.ps1") $Out

if ($FfmpegDir) {
    Copy-Item (Join-Path $FfmpegDir "ffmpeg.exe"), (Join-Path $FfmpegDir "ffprobe.exe") $Out
}

Write-Host "== check: no VC++ runtime imports"
$dumpbin = Get-Command dumpbin -ErrorAction SilentlyContinue
foreach ($exe in Get-ChildItem $Out -Filter *.exe) {
    if ($dumpbin) {
        $imports = & dumpbin /dependents $exe.FullName | Select-String -Pattern "VCRUNTIME|MSVCP"
        if ($imports) { throw "$($exe.Name) still imports the VC++ runtime: $imports" }
    }
}

Get-ChildItem $Out -File | Get-FileHash -Algorithm SHA256 |
    ForEach-Object { "{0}  {1}" -f $_.Hash.ToLowerInvariant(), (Split-Path -Leaf $_.Path) } |
    Set-Content (Join-Path $Out "bundle-sha256.txt")
Remove-Item -Recurse -Force $work
Write-Host "bundle ready: $Out"
