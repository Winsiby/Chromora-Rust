# Verifies bundled native binaries and source archives against native/checksums.sha256.
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$list = Join-Path $root "native\checksums.sha256"
if (-not (Test-Path $list)) {
    Write-Error "Missing checksum list: $list"
}
$failed = 0
$checked = 0
Get-Content -LiteralPath $list | ForEach-Object {
    $line = $_.Trim()
    if ($line -eq "" -or $line.StartsWith("#")) { return }
    $parts = $line -split "\s+", 2
    if ($parts.Count -ne 2) {
        Write-Error "Invalid checksum line: $line"
    }
    $expected = $parts[0].ToLowerInvariant()
    $relative = $parts[1] -replace "/", "\"
    $path = Join-Path $root $relative
    if (-not (Test-Path -LiteralPath $path)) {
        Write-Host "MISSING  $relative"
        $script:failed++
        return
    }
    $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
        Write-Host "MISMATCH $relative"
        Write-Host "  expected $expected"
        Write-Host "  actual   $actual"
        $script:failed++
        return
    }
    Write-Host "OK       $relative"
    $script:checked++
}
if ($failed -gt 0) {
    Write-Error "$failed file(s) failed hash verification ($checked passed)."
}
Write-Host "Verified $checked file(s)."
