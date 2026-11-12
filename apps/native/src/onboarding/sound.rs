//! First-run setup's sounds (#97), as the webview's `onboarding/sound.ts`:
//! the startup recording as the welcome's logo appears, and small cues
//! synthesised from the same notes and envelopes (a step, something found,
//! setup done). The output opens on the first sound and closes with setup;
//! without an audio device setup stays silent. Muting takes effect at once.

use std::cell::RefCell;
use std::num::NonZero;
use std::rc::Rc;

/// The webview's startup recording, shared.
const STARTUP: &[u8] = include_bytes!("../../../desktop/src/onboarding/sounds/startup.wav");

const RATE: u32 = 44_100;
/// The output's level, as the webview's master gain.
const OUTPUT_GAIN: f32 = 0.8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    Start,
    Step,
    Found,
    Done,
}

#[derive(Clone, Copy)]
enum Wave {
    Sine,
    Triangle,
}

/// A note: when it starts and how long it lasts (seconds), its pitch and
/// level.
struct Note {
    at: f32,
    freq: f32,
    length: f32,
    gain: f32,
    wave: Wave,
}

const fn note(at: f32, freq: f32, length: f32, gain: f32, wave: Wave) -> Note {
    Note {
        at,
        freq,
        length,
        gain,
        wave,
    }
}

const STEP: &[Note] = &[note(0.0, 880.0, 0.09, 0.05, Wave::Sine)];
const FOUND: &[Note] = &[
    note(0.0, 1318.5, 0.12, 0.05, Wave::Triangle),
    note(0.06, 1760.0, 0.16, 0.04, Wave::Sine),
];
/// A major chord, spread upwards.
const DONE: &[Note] = &[
    note(0.0, 261.63, 1.8, 0.09, Wave::Sine),
    note(0.07, 329.63, 1.7, 0.07, Wave::Sine),
    note(0.14, 392.0, 1.6, 0.07, Wave::Triangle),
    note(0.21, 523.25, 1.5, 0.06, Wave::Sine),
    note(0.3, 783.99, 1.2, 0.04, Wave::Sine),
];

/// A cue's samples (mono, 44.1 kHz): each note rises linearly over its
/// first 30 ms (or third), then falls exponentially to silence at its end,
/// as the webview's gain ramps do.
pub fn samples(cue: Cue) -> Vec<f32> {
    let notes = match cue {
        Cue::Start => return Vec::new(),
        Cue::Step => STEP,
        Cue::Found => FOUND,
        Cue::Done => DONE,
    };
    let rate = RATE as f32;
    let end = notes
        .iter()
        .map(|note| note.at + note.length + 0.05)
        .fold(0.0, f32::max);
    let mut out = vec![0.0f32; (end * rate) as usize + 1];
    for note in notes {
        let attack = (note.length / 3.0).min(0.03);
        let start = (note.at * rate) as usize;
        let count = (note.length * rate) as usize;
        for i in 0..count {
            let t = i as f32 / rate;
            let level = if t < attack {
                note.gain * t / attack
            } else {
                // From the gain down to 0.0001 at the note's end.
                let fall = (t - attack) / (note.length - attack);
                note.gain * (0.0001 / note.gain).powf(fall)
            };
            let phase = (note.freq * t).fract();
            let wave = match note.wave {
                Wave::Sine => (std::f32::consts::TAU * phase).sin(),
                Wave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
            };
            if let Some(sample) = out.get_mut(start + i) {
                *sample += wave * level * OUTPUT_GAIN;
            }
        }
    }
    out
}

/// The audio output, once opened.
struct Output {
    sink: rodio::MixerDeviceSink,
    startup: Option<rodio::Player>,
}

/// Setup's sounds; `enabled` is read on every cue.
#[derive(Clone)]
pub struct Sounds {
    enabled: Rc<dyn Fn() -> bool>,
    output: Rc<RefCell<Option<Output>>>,
    /// Tried and failed: no device; stay silent.
    unavailable: Rc<RefCell<bool>>,
    started: Rc<RefCell<bool>>,
}

impl PartialEq for Sounds {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.output, &other.output)
    }
}

impl Sounds {
    pub fn new(enabled: impl Fn() -> bool + 'static) -> Self {
        Self {
            enabled: Rc::new(enabled),
            output: Rc::default(),
            unavailable: Rc::default(),
            started: Rc::default(),
        }
    }

    /// Silent: tests, and where sounds aren't wanted.
    pub fn silent() -> Self {
        Self::new(|| false)
    }

    fn with_output(&self, play: impl FnOnce(&mut Output)) {
        if *self.unavailable.borrow() {
            return;
        }
        let mut output = self.output.borrow_mut();
        if output.is_none() {
            match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(mut sink) => {
                    sink.log_on_drop(false);
                    *output = Some(Output {
                        sink,
                        startup: None,
                    });
                }
                Err(error) => {
                    tracing::info!(%error, "no audio output: setup stays silent");
                    *self.unavailable.borrow_mut() = true;
                    return;
                }
            }
        }
        if let Some(output) = output.as_mut() {
            play(output);
        }
    }

    pub fn play(&self, cue: Cue) {
        if !(self.enabled)() {
            return;
        }
        if cue == Cue::Start {
            // Once per setup: going back to the welcome doesn't play it again.
            if self.started.replace(true) {
                return;
            }
            self.with_output(|output| {
                let Ok(decoded) = rodio::Decoder::try_from(std::io::Cursor::new(STARTUP)) else {
                    return;
                };
                let player = rodio::Player::connect_new(output.sink.mixer());
                player.set_volume(OUTPUT_GAIN);
                player.append(decoded);
                output.startup = Some(player);
            });
            return;
        }
        let samples = samples(cue);
        self.with_output(|output| {
            output.sink.mixer().add(rodio::buffer::SamplesBuffer::new(
                NonZero::new(1).expect("one channel"),
                NonZero::new(RATE).expect("a rate"),
                samples,
            ));
        });
    }

    /// Stops what is playing, when sounds are turned off.
    pub fn silence(&self) {
        if let Some(output) = self.output.borrow().as_ref()
            && let Some(startup) = &output.startup
        {
            startup.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_follow_the_webviews_notes_and_envelopes() {
        // A step: one 90 ms note, then 50 ms of tail.
        let step = samples(Cue::Step);
        assert_eq!(step.len(), (0.14 * RATE as f32) as usize + 1);
        let peak = step
            .iter()
            .fold(0.0f32, |most, sample| most.max(sample.abs()));
        assert!(peak > 0.03 && peak <= 0.05 * OUTPUT_GAIN + 1e-6, "{peak}");
        // Silent at the start and after the note.
        assert!(step[0].abs() < 1e-6);
        assert!(step.last().unwrap().abs() < 1e-6);
        // Done is the longest: its first note lasts 1.8 s.
        assert!(samples(Cue::Done).len() > (1.8 * RATE as f32) as usize);
        assert!(samples(Cue::Start).is_empty());
    }

    #[test]
    fn the_startup_recording_decodes() {
        let decoded = rodio::Decoder::try_from(std::io::Cursor::new(STARTUP)).unwrap();
        use rodio::Source;
        assert!(
            decoded
                .total_duration()
                .is_some_and(|length| length.as_secs_f32() > 0.5)
        );
    }
}
