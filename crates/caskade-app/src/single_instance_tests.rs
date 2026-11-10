//! A second start hands its arguments to the first and ends; what they ask
//! for is read the same in every shell.
use super::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

#[test]
fn a_second_start_hands_its_arguments_to_the_first() {
    crate::runtime::block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let endpoint = endpoint(&caskade_core::AppPaths::under(temp.path()));
        let first = Forwarded {
            args: args(&["caskade"]),
            cwd: temp.path().to_path_buf(),
        };
        let Claim::First(listener) = claim(&endpoint, &first).await.unwrap() else {
            panic!("the first start runs");
        };
        let received = Arc::new(Mutex::new(Vec::new()));
        serve(listener, {
            let received = received.clone();
            move |forwarded| received.lock().unwrap().push(forwarded)
        });
        let second = Forwarded {
            args: args(&[
                "caskade",
                "caskade://install?url=https%3A%2F%2Fexample.com%2Fg.zip",
            ]),
            cwd: PathBuf::from("/home/me/Downloads"),
        };
        assert!(matches!(
            claim(&endpoint, &second).await.unwrap(),
            Claim::Forwarded
        ));
        // `send` returns once the first has the arguments.
        assert_eq!(*received.lock().unwrap(), vec![second.clone()]);
        // A third start too.
        let third = Forwarded {
            args: args(&["caskade", "--console"]),
            cwd: PathBuf::from("/"),
        };
        send(&endpoint, &third).await.unwrap();
        assert_eq!(received.lock().unwrap().len(), 2);
    });
}

#[test]
fn a_start_after_the_first_ended_runs_itself() {
    crate::runtime::block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let endpoint = endpoint(&caskade_core::AppPaths::under(temp.path()));
        let me = Forwarded {
            args: args(&["caskade"]),
            cwd: temp.path().to_path_buf(),
        };
        let Claim::First(listener) = claim(&endpoint, &me).await.unwrap() else {
            panic!("the first start runs");
        };
        drop(listener);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(matches!(
            claim(&endpoint, &me).await.unwrap(),
            Claim::First(_)
        ));
    });
}

#[test]
fn a_garbled_message_is_refused_and_the_endpoint_keeps_serving() {
    crate::runtime::block_on(async {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let temp = tempfile::tempdir().unwrap();
        let endpoint = endpoint(&caskade_core::AppPaths::under(temp.path()));
        let me = Forwarded {
            args: args(&["caskade"]),
            cwd: temp.path().to_path_buf(),
        };
        let Claim::First(listener) = claim(&endpoint, &me).await.unwrap() else {
            panic!("the first start runs");
        };
        let received = Arc::new(Mutex::new(0));
        serve(listener, {
            let received = received.clone();
            move |_| *received.lock().unwrap() += 1
        });
        let mut stream = channel::connect(&endpoint).await.unwrap();
        stream.write_all(b"not json\n").await.unwrap();
        let mut answer = Vec::new();
        let _ = stream.read_to_end(&mut answer).await;
        assert!(!String::from_utf8_lossy(&answer).contains(TAKEN));
        send(&endpoint, &me).await.unwrap();
        assert_eq!(*received.lock().unwrap(), 1);
    });
}

#[test]
fn arguments_ask_for_links_installs_dev_folders_and_console_mode() {
    let cwd = Path::new("/home/me/Downloads");
    assert_eq!(intents(&args(&["caskade"]), cwd), vec![]);
    assert_eq!(
        intents(
            &args(&[
                "caskade",
                "caskade://pair?code=1234",
                "https://example.com/x"
            ]),
            cwd
        ),
        vec![Intent::Links(vec![
            url::Url::parse("caskade://pair?code=1234").unwrap()
        ])]
    );
    let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567";
    assert_eq!(
        intents(&args(&["caskade", magnet]), cwd),
        vec![Intent::Install(magnet.to_owned())]
    );
    assert_eq!(
        intents(&args(&["caskade", "game.torrent"]), cwd),
        vec![Intent::Install(
            "/home/me/Downloads/game.torrent".to_owned()
        )]
    );
    assert_eq!(
        intents(&args(&["caskade", "--console"]), cwd),
        vec![Intent::Console]
    );
    if cfg!(debug_assertions) {
        assert_eq!(
            intents(&args(&["caskade", "dev", "my-plugin"]), cwd),
            vec![Intent::DevFolder(cwd.join("my-plugin"))]
        );
    }
}
