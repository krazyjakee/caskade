# Types Japanese into the text field demo through Microsoft IME (#84), in a
# Windows test session (the VM lab's desktop session through a scheduled
# task; never on someone's own desktop). Needs the Japanese language feature
# (`Add-WindowsCapability -Online -Name "Language.Basic~~~ja-JP~0.0.1.0"`)
# and ja-JP with Microsoft IME in the user's language list, and
# text_field_demo.exe (`cargo build -p caskade-native --example
# text_field_demo --release --target x86_64-pc-windows-gnu`) beside it in
# C:\Caskade. Writes what the field heard to C:\Caskade\tf.out (UTF-8):
# "input abxy" (direct input on the Japanese layout), then a composition
# ending in "input abxy日本" and "compositionend 日本".
$out = "C:\Caskade\ime.out"
"start" | Set-Content $out
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class W {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr LoadKeyboardLayout(string id, uint flags);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint thread);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  public static void Click(int x, int y) { SetCursorPos(x, y); mouse_event(2, 0, 0, 0, UIntPtr.Zero); System.Threading.Thread.Sleep(80); mouse_event(4, 0, 0, 0, UIntPtr.Zero); }
  public static void Key(byte vk) { keybd_event(vk, 0, 0, UIntPtr.Zero); System.Threading.Thread.Sleep(60); keybd_event(vk, 0, 2, UIntPtr.Zero); System.Threading.Thread.Sleep(120); }
}
"@
$p = Start-Process -FilePath "cmd.exe" -ArgumentList "/c C:\Caskade\text_field_demo.exe > C:\Caskade\tf.out 2>&1" -PassThru -WindowStyle Hidden
$demo = $null
for ($i = 0; $i -lt 60 -and -not $demo; $i++) { Start-Sleep -Milliseconds 500; $demo = Get-Process text_field_demo -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1 }
if (-not $demo) { "no window" | Add-Content $out; exit 1 }
$h = $demo.MainWindowHandle
Start-Sleep -Seconds 2
[W]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 500
$r = New-Object W+RECT
[W]::GetWindowRect($h, [ref]$r) | Out-Null
"window: $($r.L),$($r.T) $($r.R),$($r.B); foreground=$([W]::GetForegroundWindow() -eq $h)" | Add-Content $out
# The field: 20px in, 20px down, 32px tall, below the title bar.
[W]::Click($r.L + 120, $r.T + 70)
Start-Sleep -Milliseconds 500
"foreground after click=$([W]::GetForegroundWindow() -eq $h)" | Add-Content $out
[W]::Key(0x41); [W]::Key(0x42)
Start-Sleep -Milliseconds 500
"after ab (US):" | Add-Content $out
Get-Content C:\Caskade\tf.out -Encoding UTF8 | Add-Content $out
$hkl = [W]::LoadKeyboardLayout("00000411", 1)
[W]::PostMessage($h, 0x50, [IntPtr]::Zero, $hkl) | Out-Null   # WM_INPUTLANGCHANGEREQUEST
Start-Sleep -Seconds 1
$tid = [W]::GetWindowThreadProcessId($h, [IntPtr]::Zero)
"layout: {0:X}" -f [W]::GetKeyboardLayout($tid).ToInt64() | Add-Content $out
[W]::Key(0x58); [W]::Key(0x59)   # "xy" with the IME off (direct input)
Start-Sleep -Milliseconds 500
[W]::Key(0x16)   # VK_IME_ON
Start-Sleep -Milliseconds 500
foreach ($c in "NIHON".ToCharArray()) { [W]::Key([byte][char]$c) }
Start-Sleep -Milliseconds 300
"after typing:" | Add-Content $out
Get-Content C:\Caskade\tf.out -Encoding UTF8 | Add-Content $out
Start-Sleep -Milliseconds 500
[W]::Key(0x20)   # Space: convert
Start-Sleep -Milliseconds 800
[W]::Key(0x0D)   # Enter: commit
Start-Sleep -Seconds 2
Get-Content C:\Caskade\tf.out -Encoding UTF8 | Add-Content $out
Stop-Process -Id $demo.Id
"end" | Add-Content $out
