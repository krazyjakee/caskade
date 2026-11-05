//! The real `caskade --launch` on Windows (#22, the CI job `windows-launch`):
//! WM_CLOSE to its hidden window ends it as stopped. `launch_cli::tests`
//! follows a closed channel from there to the game being stopped.

use caskade_core::AppPaths;
use caskade_desktop::launch_cli::{Endpoint, Reply, channel::Listener, code};
use std::{
    process::{Child, Command},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    time::timeout,
};

/// Posts WM_CLOSE to the process's window titled Caskade and prints how many
/// took it. PowerShell, as this workspace forbids `unsafe`.
const POST_WM_CLOSE: &str = r#"
param([uint32]$Caller)
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class Closer {
    delegate bool Each(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(Each each, IntPtr data);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr window, StringBuilder text, int size);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool PostMessage(IntPtr window, uint message, IntPtr wide, IntPtr low);
    public static int Close(uint caller) {
        int posted = 0;
        EnumWindows((window, data) => {
            uint process;
            GetWindowThreadProcessId(window, out process);
            var title = new StringBuilder(64);
            GetWindowText(window, title, title.Capacity);
            if (process == caller && title.ToString() == "Caskade"
                && PostMessage(window, 0x0010, IntPtr.Zero, IntPtr.Zero)) posted++;
            return true;
        }, IntPtr.Zero);
        return posted;
    }
}
'@
[Closer]::Close($Caller)
"#;

/// Gives the windows of `caller` that took WM_CLOSE.
fn post_wm_close(caller: u32) -> String {
    let folder = tempfile::tempdir().unwrap();
    let script = folder.path().join("close.ps1");
    std::fs::write(&script, POST_WM_CLOSE).unwrap();
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
        ])
        .arg("-File")
        .arg(&script)
        .arg(caller.to_string())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Ends the caller when a failed check leaves it running.
struct Caller(Child);

impl Drop for Caller {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

#[tokio::test]
async fn wm_close_to_the_hidden_window_ends_the_caller_as_stopped() {
    // The caller finds the standard pipe. Binding it first stands in for
    // Caskade, so the caller doesn't start the real app.
    let endpoint = Endpoint::for_paths(&AppPaths::standard().unwrap());
    let mut listener = Listener::bind(&endpoint)
        .await
        .expect("no other Caskade may be running");
    let mut caller = Caller(
        Command::new(env!("CARGO_BIN_EXE_caskade"))
            .args(["--launch", "a-game"])
            .spawn()
            .unwrap(),
    );
    // The window exists before the caller connects.
    let stream = timeout(Duration::from_secs(30), listener.accept())
        .await
        .expect("the caller never connected")
        .unwrap();
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = BufReader::new(read);
    let mut request = String::new();
    reader.read_line(&mut request).await.unwrap();
    assert!(request.contains("a-game"), "{request}");
    let mut started = serde_json::to_vec(&Reply::Started {
        session_id: "session".into(),
        title: "A Game".into(),
    })
    .unwrap();
    started.push(b'\n');
    write.write_all(&started).await.unwrap();

    assert_eq!(post_wm_close(caller.0.id()), "1");

    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = caller.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the caller stayed");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(status.code(), Some(code::STOPPED));
    // Its end closed with it, which is what stops the game.
    let mut rest = [0_u8; 1];
    let read = timeout(Duration::from_secs(5), reader.read(&mut rest))
        .await
        .expect("the pipe stayed open");
    assert!(matches!(read, Ok(0) | Err(_)), "{read:?}");
}
