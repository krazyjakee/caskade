//! Crashy (#134, test-only): every hook fails in a different way, so the
//! host's isolation can be seen working. Events trap, `launch.before`
//! never returns (the call's time box stops it), and the `grow` request
//! allocates past the memory limit.
use caskade_plugin_sdk::{Error, Intercept, Plugin};

struct Crashy;

impl Plugin for Crashy {
    fn on_event(name: &str, _payload: &str) {
        panic!("crashy trapped on {name}");
    }

    fn intercept(_point: &str, _payload: &str) -> Result<Intercept, Error> {
        loop {
            core::hint::spin_loop();
        }
    }

    fn request(method: &str, _payload: &str) -> Result<String, Error> {
        match method {
            "grow" => {
                let mut hoard: Vec<Vec<u8>> = Vec::new();
                loop {
                    hoard.push(vec![1; 16 << 20]);
                    if hoard.len() > 1 << 12 {
                        return Ok(hoard.len().to_string());
                    }
                }
            }
            "ping" => Ok("\"pong\"".into()),
            _ => Err(Error::unknown_method(method)),
        }
    }
}

caskade_plugin_sdk::export!(Crashy);
