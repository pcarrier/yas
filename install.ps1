# Install yas on Windows — https://yas.run
# Usage: irm https://yas.run/install.ps1 | iex
$ErrorActionPreference = "Stop"

$Releases = "https://github.com/pcarrier/yas/releases/latest/download"
$InstallDir = if ($env:YAS_INSTALL_DIR) { $env:YAS_INSTALL_DIR } else { "$env:LOCALAPPDATA\yas\bin" }
$YASExe = Join-Path $InstallDir "yas.exe"

# A running yas.exe can be renamed during an upgrade but cannot be deleted
# until that process exits. Clean up backups left by earlier upgrades.
if (Test-Path -LiteralPath $InstallDir) {
    Get-ChildItem -LiteralPath $InstallDir -Filter "yas.exe.old.*" -File -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue
}

$Arch = "x86_64"
$ZipName = "yas_windows_${Arch}.zip"
$Url = "$Releases/$ZipName"

$TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $TmpDir | Out-Null

try {
    Write-Host "downloading the latest yas for windows/$Arch from GitHub..."
    # Windows PowerShell's progress rendering can make Invoke-WebRequest much
    # slower than the network transfer itself. Suppress it only for the
    # download and restore the caller's preference afterwards.
    $OldProgressPreference = $ProgressPreference
    try {
        $ProgressPreference = "SilentlyContinue"
        Invoke-WebRequest -Uri $Url -OutFile (Join-Path $TmpDir $ZipName) -UseBasicParsing
    } finally {
        $ProgressPreference = $OldProgressPreference
    }

    Write-Host "extracting yas..."
    Expand-Archive -Path (Join-Path $TmpDir $ZipName) -DestinationPath $TmpDir -Force

    $DownloadedExe = Join-Path $TmpDir "yas.exe"
    $Version = (& $DownloadedExe --version 2>$null) -replace '.*\s', ''
    if (-not $Version) {
        throw "downloaded binary is invalid"
    }
    if (Test-Path $YASExe) {
        $Current = (& $YASExe --version 2>$null) -replace '.*\s', ''
        if ($Current -eq $Version) {
            Write-Host "yas $Version already installed."
            exit 0
        }
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

    # Windows does not let us overwrite an executable while it is running.
    # Renaming it is allowed, however, and leaves the running process attached
    # to the old file while new invocations use the replacement.
    $InstallId = [guid]::NewGuid().ToString("N")
    $StagedExe = "$YASExe.new.$InstallId"
    $BackupExe = $null
    Copy-Item -LiteralPath $DownloadedExe -Destination $StagedExe -Force

    try {
        if (Test-Path -LiteralPath $YASExe) {
            $BackupExe = "$YASExe.old.$InstallId"
            Move-Item -LiteralPath $YASExe -Destination $BackupExe
        }
        Move-Item -LiteralPath $StagedExe -Destination $YASExe
    } catch {
        $InstallError = $_
        Remove-Item -LiteralPath $StagedExe -Force -ErrorAction SilentlyContinue
        if ($BackupExe -and
            (Test-Path -LiteralPath $BackupExe) -and
            -not (Test-Path -LiteralPath $YASExe)) {
            try {
                Move-Item -LiteralPath $BackupExe -Destination $YASExe
            } catch {
                Write-Warning "failed to restore the previous yas.exe from $BackupExe"
            }
        }
        throw $InstallError
    }

    # This succeeds for ordinary installs. During `yas upgrade` the old file
    # remains in use until this installer and its parent return, so a later
    # install removes it via the stale-backup cleanup above.
    if ($BackupExe) {
        Remove-Item -LiteralPath $BackupExe -Force -ErrorAction SilentlyContinue
    }
    Write-Host "installed yas $Version to $YASExe"

    $UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($UserPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable("PATH", "$InstallDir;$UserPath", "User")
        $env:PATH = "$InstallDir;$env:PATH"
        Write-Host "added $InstallDir to PATH"
    }
} finally {
    Remove-Item -Recurse -Force $TmpDir -ErrorAction SilentlyContinue
}
