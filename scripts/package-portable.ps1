#Requires -Version 7
<#
.SYNOPSIS
  Assemble the Windows x64 portable zip for a Chromora version tag.

.DESCRIPTION
  Verifies the bundled native binaries, copies the release exe, libraw.dll,
  official MSVC x64 CRT DLLs from the local Visual Studio installation, third-party
  licenses, Rust dependency notices, and the film-style pack, then writes
  SHA256SUMS.txt and a UTF-8 zip.

  When scripts/verify-native-hashes.ps1 is present (owned by the CI change), this
  script calls it. It does not add or replace that shared verifier. Until that
  file exists, the two binaries this package builds and ships are checked here.

  This script does not code-sign.
#>
[CmdletBinding(DefaultParameterSetName = "Package")]
param(
    [Parameter(ParameterSetName = "Package", Mandatory = $true)]
    [string]$Version,

    [Parameter(ParameterSetName = "Verify")]
    [switch]$VerifyOnly,

    [string]$CommitSha = "",
    [string]$RepoUrl = "https://github.com/Winsiby/Chromora-Rust"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
# cargo-about writes license warnings to stderr and still exits 0.
$PSNativeCommandUseErrorActionPreference = $false

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

# Hashes recorded in native/libraw/SOURCE.md and native/jpeg/SOURCE.md.
$LibrawDllSha256 = "cbd9f18eb43d23d8214fcfaafb77feb3343af227b71e502e033146bdd4e88261"
$JpegStaticSha256 = "304077f3cf159373c55b4f3b4c933b08b279061bebd7fbafba4707b0ff2bb776"

$CrtDllNames = @(
    "concrt140.dll",
    "msvcp140.dll",
    "msvcp140_1.dll",
    "msvcp140_2.dll",
    "msvcp140_atomic_wait.dll",
    "msvcp140_codecvt_ids.dll",
    "vccorlib140.dll",
    "vcruntime140.dll",
    "vcruntime140_1.dll",
    "vcruntime140_threads.dll"
)

$StyleDisclaimerLines = @(
    "01–08为八款近似风格，00为撤去本包风格；它们不代表原厂官方滤镜或逐像素复刻。",
    "本次为 Chromora 自行编写的参数，非官方配方或官方 LUT。",
    "这些是近似参数风格，不是厂商官方胶片模拟。"
)

function Write-Utf8File {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Content,
        [bool]$Bom = $false,
        [bool]$CrLf = $false
    )
    $text = $Content.Replace("`r`n", "`n").Replace("`r", "`n")
    if (-not $text.EndsWith("`n")) {
        $text += "`n"
    }
    if ($CrLf) {
        $text = $text.Replace("`n", "`r`n")
    }
    $encoding = New-Object System.Text.UTF8Encoding $Bom
    [System.IO.File]::WriteAllText($Path, $text, $encoding)
}

function Assert-FileSha256 {
    param(
        [Parameter(Mandatory = $true)][string]$RelativePath,
        [Parameter(Mandatory = $true)][string]$Expected
    )
    $path = Join-Path $Root ($RelativePath.Replace("/", [IO.Path]::DirectorySeparatorChar))
    if (-not (Test-Path -LiteralPath $path)) {
        throw "Missing native file $RelativePath"
    }
    $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $Expected.ToLowerInvariant()) {
        throw "SHA-256 mismatch for $RelativePath`n  expected $Expected`n  actual   $actual"
    }
    Write-Host "OK $RelativePath"
}

function Assert-NativeHashes {
    $shared = Join-Path $Root "scripts/verify-native-hashes.ps1"
    if (Test-Path -LiteralPath $shared) {
        Write-Host "Using shared native hash verifier: scripts/verify-native-hashes.ps1"
        $global:LASTEXITCODE = 0
        & $shared
        if ($LASTEXITCODE -ne 0) {
            throw "scripts/verify-native-hashes.ps1 failed with exit code $LASTEXITCODE"
        }
        return
    }

    Write-Host "scripts/verify-native-hashes.ps1 is not in this checkout."
    Write-Host "Checking libraw.dll and jpeg-static.lib against SOURCE.md."
    Assert-FileSha256 -RelativePath "native/libraw/bin/libraw.dll" -Expected $LibrawDllSha256
    Assert-FileSha256 -RelativePath "native/jpeg/jpeg-static.lib" -Expected $JpegStaticSha256
}

