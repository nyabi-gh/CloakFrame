# The Windows x64 Velopack installer (plan 10). Photos only until FFmpeg is built from source
# (plan 10.1). Needs Rust, Python, cargo-about, the .NET 10 runtime and the Windows SDK's rc.exe.
#
# Usage: tools/package/windows.ps1 <out>
# Writes the installer, the portable zip, the full package and releases.win.json to <out> and
# prints the installer's path.
param(
    [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

$root = Resolve-Path "$PSScriptRoot/../.."
$target = 'x86_64-pc-windows-msvc'
$native = Join-Path ([IO.Path]::GetTempPath()) 'cloakframe-native'

$metadata = cargo metadata --no-deps --format-version 1 --manifest-path "$root/Cargo.toml" | ConvertFrom-Json
$version = ($metadata.packages | Where-Object name -EQ 'cloakframe-app').version

# Without the static CRT the app does not start on a PC lacking the VC++ runtime (docs/M0_ORT_SPIKE.md).
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --release --locked -p cloakframe-app --target $target --manifest-path "$root/Cargo.toml"
$exe = "$root/target/$target/release/cloakframe.exe"
if ([Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($exe)) -match 'VCRUNTIME140') {
    throw "$exe still imports the VC++ runtime"
}

$ort = python -I "$root/tools/fetch_native.py" windows-ml windows-x64 $native
$ortRoot = Resolve-Path "$(Split-Path $ort)/../../.."
$vpkRoot = python -I "$root/tools/fetch_native.py" vpk windows-x64 $native

$stage = Join-Path ([IO.Path]::GetTempPath()) "cloakframe-package-$PID"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory $stage | Out-Null
try {
    Copy-Item $exe, $ort, "$(Split-Path $ort)/DirectML.dll" $stage
    Copy-Item "$root/LICENSE", "$root/THIRD_PARTY_NOTICES.txt" $stage
    Copy-Item "$ortRoot/license.txt" "$stage/WINDOWS_ML_LICENSE.txt"
    Copy-Item "$ortRoot/ThirdPartyNotices.txt" "$stage/WINDOWS_ML_THIRD_PARTY_NOTICES.txt"
    cargo about generate --locked --manifest-path "$root/crates/cloakframe-app/Cargo.toml" `
        --config "$root/tools/about/about.toml" --target $target `
        --output-file "$stage/RUST_CRATE_NOTICES.txt" "$root/tools/about/notices.hbs"

    New-Item -ItemType Directory -Force $Out | Out-Null
    # packId and packTitle differ from 1.12.1's CloakFrame so that neither replaces or removes the
    # other's folder and shortcuts (docs/M7_WINDOWS.md). 10.0.18362 is Windows 10 1903.
    dotnet "$vpkRoot/tools/net10.0/any/vpk.dll" --yes --skip-updates pack `
        --packId CloakFrameApp `
        --packVersion $version `
        --packDir $stage `
        --mainExe cloakframe.exe `
        --packTitle 'CloakFrame 2' `
        --packAuthors Nyabi `
        --icon "$root/assets/icons/cloakframe.ico" `
        --runtime win10.0.18362-x64 `
        --channel win `
        --delta None `
        --outputDir $Out | Out-Host
} finally {
    Remove-Item -Recurse -Force $stage
}
Resolve-Path "$Out/CloakFrameApp-win-Setup.exe"
