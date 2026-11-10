# Samples every process whose name matches -Names: private working set (the
# closest to Linux's anonymous memory), private bytes (commit), working set,
# threads and CPU. Waits -Settle seconds, samples every -Interval for
# -Duration, then writes <Out>.csv and <Out>.json.
param([string]$Names, [int]$Settle = 120, [int]$Duration = 300, [int]$Interval = 10, [string]$Out)
Start-Sleep -Seconds $Settle
$rows = @(); $start = Get-Date
while ($true) {
  $t = ((Get-Date) - $start).TotalSeconds
  $perf = @{}
  Get-CimInstance Win32_PerfRawData_PerfProc_Process | ForEach-Object { $perf[[int]$_.IDProcess] = $_.WorkingSetPrivate }
  Get-Process | Where-Object { $_.ProcessName -match "^($Names)$" } | ForEach-Object {
    $rows += [pscustomobject]@{ t = [math]::Round($t, 1); pid = $_.Id; exe = $_.ProcessName
      private_ws_mb = [math]::Round($perf[$_.Id] / 1MB, 1); private_mb = [math]::Round($_.PrivateMemorySize64 / 1MB, 1)
      ws_mb = [math]::Round($_.WorkingSet64 / 1MB, 1); threads = $_.Threads.Count; cpu_s = $_.TotalProcessorTime.TotalSeconds }
  }
  if ($t -ge $Duration) { break }
  Start-Sleep -Seconds $Interval
}
$rows | Export-Csv "$Out.csv" -NoTypeInformation
$times = $rows | Select-Object -ExpandProperty t -Unique
$totals = foreach ($s in $times) { $at = $rows | Where-Object t -eq $s
  [pscustomobject]@{ t = $s; private_ws = ($at | Measure-Object private_ws_mb -Sum).Sum; private = ($at | Measure-Object private_mb -Sum).Sum; n = $at.Count } }
$first = $times[0]; $last = $times[-1]; $span = $last - $first
$byExe = @{}
foreach ($g in ($rows | Group-Object exe)) {
  $a = $g.Group | Where-Object t -eq $first; $b = $g.Group | Where-Object t -eq $last
  $cpu = 0; foreach ($p in $b) { $q = $a | Where-Object pid -eq $p.pid; if ($q) { $cpu += $p.cpu_s - $q.cpu_s } }
  $byExe[$g.Name] = [pscustomobject]@{ processes = $b.Count; private_ws_mb = ($b | Measure-Object private_ws_mb -Sum).Sum
    private_mb = ($b | Measure-Object private_mb -Sum).Sum; threads = ($b | Measure-Object threads -Sum).Sum
    cpu_s_per_min = if ($span) { [math]::Round($cpu * 60 / $span, 3) } else { 0 } }
}
$summary = [pscustomobject]@{ samples = $times.Count; measured_s = $span
  private_ws_mb_mean = [math]::Round(($totals | Measure-Object private_ws -Average).Average, 1)
  private_ws_mb_max = ($totals | Measure-Object private_ws -Maximum).Maximum
  private_mb_mean = [math]::Round(($totals | Measure-Object private -Average).Average, 1)
  by_exe = $byExe }
$summary | ConvertTo-Json -Depth 4 | Set-Content "$Out.json"
$summary | ConvertTo-Json -Depth 4
