# BLITZ-0 on Windows: puts a library copy (unpacked in C:\Blitz0: data\caskade
# and cache\caskade) where the webview build reads it, for one desktop user,
# and lets that user write to C:\Blitz0 (the prototype's copy). Run as admin.
param([string]$User = "Jo O'Neil")
$ErrorActionPreference = "Stop"
$home_ = "C:\Users\$User"
$roaming = "$home_\AppData\Roaming\Caskade\Caskade"
$local = "$home_\AppData\Local\Caskade\Caskade"
foreach ($dir in $roaming, $local) {
  if (Test-Path $dir) { Rename-Item $dir "$dir.before-blitz0-$(Get-Date -Format yyyyMMddHHmmss)" }
  New-Item -ItemType Directory -Force $dir | Out-Null
}
Copy-Item -Recurse C:\Blitz0\data\caskade "$roaming\data"
Copy-Item -Recurse C:\Blitz0\cache\caskade "$local\cache"
icacls "$home_\AppData\Roaming\Caskade" /grant "${User}:(OI)(CI)F" /T /Q | Out-Null
icacls "$home_\AppData\Local\Caskade" /grant "${User}:(OI)(CI)F" /T /Q | Out-Null
icacls C:\Blitz0 /grant "${User}:(OI)(CI)F" /T /Q | Out-Null
"library copied for $User"
