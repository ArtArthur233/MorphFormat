<#
.SYNOPSIS
  Builds Morph-Format (features heic,video) and produces the NSIS installer in dist/.
.PARAMETER FfmpegDir  vcpkg x64-windows tree holding the LGPL FFmpeg (bin, lib, include, share).
.PARAMETER HeifDir    vcpkg x64-windows tree holding libheif + libde265.
.PARAMETER VcpkgRoot  Small vcpkg root read by libheif-sys: .vcpkg-root, installed\vcpkg, installed\x64-windows (= HeifDir). See docs/packaging.md.
.PARAMETER LibClang   Folder with libclang.dll (bindgen).
.PARAMETER SkipBuild  Reuse the existing target\release\morph-format.exe.
#>
param(
    [string]$DepsDir = $(if ($env:MORPH_DEPS) { $env:MORPH_DEPS } else { Join-Path $env:USERPROFILE 'Desktop\dev\morph-deps' }),
    [string]$FfmpegDir = (Join-Path $DepsDir 'ffmpeg'),
    [string]$HeifDir = (Join-Path $DepsDir 'heif'),
    [string]$VcpkgRoot = (Join-Path $DepsDir 'vcpkg-root'),
    [string]$LibClang = 'C:\Program Files\LLVM\bin',
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Set-Location $repo

function Run([scriptblock]$cmd) { & $cmd; if ($LASTEXITCODE -ne 0) { throw "Failed: $cmd" } }

foreach ($d in $FfmpegDir, $HeifDir) { if (-not (Test-Path (Join-Path $d 'bin'))) { throw "Dependency folder not found: $d" } }
if (-not (Get-Command cargo-packager -ErrorAction SilentlyContinue)) { throw 'cargo-packager missing: cargo install cargo-packager --locked' }

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
if (-not (Select-String -Path Packager.toml -Pattern "^version\s*=\s*`"$([regex]::Escape($version))`"" -Quiet)) {
    throw "Packager.toml: version differs from Cargo.toml ($version)"
}

# --- build ---
if (-not $SkipBuild) {
    $env:FFMPEG_DIR = (Resolve-Path $FfmpegDir).Path
    $env:LIBCLANG_PATH = $LibClang
    $env:VCPKGRS_DYNAMIC = '1'
    if (-not (Test-Path (Join-Path $VcpkgRoot '.vcpkg-root'))) { throw "Invalid VcpkgRoot: $VcpkgRoot (see docs/packaging.md)" }
    $env:VCPKG_ROOT = (Resolve-Path $VcpkgRoot).Path
    # The DLLs must be findable at build time (build scripts probe them)
    $env:PATH = "$(Join-Path $env:FFMPEG_DIR 'bin');$(Join-Path (Resolve-Path $HeifDir).Path 'bin');$env:PATH"
    Run { cargo build --release -p morph-app --features heic,video }
}
$exe = Join-Path $repo 'target\release\morph-format.exe'
if (-not (Test-Path $exe)) { throw "Not found: $exe" }

# --- collect into dist/app ---
$dist = Join-Path $repo 'dist'
$app = Join-Path $dist 'app'
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $app 'licences') | Out-Null
Copy-Item $exe $app

# Release DLLs only (no debug\, no .pdb); pkgconf is a vcpkg build tool, not a dependency.
foreach ($d in $FfmpegDir, $HeifDir) {
    Get-ChildItem (Join-Path $d 'bin') -Filter *.dll | Where-Object { $_.Name -notlike 'pkgconf*' } | Copy-Item -Destination $app -Force
}

$lic = Join-Path $app 'licences'
Copy-Item LICENSE-MIT, LICENSE-APACHE, THIRD-PARTY-NOTICES.md -Destination $lic
Copy-Item (Join-Path $PSScriptRoot 'licences\*') -Destination $lic
foreach ($p in @(@($FfmpegDir, 'ffmpeg'), @($FfmpegDir, 'libvpx'), @($FfmpegDir, 'aom'), @($FfmpegDir, 'dav1d'), @($FfmpegDir, 'opus'), @($HeifDir, 'libheif'), @($HeifDir, 'libde265'))) {
    $f = Join-Path $p[0] "share\$($p[1])\copyright"
    if (Test-Path $f) { Copy-Item $f (Join-Path $lic "$($p[1])-copyright.txt") }
}

# Every DLL imported (transitively) by the exe must be in dist/app, else the installed app would not start.
$dumpbin = Get-Command dumpbin -ErrorAction SilentlyContinue
if ($dumpbin) {
    $have = (Get-ChildItem $app -Filter *.dll).Name
    $seen = @{}; $todo = New-Object System.Collections.Queue; $todo.Enqueue((Join-Path $app 'morph-format.exe'))
    while ($todo.Count) {
        $f = $todo.Dequeue()
        foreach ($l in (& dumpbin /dependents $f)) {
            if ($l -match '^\s+(\S+\.dll)\s*$') {
                $n = $Matches[1].ToLower()
                if ($seen[$n]) { continue }; $seen[$n] = $true
                $local = $have | Where-Object { $_.ToLower() -eq $n }
                if ($local) { $todo.Enqueue((Join-Path $app $local)) }
            }
        }
    }
    $unused = $have | Where-Object { -not $seen[$_.ToLower()] }
    if ($unused) { Write-Warning "Unused DLLs (remove them?): $($unused -join ', ')" }
    $missing = $seen.Keys | Where-Object { -not ($have | Where-Object { $_.ToLower() -eq $_ }) -and -not (Test-Path (Join-Path $env:SystemRoot "System32\$_")) -and $_ -notlike 'api-ms-*' -and $_ -notlike 'vcruntime*' -and $_ -notlike 'msvcp*' }
    if ($missing) { Write-Warning "DLLs not shipped (check they are system DLLs): $($missing -join ', ')" }
}
else { Write-Warning 'dumpbin not found (use a VS developer prompt): DLL check skipped.' }

# --- installer ---
Run { cargo packager -c Packager.toml }
Move-Item (Join-Path $dist "morph-format_${version}_x64-setup.exe") (Join-Path $dist "Morph-Format-$version-windows-x64-setup.exe")
Get-ChildItem $dist -File | Select-Object Name, @{n = 'MB'; e = { [math]::Round($_.Length / 1MB, 1) } }
