# Exercise Inno Setup install, upgrade and uninstall in both scopes on a CI VM.
param([Parameter(Mandatory=$true)][string]$Installer,
      [Parameter(Mandatory=$true)][string]$UpgradeInstaller,
      [Parameter(Mandatory=$true)][string]$UpgradeVersion)
$ErrorActionPreference = 'Stop'
function Run-Setup([string]$Exe, [string[]]$Options) {
    $process = Start-Process -FilePath $Exe -ArgumentList $Options -PassThru
    if (-not $process.WaitForExit(90000)) { $process.Kill(); throw 'Setup timed out' }
    if ($process.ExitCode -ne 0) { throw "Setup failed: $($process.ExitCode)" }
}
function Defaults {
    return @('.mp4', '.mov', '.lrv', '.insv') | ForEach-Object {
        $extension = $_
        foreach ($root in @('HKCU:', 'HKLM:')) {
            $key = Get-Item "$root\Software\Classes\$extension" -ErrorAction SilentlyContinue
            "$root$extension=$(if ($key) { $key.GetValue('') })"
        }
        $choice = Get-Item "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\$extension\UserChoice" -ErrorAction SilentlyContinue
        "$extension UserChoice=$(if ($choice) { $choice.GetValue('ProgId') })"
    }
}
$before = @(Defaults)
foreach ($scope in @('CURRENTUSER', 'ALLUSERS')) {
    $root = if ($scope -eq 'CURRENTUSER') { 'HKCU:' } else { 'HKLM:' }
    $directory = Join-Path $env:TEMP "ActionLay-installer-$scope"
    $options = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/$scope", "/DIR=`"$directory`"", '/TASKS=desktopicon')
    Run-Setup $Installer $options
    $command = Get-Item "$root\Software\Classes\ActionLay.InstalledVideo\shell\open\command"
    if ($command.GetValue('') -ne "`"$directory\actionlay.exe`" `"%1`"") { throw 'Incorrect open command' }
    if (-not (Test-Path "$root\Software\Microsoft\Windows\CurrentVersion\Uninstall\{B7B4CFBE-D778-4FB8-A976-84E90F770ACF}_is1")) { throw 'Missing Apps uninstaller entry' }
    foreach ($extension in @('.mp4', '.mov', '.lrv', '.insv')) {
        $key = Get-Item "$root\Software\Classes\$extension\OpenWithProgids"
        if ($key.GetValueNames() -notcontains 'ActionLay.InstalledVideo') { throw "Missing $extension association" }
    }
    $desktop = if ($scope -eq 'CURRENTUSER') { [Environment]::GetFolderPath('Desktop') } else { [Environment]::GetFolderPath('CommonDesktopDirectory') }
    if (-not (Test-Path (Join-Path $desktop 'ActionLay.lnk'))) { throw 'Missing optional desktop shortcut' }
    # A newer version uses the same AppId and remembers the installation path.
    Run-Setup $UpgradeInstaller @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/$scope", '/TASKS=desktopicon')
    $uninstaller = Get-Item "$root\Software\Microsoft\Windows\CurrentVersion\Uninstall\{B7B4CFBE-D778-4FB8-A976-84E90F770ACF}_is1"
    if ($uninstaller.GetValue('DisplayVersion') -ne $UpgradeVersion) { throw 'Upgrade did not update the existing installation' }
    Run-Setup "$directory\unins000.exe" @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART')
    if (Test-Path "$root\Software\Classes\ActionLay.InstalledVideo") { throw 'Uninstaller left its ProgID' }
    if (Test-Path "$directory\actionlay.exe") { throw 'Uninstaller left the executable' }
}
if (Compare-Object $before @(Defaults)) { throw 'Setup changed a default file association' }
Write-Output 'Installer scopes, reinstall/upgrade, uninstall and unchanged defaults verified.'
