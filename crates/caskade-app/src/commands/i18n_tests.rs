//! The language the UI starts in, and the event for each change.
use super::*;
use crate::events::Emit;
use std::sync::{Arc, Mutex};

/// The languages that reached the UI.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<String>>>);

impl Emit for Sink {
    fn emit(&self, event: Event) -> Result<(), String> {
        if let Event::LocaleChanged(changed) = event {
            self.0.lock().unwrap().push(changed.locale);
        }
        Ok(())
    }
}

#[test]
fn the_ui_learns_the_language_and_what_system_means() {
    let reply = locale_current();
    assert_eq!(reply.available.first().map(String::as_str), Some("en"));
    assert!(reply.available.iter().any(|tag| tag == "sv"));
    assert!(reply.available.contains(&reply.locale));
    // System is the OS's language when Caskade ships it, else English.
    assert!(reply.available.contains(&reply.system));
    assert_eq!(reply.system, i18n::resolve(i18n::SYSTEM));
}

#[tokio::test(flavor = "multi_thread")]
async fn each_change_of_language_reaches_the_ui() {
    let sink = Sink::default();
    // A channel of its own: the core's language is the whole process's.
    let (language, changes) = watch::channel("en".to_owned());
    let announcing = tokio::spawn(announce(Events::new(sink.clone()), changes));
    let wait_for = |count: usize| {
        let seen = sink.0.clone();
        async move {
            for _ in 0..200 {
                if seen.lock().unwrap().len() >= count {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            panic!("no LocaleChanged");
        }
    };

    language.send("sv".into()).unwrap();
    wait_for(1).await;
    language.send("en".into()).unwrap();
    wait_for(2).await;
    assert_eq!(sink.0.lock().unwrap().as_slice(), ["sv", "en"]);
    announcing.abort();
}