function Copy-ByteForByte {
    param(
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Destination
    )
    $parent = Split-Path -Parent $Destination
    if ($parent) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    Copy-Item -LiteralPath $Source -Destination $Destination
    $before = (Get-FileHash -LiteralPath $Source -Algorithm SHA256).Hash
    $after = (Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash
    if ($before -ne $after) {
        throw "Copy changed the bytes of $(Split-Path -Leaf $Source)"
    }
}

function Get-VisualStudioInstallPaths {
    $found = New-Object System.Collections.Generic.List[string]
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path -LiteralPath $vswhere) {
        $installed = & $vswhere -all -products * -property installationPath
        foreach ($entry in @($installed)) {
            $trimmed = "$entry".Trim()
            if ($trimmed -and (Test-Path -LiteralPath $trimmed)) {
                $found.Add($trimmed)
            }
        }
    }
    foreach ($programFiles in @(${env:ProgramFiles}, ${env:ProgramFiles(x86)})) {
        if (-not $programFiles) { continue }
        $vsRoot = Join-Path $programFiles "Microsoft Visual Studio"
        if (-not (Test-Path -LiteralPath $vsRoot)) { continue }
        foreach ($year in Get-ChildItem -LiteralPath $vsRoot -Directory) {
            foreach ($edition in Get-ChildItem -LiteralPath $year.FullName -Directory) {
                $found.Add($edition.FullName)
            }
        }
    }
    return @($found | Select-Object -Unique)
}

function Get-CrtSortKey {
    param([Parameter(Mandatory = $true)][string]$Directory)
    # ...\VC\Redist\MSVC\<version>\x64\Microsoft.VC###.CRT
    $versionFolder = Split-Path -Leaf (Split-Path -Parent (Split-Path -Parent $Directory))
    $numbers = @()
    foreach ($part in ($versionFolder -split "\.")) {
        if ($part -match "^\d+$") {
            $numbers += [int]$part
        }
        else {
            $numbers += -1
        }
    }
    while ($numbers.Count -lt 4) { $numbers += 0 }
    $toolset = 0
    $leaf = Split-Path -Leaf $Directory
    if ($leaf -match "VC(\d+)") {
        $toolset = [int]$Matches[1]
    }
    return [PSCustomObject]@{
        Directory = $Directory
        Major     = $numbers[0]
        Minor     = $numbers[1]
        Build     = $numbers[2]
        Revision  = $numbers[3]
        Toolset   = $toolset
    }
}

