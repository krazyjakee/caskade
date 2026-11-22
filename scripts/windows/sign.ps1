<#
.SYNOPSIS
  Authenticode-signs Caskade's Windows files with the Certum cloud
  certificate that scripts/windows/simplysign-login.ps1 mounted.

.DESCRIPTION
  The Tauri bundler's `bundle.windows.signCommand` in the release's
  sign-windows workflow: it calls this once per file, for caskade.exe, the
  NSIS plugins, the uninstaller and the setup .exe. It always fails closed:
  no certificate, or a failed signature, timestamp or trust check, exits
  non-zero, and the bundler stops.

  It never signs the pinned 7-Zip (7zz.exe, 7z.dll): the app accepts those
  by their SHA-256 (SevenZip::pinned in caskade-installer), and a signature
  would change their bytes. The workflow checks they come out unchanged.

  Environment:

    WINDOWS_CERT_SHA1      thumbprint of the certificate in
                           Cert:\CurrentUser\My (set by simplysign-login.ps1)
    WINDOWS_TIMESTAMP_URL  RFC 3161 timestamp server (optional, defaults to
                           Certum's http://time.certum.pl)

  Without a timestamp every signature stops validating the day the
  certificate expires, so it isn't optional; the server is the flakiest
  part, so signing is tried three times.

  Taken from MoGen's scripts/sign-windows.ps1, without its .pfx mode.

.EXAMPLE
  ./scripts/windows/sign.ps1 target/x86_64-pc-windows-msvc/release/caskade.exe
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)]
    [string[]]$Path
)

$ErrorActionPreference = 'Stop'
# PowerShell 7.4+ turns a native command's non-zero exit into a terminating
# error under 'Stop'; $LASTEXITCODE is checked here instead, for the retries.
$PSNativeCommandUseErrorActionPreference = $false

# Third-party files the app pins by hash.
$neverSign = @('7zz.exe', '7z.dll')

# Thumbprints copied out of certmgr carry spaces and an invisible
# left-to-right mark; signtool wants bare hex.
$certSha1 = ($env:WINDOWS_CERT_SHA1 -replace '[^0-9a-fA-F]', '')
if ([string]::IsNullOrWhiteSpace($certSha1)) {
    throw 'WINDOWS_CERT_SHA1 is not set: run scripts/windows/simplysign-login.ps1 first.'
}

$timestampUrl = $env:WINDOWS_TIMESTAMP_URL
if ([string]::IsNullOrWhiteSpace($timestampUrl)) {
    $timestampUrl = 'http://time.certum.pl'
}

# signtool.exe ships with the Windows SDK under a versioned folder, and the
# runner image can carry several: take the newest x64 one.
function Find-SignTool {
    $cmd = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }

    $roots = @(
        "${env:ProgramFiles(x86)}\Windows Kits\10\bin",
        "$env:ProgramFiles\Windows Kits\10\bin"
    ) | Where-Object { $_ -and (Test-Path $_) }

    $found = foreach ($root in $roots) {
        Get-ChildItem -Path $root -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match '\\x64\\signtool\.exe$' }
    }
    # By the parsed SDK version (…\bin\10.0.22621.0\x64): as strings,
    # "10.0.9" sorts after "10.0.22621".
    $best = $found | Sort-Object -Property @{ Expression = {
        $parsed = $null
        if ([version]::TryParse($_.Directory.Parent.Name, [ref]$parsed)) { $parsed } else { [version]'0.0.0.0' }
    } } -Descending | Select-Object -First 1
    if (-not $best) { throw 'signtool.exe not found (is the Windows SDK installed on this runner?)' }
    return $best.FullName
}

$targets = @()
foreach ($pattern in $Path) {
    $items = @(Get-Item -Path $pattern -ErrorAction SilentlyContinue)
    if ($items.Count -eq 0) { throw "Nothing to sign at $pattern" }
    foreach ($item in $items) {
        if ($item.PSIsContainer) { continue }
        if ($neverSign -contains $item.Name) {
            Write-Host "Not signing $($item.Name): the app pins its hash."
            continue
        }
        $existing = Get-AuthenticodeSignature -FilePath $item.FullName -ErrorAction SilentlyContinue
        if ($existing -and $existing.Status -eq 'Valid') {
            Write-Host "Already signed: $($item.Name)"
            continue
        }
        $targets += $item.FullName
    }
}
if ($targets.Count -eq 0) { exit 0 }

$cert = Get-ChildItem Cert:\CurrentUser\My -ErrorAction SilentlyContinue |
    Where-Object { $_.Thumbprint -eq $certSha1 } | Select-Object -First 1
if (-not $cert) {
    throw "No certificate with thumbprint $certSha1 in Cert:\CurrentUser\My (is the SimplySign session still open?)"
}

$signtool = Find-SignTool
# The key stays in Certum's cloud behind SimplySign's virtual card; signtool
# reaches it through the store, so no key or password is on disk.
$signArgs = @('sign', '/fd', 'sha256', '/sha1', $certSha1, '/d', 'Caskade',
    '/tr', $timestampUrl, '/td', 'sha256') + $targets

$attempts = 3
for ($attempt = 1; $attempt -le $attempts; $attempt++) {
    & $signtool @signArgs
    if ($LASTEXITCODE -eq 0) { break }
    if ($attempt -eq $attempts) { throw "signtool failed $attempts times (exit $LASTEXITCODE)" }
    $delay = $attempt * 15
    Write-Host "::warning::signtool attempt $attempt failed (exit $LASTEXITCODE), usually the timestamp server; retrying in ${delay}s."
    Start-Sleep -Seconds $delay
}

# The Authenticode policy: the chain to a trusted root, and the timestamp.
& $signtool verify /pa /all $targets
if ($LASTEXITCODE -ne 0) { throw "signtool verify failed (exit $LASTEXITCODE)" }
foreach ($target in $targets) { Write-Host "Signed $target" }
