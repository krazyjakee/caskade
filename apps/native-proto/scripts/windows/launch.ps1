# Starts a command line in the console user's desktop session (a scheduled
# task: ssh logons have no desktop).
param([string]$Cmd, [string]$User = "Jo O'Neil")
$name = "blitz0-run"
schtasks /delete /tn $name /f 2>$null | Out-Null
$action = New-ScheduledTaskAction -Execute "cmd.exe" -Argument "/c $Cmd"
$principal = New-ScheduledTaskPrincipal -UserId $User -LogonType Interactive -RunLevel Limited
Register-ScheduledTask -TaskName $name -Action $action -Principal $principal -Force | Out-Null
Start-ScheduledTask -TaskName $name