function Find-OfficialCrtDirectory {
    $candidates = New-Object System.Collections.Generic.List[string]
    $installs = @(Get-VisualStudioInstallPaths)
    if ($installs.Count -eq 0) {
        throw "No Visual Studio installation was found. Official VC\Redist CRT DLLs are required; this script does not build substitute runtime DLLs."
    }
    foreach ($install in $installs) {
        $redist = Join-Path $install "VC\Redist\MSVC"
        if (-not (Test-Path -LiteralPath $redist)) {
            Write-Host "No VC Redist under $install"
            continue
        }
        foreach ($versionDir in Get-ChildItem -LiteralPath $redist -Directory) {
            $x64 = Join-Path $versionDir.FullName "x64"
            if (-not (Test-Path -LiteralPath $x64)) { continue }
            foreach ($crtDir in Get-ChildItem -LiteralPath $x64 -Directory) {
                $name = $crtDir.Name
                if ($name -notmatch "^Microsoft\.VC\d+\.CRT$") { continue }
                if ($name -match "Debug") { continue }
                $candidates.Add($crtDir.FullName)
            }
        }
    }
    if ($candidates.Count -eq 0) {
        throw "No x64 Microsoft.VC*.CRT directory was found under VC\Redist. Refusing to invent CRT DLLs."
    }

    $ordered = @(
        $candidates |
            ForEach-Object { Get-CrtSortKey $_ } |
            Sort-Object -Property Major, Minor, Build, Revision, Toolset -Descending
    )
    $rejected = New-Object System.Collections.Generic.List[string]
    foreach ($candidate in $ordered) {
        $missing = @()
        foreach ($dll in $CrtDllNames) {
            $path = Join-Path $candidate.Directory $dll
            if (-not (Test-Path -LiteralPath $path)) {
                $missing += $dll
            }
        }
        if ($missing.Count -gt 0) {
            $rejected.Add("$($candidate.Directory) missing $($missing -join ', ')")
            continue
        }
        $unofficial = @()
        foreach ($dll in $CrtDllNames) {
            $path = Join-Path $candidate.Directory $dll
            $info = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($path)
            $company = "$($info.CompanyName)"
            if ($company -notmatch "Microsoft") {
                $unofficial += "$dll company '$company'"
            }
            if (-not $info.FileVersion) {
                $unofficial += "$dll has no file version"
            }
            if ((Get-Item -LiteralPath $path).Length -le 0) {
                $unofficial += "$dll is empty"
            }
        }
        if ($unofficial.Count -gt 0) {
            $rejected.Add("$($candidate.Directory): $($unofficial -join '; ')")
            continue
        }
        return $candidate.Directory
    }

    $detail = $rejected -join [Environment]::NewLine
    throw "Found VC Redist directories, but none contained the unmodified official x64 CRT set.`n$detail"
}

function Get-RedistributionLinks {
    param([Parameter(Mandatory = $true)][string]$CrtDirectory)
    $links = @(
        "https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files"
    )
    $leaf = Split-Path -Leaf $CrtDirectory
    if ($leaf -match "VC145" -or $CrtDirectory -match "[\\/]2026[\\/]" -or $CrtDirectory -match "[\\/]18[\\/]") {
        $links += "https://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution"
    }
    elseif ($leaf -match "VC143" -or $CrtDirectory -match "[\\/]2022[\\/]") {
        $links += "https://learn.microsoft.com/en-us/visualstudio/releases/2022/redistribution"
    }
    elseif ($leaf -match "VC142" -or $CrtDirectory -match "[\\/]2019[\\/]") {
        $links += "https://learn.microsoft.com/en-us/visualstudio/releases/2019/redistribution"
    }
    $links += "https://learn.microsoft.com/en-us/cpp/windows/choosing-a-deployment-method?view=msvc-170"
    return $links
}

function Write-CrtProvenance {
    param(
        [Parameter(Mandatory = $true)][string]$CrtDirectory,
        [Parameter(Mandatory = $true)][string]$Destination
    )
    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add("# 随应用部署的 VC 运行库")
    $lines.Add("")
    $lines.Add("以下 DLL 与 chromora.exe、libraw.dll 放在同一目录。它们从本构建机的 Visual Studio 安装目录原样复制，没有修改，也不是另行编译或替换的运行库。")
    $lines.Add("")
    $lines.Add("- 来源目录：``$CrtDirectory``")
    $lines.Add("- 目录名：``$(Split-Path -Leaf $CrtDirectory)``")
    $lines.Add("")
    $lines.Add("| 文件 | 文件版本 | 产品版本 | 公司 | SHA-256 |")
    $lines.Add("| --- | --- | --- | --- | --- |")
    foreach ($dll in $CrtDllNames) {
        $path = Join-Path $CrtDirectory $dll
        $info = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($path)
        $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        $fileVersion = "$($info.FileVersion)".Replace("|", "/")
        $productVersion = "$($info.ProductVersion)".Replace("|", "/")
        $company = "$($info.CompanyName)".Replace("|", "/")
        $lines.Add("| $dll | $fileVersion | $productVersion | $company | $hash |")
    }
    $lines.Add("")
    $lines.Add("Microsoft 的许可允许随程序分发 ``VC\Redist`` 中未经修改的文件。再分发条款：")
    $lines.Add("")
    foreach ($link in Get-RedistributionLinks -CrtDirectory $CrtDirectory) {
        $lines.Add("- $link")
    }
    $lines.Add("")
    $lines.Add("干净 Windows 上的加载仍需单独验收。")
    Write-Utf8File -Path $Destination -Content ($lines -join "`n") -Bom $false -CrLf $true
}

