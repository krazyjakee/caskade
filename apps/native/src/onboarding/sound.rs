//! First-run setup's sounds (#97), as the webview's `onboarding/sound.ts`:
//! the startup recording as the welcome's logo appears, and small cues
//! synthesised from the same notes and envelopes (a step, something found,
//! setup done). The output opens on the first sound, off the UI thread, and
//! closes with setup; without an audio device setup stays silent. Muting
//! takes effect at once.

use std::cell::RefCell;
use std::num::NonZero;
use std::rc::Rc;

/// The startup recording.
const STARTUP: &[u8] = include_bytes!("../../assets/sounds/startup.wav");

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

/// Opens the audio output: the default device, or a test's stand-in.
type Open = fn() -> Result<rodio::MixerDeviceSink, rodio::DeviceSinkError>;

fn open_default() -> Result<rodio::MixerDeviceSink, rodio::DeviceSinkError> {
    rodio::DeviceSinkBuilder::open_default_sink()
}

/// What setup asks of the audio output.
enum Command {
    /// The startup recording.
    Startup,
    /// A cue's samples.
    Cue(Vec<f32>),
    /// Stop the startup recording.
    Silence,
}

/// The audio output, opened and played on a blocking task of the app's
/// runtime, never on the UI thread: opening a device can stall (an ALSA
/// setup without a sound card), and the UI only ever sends. The task ends,
/// closing the output, when setup's last handle goes.
fn run_output(commands: std::sync::mpsc::Receiver<Command>, open: Open) {
    let mut sink = match open() {
        Ok(sink) => sink,
        Err(error) => {
            tracing::info!(%error, "no audio output: setup stays silent");
            return;
        }
    };
    sink.log_on_drop(false);
    let mut startup: Option<rodio::Player> = None;
    while let Ok(command) = commands.recv() {
        match command {
            Command::Startup => {
                let Ok(decoded) = rodio::Decoder::try_from(std::io::Cursor::new(STARTUP)) else {
                    continue;
                };
                let player = rodio::Player::connect_new(sink.mixer());
                player.set_volume(OUTPUT_GAIN);
                player.append(decoded);
                startup = Some(player);
            }
            Command::Cue(samples) => {
                sink.mixer().add(rodio::buffer::SamplesBuffer::new(
                    NonZero::new(1).expect("one channel"),
                    NonZero::new(RATE).expect("a rate"),
                    samples,
                ));
            }
            Command::Silence => {
                if let Some(startup) = &startup {
                    startup.clear();
                }
            }
        }
    }
}

/// Setup's sounds; `enabled` is read on every cue.
#[derive(Clone)]
pub struct Sounds {
    enabled: Rc<dyn Fn() -> bool>,
    open: Open,
    /// The output task's queue, from the first sound on; `None` before it,
    /// and again once the output turned out to be unavailable.
    output: Rc<RefCell<Option<std::sync::mpsc::Sender<Command>>>>,
    /// The output was tried and isn't there: stay silent.
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
            open: open_default,
            output: Rc::default(),
            unavailable: Rc::default(),
            started: Rc::default(),
        }
    }

    /// Silent: tests, and where sounds aren't wanted.
    pub fn silent() -> Self {
        Self::new(|| false)
    }

    /// Queues `command` for the output, opening it on first use. Never
    /// waits.
    fn send(&self, command: Command) {
        if *self.unavailable.borrow() {
            return;
        }
        let mut output = self.output.borrow_mut();
        let sender = output.get_or_insert_with(|| {
            let (sender, commands) = std::sync::mpsc::channel();
            let open = self.open;
            caskade_app::runtime::spawn_blocking(move || run_output(commands, open));
            sender
        });
        // The task ended: no output.
        if sender.send(command).is_err() {
            *output = None;
            *self.unavailable.borrow_mut() = true;
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
            self.send(Command::Startup);
            return;
        }
        self.send(Command::Cue(samples(cue)));
    }

    /// Stops what is playing, when sounds are turned off.
    pub fn silence(&self) {
        if self.output.borrow().is_some() {
            self.send(Command::Silence);
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

    /// An output that takes its time to open, as ALSA without a sound card
    /// can, and then isn't there.
    fn stalls() -> Result<rodio::MixerDeviceSink, rodio::DeviceSinkError> {
        std::thread::sleep(std::time::Duration::from_secs(5));
        Err(rodio::DeviceSinkError::NoDevice)
    }

    #[test]
    fn a_stalled_output_never_holds_up_the_ui() {
        let sounds = Sounds {
            open: stalls,
            ..Sounds::new(|| true)
        };
        let started = std::time::Instant::now();
        sounds.play(Cue::Start);
        sounds.play(Cue::Step);
        sounds.silence();
        sounds.play(Cue::Done);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
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
