/* A real ZIP SFX launcher. The appended ZIP has absolute central-directory
 * offsets. Windows PowerShell/.NET performs extraction; no payload is executed.
 * Native smoke coverage belongs to the Windows fixture workflow (#22). */
#define UNICODE
#define _UNICODE
#include <windows.h>
#include <wchar.h>
int wmain(void) {
    wchar_t exe[32768], cwd[32768], cmd[140000];
    STARTUPINFOW si = {0}; PROCESS_INFORMATION pi = {0}; DWORD code;
    si.cb = sizeof(si);
    if (!GetModuleFileNameW(NULL, exe, 32768) || !GetCurrentDirectoryW(32768, cwd)) return 2;
    /* Single quotes cannot occur in fixture builder paths on the native runner. */
    if (wcschr(exe, L'\'') || wcschr(cwd, L'\'')) return 3;
    _snwprintf(cmd, 140000, L"powershell.exe -NoProfile -NonInteractive -Command \"Add-Type -AssemblyName System.IO.Compression.FileSystem; [IO.Compression.ZipFile]::ExtractToDirectory('%ls','%ls')\"", exe, cwd);
    if (!CreateProcessW(NULL, cmd, NULL, NULL, FALSE, 0, NULL, NULL, &si, &pi)) return 4;
    WaitForSingleObject(pi.hProcess, INFINITE);
    GetExitCodeProcess(pi.hProcess, &code);
    CloseHandle(pi.hThread); CloseHandle(pi.hProcess);
    return (int)code;
}