function Copy-ThirdPartyLicenses {
    param([Parameter(Mandatory = $true)][string]$Stage)
    $pairs = @(
        @{ Source = "native/libraw/COPYRIGHT"; Dest = "third-party/LibRaw/COPYRIGHT" },
        @{ Source = "native/libraw/LICENSE.CDDL"; Dest = "third-party/LibRaw/LICENSE.CDDL" },
        @{ Source = "native/libraw/LICENSE.LGPL"; Dest = "third-party/LibRaw/LICENSE.LGPL" },
        @{ Source = "native/libraw/LICENSE.X3FTOOLS"; Dest = "third-party/LibRaw/LICENSE.X3FTOOLS" },
        @{ Source = "native/libraw/jpeg-licenses/LICENSE.md"; Dest = "third-party/libjpeg-turbo/LICENSE.md" },
        @{ Source = "native/libraw/jpeg-licenses/README.ijg"; Dest = "third-party/libjpeg-turbo/README.ijg" }
    )
    foreach ($pair in $pairs) {
        $source = Join-Path $Root ($pair.Source.Replace("/", [IO.Path]::DirectorySeparatorChar))
        if (-not (Test-Path -LiteralPath $source)) {
            throw "Missing third-party license $($pair.Source)"
        }
        $dest = Join-Path $Stage ($pair.Dest.Replace("/", [IO.Path]::DirectorySeparatorChar))
        Copy-ByteForByte -Source $source -Destination $dest
    }
}

