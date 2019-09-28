use nix::unistd::*;
use serde::Deserialize;
use std::env;
use std::ffi::CString;
use std::path::{Path, PathBuf};

mod breakpoint;
pub mod linux;
use linux::*;
pub mod ptrace_control;
mod statemachine;

use statemachine::*;

pub mod prelude {
    pub use super::*;
}

#[derive(Deserialize)]
pub struct Config {
    pub breakpoints: Vec<u64>,
    pub binary: PathBuf,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Copy)]
pub struct Trace {
    /// Optional address showing location in the test artefact
    pub address: Option<u64>,
    pub count: usize,
}

impl Trace {
    pub fn new(addr: u64) -> Trace {
        Trace {
            address: Some(addr),
            count: 0,
        }
    }
}

#[derive(Debug)]
pub enum Error {
    TestDoesntExist,
    ForkFail,
    Internal,
    TestFail,
    Trace(String),
    TestRuntime(String),
    BadToml(String),
    Sys,
    StateMachine(String),
    NixError(nix::Error),
    IO(std::io::Error),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IO(e)
    }
}

impl From<nix::Error> for Error {
    fn from(e: nix::Error) -> Self {
        Error::NixError(e)
    }
}

pub fn run(config: &Config) -> Result<(), Error> {
    if !config.binary.exists() {
        return Err(Error::TestDoesntExist);
    }
    match fork() {
        Ok(ForkResult::Parent { child }) => match collect_coverage(child, config) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        },
        Ok(ForkResult::Child) => {
            execute_test(&config.binary)?;
            Ok(())
        }
        Err(err) => Err(Error::ForkFail),
    }
}

fn collect_coverage(test: Pid, config: &Config) -> Result<(), Error> {
    let mut traces = config
        .breakpoints
        .iter()
        .map(|x| Trace::new(*x))
        .collect::<Vec<_>>();
    let (mut state, mut data) = create_state_machine(test, traces.as_mut_slice(), config);
    loop {
        state = state.step(&mut data, config)?;
        if state.is_finished() {
            if let TestState::End(i) = state {
                println!("Return code is {}", i);
            }
            break;
        }
    }
    for t in &traces {
        println!("Address {:x} hits {}", t.address.unwrap_or(0), t.count);
    }
    Ok(())
}

/// Launches the test executable
fn execute_test(test: &Path) -> Result<(), Error> {
    let exec_path = CString::new(test.to_str().unwrap()).unwrap();
    println!("running {}", test.display());

    let mut envars: Vec<CString> = vec![CString::new("RUST_TEST_THREADS=1").unwrap()];
    for (key, value) in env::vars() {
        let mut temp = String::new();
        temp.push_str(key.as_str());
        temp.push('=');
        temp.push_str(value.as_str());
        envars.push(CString::new(temp).unwrap());
    }
    let argv = vec![exec_path.clone()];

    envars.push(CString::new("RUST_BACKTRACE=1").unwrap());

    execute(exec_path, &argv, envars.as_slice())
}
