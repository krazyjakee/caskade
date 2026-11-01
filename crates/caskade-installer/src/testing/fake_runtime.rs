use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    sync::Mutex,
    time::Duration,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::runtime::{ProcessCommand, ProcessExit, ProcessId, Runtime, RuntimeError};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct FakeScript {
    pub actions: Vec<FakeAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum FakeAction {
    /// Relative paths resolve against the command's working directory; absolute
    /// paths intentionally permit simulating a malicious installer's writes.
    WriteFile {
        path: std::path::PathBuf,
        contents: Vec<u8>,
    },
    Exit {
        code: i32,
    },
    Hang,
    SpawnChild {
        command: ProcessCommand,
        script: FakeScript,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ProcessRecord {
    pub id: ProcessId,
    pub parent: Option<ProcessId>,
    pub command: ProcessCommand,
    pub finished: bool,
    pub killed: bool,
    /// Bytes given to [`Runtime::spawn_with_input`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
enum Completion {
    Exit(i32),
    Hang,
}

#[derive(Debug)]
struct Process {
    record: ProcessRecord,
    completion: Completion,
}

#[derive(Debug, Default)]
struct State {
    scripts: VecDeque<FakeScript>,
    next_id: u64,
    processes: BTreeMap<ProcessId, Process>,
}

#[derive(Debug, Default)]
pub struct FakeRuntime {
    state: Mutex<State>,
}

impl FakeRuntime {
    pub fn new(scripts: impl IntoIterator<Item = FakeScript>) -> Self {
        Self {
            state: Mutex::new(State {
                scripts: scripts.into_iter().collect(),
                ..State::default()
            }),
        }
    }
    pub fn processes(&self) -> Vec<ProcessRecord> {
        self.state
            .lock()
            .expect("fake runtime poisoned")
            .processes
            .values()
            .map(|p| p.record.clone())
            .collect()
    }
}

fn play(
    state: &mut State,
    command: ProcessCommand,
    script: FakeScript,
    parent: Option<ProcessId>,
) -> Result<ProcessId, RuntimeError> {
    state.next_id += 1;
    let id = ProcessId(state.next_id);
    state.processes.insert(
        id,
        Process {
            record: ProcessRecord {
                id,
                parent,
                command: command.clone(),
                finished: false,
                killed: false,
                input: vec![],
            },
            completion: Completion::Exit(0),
        },
    );
    let mut completion = Completion::Exit(0);
    for action in script.actions {
        match action {
            FakeAction::WriteFile { path, contents } => {
                let path = command.cwd.join(path);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|error| RuntimeError::Io {
                        message: error.to_string(),
                    })?;
                }
                fs::write(path, contents).map_err(|error| RuntimeError::Io {
                    message: error.to_string(),
                })?;
            }
            FakeAction::Exit { code } => {
                completion = Completion::Exit(code);
                break;
            }
            FakeAction::Hang => {
                completion = Completion::Hang;
                break;
            }
            FakeAction::SpawnChild { command, script } => {
                play(state, command, script, Some(id))?;
            }
        }
    }
    state
        .processes
        .get_mut(&id)
        .expect("process inserted")
        .completion = completion;
    Ok(id)
}

fn kill_tree(state: &mut State, id: ProcessId) {
    let children: Vec<_> = state
        .processes
        .values()
        .filter(|p| p.record.parent == Some(id))
        .map(|p| p.record.id)
        .collect();
    for child in children {
        kill_tree(state, child);
    }
    if let Some(process) = state.processes.get_mut(&id) {
        process.record.killed = true;
        process.record.finished = true;
    }
}

impl Runtime for FakeRuntime {
    fn spawn(&self, command: &ProcessCommand) -> Result<ProcessId, RuntimeError> {
        let mut state = self.state.lock().expect("fake runtime poisoned");
        let script = state
            .scripts
            .pop_front()
            .ok_or_else(|| RuntimeError::NoScript {
                program: command.program.clone(),
            })?;
        let next_id = ProcessId(state.next_id + 1);
        let result = play(&mut state, command.clone(), script, None);
        if result.is_err() {
            kill_tree(&mut state, next_id);
        }
        result
    }
    fn spawn_with_input(
        &self,
        command: &ProcessCommand,
        input: &[u8],
    ) -> Result<ProcessId, RuntimeError> {
        if input.len() > crate::runtime::MAX_STDIN_BYTES {
            return Err(RuntimeError::Io {
                message: "process input exceeds the stdin limit".into(),
            });
        }
        let id = self.spawn(command)?;
        if let Some(process) = self
            .state
            .lock()
            .expect("fake runtime poisoned")
            .processes
            .get_mut(&id)
        {
            process.record.input = input.to_vec();
        }
        Ok(id)
    }
    fn wait(&self, id: ProcessId, timeout: Duration) -> Result<ProcessExit, RuntimeError> {
        let mut state = self.state.lock().expect("fake runtime poisoned");
        let process = state
            .processes
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownProcess { id })?;
        if process.record.killed {
            return Err(RuntimeError::Killed { id });
        }
        match process.completion {
            Completion::Hang => {
                // Simulated time advances instantly; real wall time is never slept.
                let _simulated_elapsed = timeout;
                kill_tree(&mut state, id);
                Err(RuntimeError::TimedOut { id })
            }
            Completion::Exit(code) => {
                process.record.finished = true;
                Ok(ProcessExit { code })
            }
        }
    }
    fn kill(&self, id: ProcessId) -> Result<(), RuntimeError> {
        let mut state = self.state.lock().expect("fake runtime poisoned");
        if !state.processes.contains_key(&id) {
            return Err(RuntimeError::UnknownProcess { id });
        }
        kill_tree(&mut state, id);
        Ok(())
    }
}