function Copy-TreeByteForByte {
    param(
        [Parameter(Mandatory = $true)][string]$SourceDir,
        [Parameter(Mandatory = $true)][string]$DestinationDir
    )
    $files = @(Get-ChildItem -LiteralPath $SourceDir -Recurse -File)
    if ($files.Count -eq 0) {
        throw "No files to copy from $SourceDir"
    }
    foreach ($file in $files) {
        $relative = $file.FullName.Substring($SourceDir.Length).TrimStart("\", "/")
        $dest = Join-Path $DestinationDir $relative
        Copy-ByteForByte -Source $file.FullName -Destination $dest
    }
}

function Write-RustNotices {
    param(
        [Parameter(Mandatory = $true)][string]$Destination,
        [Parameter(Mandatory = $true)][string]$ReleaseVersion,
        [Parameter(Mandatory = $true)][string]$Sha
    )
    $aboutVersion = (& cargo about --version 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $aboutVersion -notmatch "0\.6\.6") {
        throw "cargo-about 0.6.6 is required to generate Rust dependency notices. Found: $aboutVersion"
    }
    $config = Join-Path $PSScriptRoot "portable/about.toml"
    $template = Join-Path $PSScriptRoot "portable/notices.hbs"
    & cargo about generate `
        --manifest-path (Join-Path $Root "Cargo.toml") `
        --config $config `
        --locked `
        --fail `
        --target x86_64-pc-windows-msvc `
        -o $Destination `
        $template
    if ($LASTEXITCODE -ne 0) {
        throw "cargo-about generate failed with exit code $LASTEXITCODE"
    }
    $body = [System.IO.File]::ReadAllText($Destination)
    foreach ($marker in @("Apache License", "MIT License", "Independent JPEG Group")) {
        if (-not $body.Contains($marker)) {
            throw "Rust dependency notices are missing '$marker'."
        }
    }
    $header = @(
        "Portable release: Chromora $ReleaseVersion Windows x64"
        "Git commit: $Sha"
        "Crate versions below come from Cargo.lock. The chromora crate version in Cargo.toml is not this portable release version."
        ""
    ) -join "`n"
    Write-Utf8File -Path $Destination -Content ($header + $body) -Bom $false -CrLf $false
}

function Write-UsageNotes {
    param(
        [Parameter(Mandatory = $true)][string]$Destination,
        [Parameter(Mandatory = $true)][string]$ReleaseVersion,
        [Parameter(Mandatory = $true)][string]$Sha,
        [bool]$IncludeProjectLicense
    )
    $sourceUrl = "$RepoUrl/tree/$Sha"
    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add("Chromora $ReleaseVersion Windows x64 便携版")
    $lines.Add("")
    $lines.Add("1. 将整个 ZIP 解压到一个可写文件夹。")
    $lines.Add("2. 双击 chromora.exe。保持所有 DLL 和程序在同一个目录。")
    $lines.Add('3. 在应用中导入照片，并在“项目”面板查看本机数据目录。')
    $lines.Add("")
    $lines.Add("源码与使用文档：$sourceUrl")
    $lines.Add("")
    $lines.Add("风格配方随包放在 chromora-film-styles-v1，文件名与仓库 recipes/chromora-film-styles-v1 相同，没有改名。")
    foreach ($disclaimer in $StyleDisclaimerLines) {
        $lines.Add($disclaimer)
    }
    $lines.Add('桌面版可在“风格”中直接打开单个 JSON，或将该文件夹复制到“项目”面板显示的数据目录下的“数据/预设”并刷新预设列表。')
    $lines.Add("")
    $lines.Add("本包包含 LibRaw 0.22.3、libjpeg-turbo 3.1.2 及 Microsoft VC x64 运行库。")
    $lines.Add("许可文本位于 third-party，VC 运行库来源见同目录的运行库来源.md。")
    if ($IncludeProjectLicense) {
        $lines.Add("Chromora 自身的 LICENSE 在本目录。")
    }
    $lines.Add("This software is based in part on the work of the Independent JPEG Group.")
    $lines.Add("LibRaw 和 libjpeg-turbo 的对应源码及构建说明：")
    $lines.Add("$sourceUrl/native/libraw")
    Write-Utf8File -Path $Destination -Content ($lines -join "`n") -Bom $true -CrLf $false
}

function Write-ChecksumList {
    param([Parameter(Mandatory = $true)][string]$Stage)
    $entries = New-Object System.Collections.Generic.List[object]
    foreach ($file in Get-ChildItem -LiteralPath $Stage -Recurse -File) {
        if ($file.Name -eq "SHA256SUMS.txt") { continue }
        $relative = $file.FullName.Substring($Stage.Length).TrimStart("\", "/").Replace("\", "/")
        $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        $entries.Add([PSCustomObject]@{ Relative = $relative; Hash = $hash })
    }
    $sorted = @($entries | Sort-Object -Property Relative)
    $lines = @(foreach ($entry in $sorted) {
        "{0}  {1}" -f $entry.Hash, $entry.Relative
    })
    $path = Join-Path $Stage "SHA256SUMS.txt"
    Write-Utf8File -Path $path -Content ($lines -join "`n") -Bom $false -CrLf $false

    $recorded = @([System.IO.File]::ReadAllLines($path))
    if ($recorded.Count -ne $sorted.Count) {
        throw "SHA256SUMS.txt line count does not match the packed files."
    }
    foreach ($line in $recorded) {
        if ($line -notmatch "^([0-9a-f]{64})  (.+)$") {
            throw "Unreadable SHA256SUMS.txt line: $line"
        }
        $expected = $Matches[1]
        $relative = $Matches[2]
        $full = Join-Path $Stage ($relative.Replace("/", [IO.Path]::DirectorySeparatorChar))
        if (-not (Test-Path -LiteralPath $full)) {
            throw "SHA256SUMS.txt lists a missing file: $relative"
        }
        $actual = (Get-FileHash -LiteralPath $full -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $expected) {
            throw "SHA256SUMS.txt does not match $relative"
        }
    }
}

function Invoke-PythonZip {
    param(
        [Parameter(Mandatory = $true)][string]$Stage,
        [Parameter(Mandatory = $true)][string]$ZipPath
    )
    $python = $null
    $prefix = @()
    if (Get-Command python -ErrorAction SilentlyContinue) {
        $python = (Get-Command python).Source
    }
    elseif (Get-Command python3 -ErrorAction SilentlyContinue) {
        $python = (Get-Command python3).Source
    }
    elseif (Get-Command py -ErrorAction SilentlyContinue) {
        $python = (Get-Command py).Source
        $prefix = @("-3")
    }
    else {
        throw "Python is required to write the portable zip with UTF-8 filenames."
    }
    $zipScript = Join-Path $PSScriptRoot "portable/zip_utf8.py"
    & $python @prefix $zipScript $Stage $ZipPath
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to create $ZipPath"
    }
}

function Assert-ZipEntries {
    param(
        [Parameter(Mandatory = $true)][string]$ZipPath,
        [Parameter(Mandatory = $true)][string]$Stage,
        [Parameter(Mandatory = $true)][string]$StageName,
        [Parameter(Mandatory = $true)][string[]]$Required
    )
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::OpenRead($ZipPath)
    try {
        $names = @($archive.Entries | ForEach-Object { $_.FullName.Replace("\", "/") })
    }
    finally {
        $archive.Dispose()
    }
    foreach ($relative in $Required) {
        $entry = "$StageName/$relative"
        if ($names -notcontains $entry) {
            throw "Portable zip is missing $entry"
        }
    }
    foreach ($file in Get-ChildItem -LiteralPath $Stage -Recurse -File) {
        $relative = $file.FullName.Substring($Stage.Length).TrimStart("\", "/").Replace("\", "/")
        $entry = "$StageName/$relative"
        if ($names -notcontains $entry) {
            throw "Portable zip is missing packed file $entry"
        }
    }
}

Assert-NativeHashes
if ($VerifyOnly) {
    Write-Host "Native hash verification finished."
    exit 0
}

if ($Version -notmatch "^\d+\.\d+\.\d+$") {
    throw "Version must look like 0.10.2, without a leading v. Got: $Version"
}
if (-not $IsWindows) {
    throw "package-portable.ps1 copies official MSVC CRT DLLs and must run on Windows."
}
if (-not $CommitSha) {
    $CommitSha = (& git -C $Root rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or -not $CommitSha) {
        throw "Unable to resolve the git commit. Pass -CommitSha."
    }
}

$exe = Join-Path $Root "target/release/chromora.exe"
if (-not (Test-Path -LiteralPath $exe)) {
    throw "Missing $exe. Run 'cargo build --locked --release' before packaging."
}

$stageName = "Chromora-$Version-Windows-x64"
$dist = Join-Path $Root "dist"
$stage = Join-Path $dist $stageName
$zipPath = Join-Path $dist "$stageName-Portable.zip"
$notesPath = Join-Path $dist "release-body.md"
if (Test-Path -LiteralPath $stage) {
    Remove-Item -LiteralPath $stage -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Copy-ByteForByte -Source $exe -Destination (Join-Path $stage "chromora.exe")
Copy-ByteForByte -Source (Join-Path $Root "native/libraw/bin/libraw.dll") -Destination (Join-Path $stage "libraw.dll")

$crtDirectory = Find-OfficialCrtDirectory
Write-Host "CRT source: $crtDirectory"
foreach ($dll in $CrtDllNames) {
    Copy-ByteForByte -Source (Join-Path $crtDirectory $dll) -Destination (Join-Path $stage $dll)
}
Write-CrtProvenance -CrtDirectory $crtDirectory -Destination (Join-Path $stage "运行库来源.md")

Copy-ThirdPartyLicenses -Stage $stage

$projectLicense = Join-Path $Root "LICENSE"
$includedProjectLicense = Test-Path -LiteralPath $projectLicense
if ($includedProjectLicense) {
    Copy-ByteForByte -Source $projectLicense -Destination (Join-Path $stage "LICENSE")
}

$stylesSource = Join-Path $Root "recipes/chromora-film-styles-v1"
$stylesDest = Join-Path $stage "chromora-film-styles-v1"
if (-not (Test-Path -LiteralPath $stylesSource)) {
    throw "Missing film style pack: recipes/chromora-film-styles-v1"
}
Copy-TreeByteForByte -SourceDir $stylesSource -DestinationDir $stylesDest

$noticesPath = Join-Path $stage "third-party/RUST-DEPENDENCY-NOTICES.txt"
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $noticesPath) | Out-Null
Write-RustNotices -Destination $noticesPath -ReleaseVersion $Version -Sha $CommitSha

$usagePath = Join-Path $stage "使用说明.txt"
Write-UsageNotes -Destination $usagePath -ReleaseVersion $Version -Sha $CommitSha -IncludeProjectLicense $includedProjectLicense
$usageText = [System.IO.File]::ReadAllText($usagePath, (New-Object System.Text.UTF8Encoding $true))
foreach ($disclaimer in $StyleDisclaimerLines) {
    if (-not $usageText.Contains($disclaimer)) {
        throw "使用说明.txt does not include the film-style disclaimer."
    }
}
$provenance = [System.IO.File]::ReadAllText((Join-Path $stage "运行库来源.md"))
if (-not $provenance.Contains($crtDirectory) -or -not $provenance.Contains("https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files")) {
    throw "运行库来源.md is missing the CRT source path or Microsoft redistribution terms."
}

Write-ChecksumList -Stage $stage

if (Test-Path -LiteralPath $zipPath) {
    Remove-Item -LiteralPath $zipPath -Force
}
Invoke-PythonZip -Stage $stage -ZipPath $zipPath

$required = @(
    "chromora.exe",
    "libraw.dll",
    "使用说明.txt",
    "运行库来源.md",
    "SHA256SUMS.txt",
    "third-party/LibRaw/COPYRIGHT",
    "third-party/LibRaw/LICENSE.CDDL",
    "third-party/LibRaw/LICENSE.LGPL",
    "third-party/LibRaw/LICENSE.X3FTOOLS",
    "third-party/libjpeg-turbo/LICENSE.md",
    "third-party/libjpeg-turbo/README.ijg",
    "third-party/RUST-DEPENDENCY-NOTICES.txt",
    "chromora-film-styles-v1/配方目录与来源.json"
) + $CrtDllNames
Assert-ZipEntries -ZipPath $zipPath -Stage $stage -StageName $stageName -Required $required

$zipHash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
$tick = [char]96
$releaseLines = @(
    "适用于 Windows x64 的 Chromora $Version 便携版。",
    "",
    "1. 在 Assets 中下载 ${tick}Chromora-$Version-Windows-x64-Portable.zip${tick}。",
    "2. 解压整个 ZIP，保留程序与同目录 DLL。",
    "3. 双击 ${tick}chromora.exe${tick} 启动。",
    "",
    "包内附有使用说明、原生依赖许可和文件校验清单。",
    "风格配方在 ${tick}chromora-film-styles-v1${tick}。"
) + @($StyleDisclaimerLines) + @(
    "",
    "ZIP SHA-256 ${tick}$zipHash${tick}"
)
Write-Utf8File -Path $notesPath -Content ($releaseLines -join "`n") -Bom $false -CrLf $false
Write-Host "ZIP SHA-256 $zipHash"
Write-Host "Portable zip: $zipPath"
