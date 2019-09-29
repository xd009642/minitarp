use crate::statemachine::*;
use crate::Error as RunError;
use crate::Trace;
use nix::errno::Errno;
use nix::sys::signal::Signal;
use nix::sys::wait::*;
use nix::unistd::Pid;
use nix::Error as NixErr;
use std::collections::{HashMap, HashSet};

pub fn create_state_machine<'a>(test: Pid, traces: &'a mut [Trace]) -> (TestState, LinuxData<'a>) {
    let mut data = LinuxData::new(traces);
    data.parent = test;
    (TestState::start_state(), data)
}

pub type UpdateContext = (TestState, TracerAction<ProcessInfo>);

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ProcessInfo {
    pid: Pid,
    signal: Option<Signal>,
}

impl ProcessInfo {
    fn new(pid: Pid, signal: Option<Signal>) -> Self {
        Self { pid, signal }
    }
}

impl From<Pid> for ProcessInfo {
    fn from(pid: Pid) -> Self {
        ProcessInfo::new(pid, None)
    }
}

impl From<&Pid> for ProcessInfo {
    fn from(pid: &Pid) -> Self {
        ProcessInfo::new(*pid, None)
    }
}

/// Handle to linux process state
pub struct LinuxData<'a> {
    /// Recent results from waitpid to be handled by statemachine
    wait_queue: Vec<WaitStatus>,
    /// Current Pid to process
    current: Pid,
    /// Parent PID of test process
    parent: Pid,
    /// Map of addresses to breakpoints
    breakpoints: HashMap<u64, Breakpoint>,
    /// Instrumentation points in code with associated coverage data
    traces: &'a mut [Trace],
    /// Thread count. Hopefully getting rid of in future
    thread_count: isize,
    /// Used for plotting a signal timeline
    timeline: Timeline,
}

impl<'a> StateData for LinuxData<'a> {
    fn start(&mut self) -> Result<Option<TestState>, RunError> {
        match waitpid(self.current, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => Ok(None),
            Ok(sig @ WaitStatus::Stopped(_, Signal::SIGTRAP)) => {
                if let WaitStatus::Stopped(child, _) = sig {
                    self.current = child;
                }
                println!("Caught inferior transitioning to Initialise state");
                Ok(Some(TestState::Initialise))
            }
            Ok(_) => Err(RunError::TestRuntime(
                "Unexpected signal when starting test".to_string(),
            )),
            Err(e) => Err(RunError::TestRuntime(format!(
                "Error when starting test: {}",
                e
            ))),
        }
    }

    fn init(&mut self) -> Result<TestState, RunError> {
        println!("init");
        trace_children(self.current)?;
        println!("Adding traces");
        for trace in self.traces.iter() {
            if let Some(addr) = trace.address {
                match Breakpoint::new(self.current, addr) {
                    Ok(bp) => {
                        let _ = self.breakpoints.insert(addr, bp);
                    }
                    Err(e) if e == NixErr::Sys(Errno::EIO) => {
                        return Err(RunError::TestRuntime(
                            "ERROR: Tarpaulin cannot find code addresses \
                             check that pie is disabled for your linker. \
                             If linking with gcc try adding -C link-args=-no-pie \
                             to your rust flags"
                                .to_string(),
                        ));
                    }
                    Err(NixErr::UnsupportedOperation) => {
                        println!("Instrumentation address clash, ignoring 0x{:x}", addr);
                    }
                    Err(_) => {
                        return Err(RunError::TestRuntime(
                            "Failed to instrument test executable".to_string(),
                        ));
                    }
                }
            }
        }

        if continue_exec(self.parent, None).is_ok() {
            println!("Initialised inferior, transitioning to wait state");
            Ok(TestState::wait_state())
        } else {
            Err(RunError::TestRuntime(
                "Test didn't launch correctly".to_string(),
            ))
        }
    }

    fn wait(&mut self) -> Result<Option<TestState>, RunError> {
        let mut result = Ok(None);
        let mut running = true;
        while running {
            let wait = waitpid(
                Pid::from_raw(-1),
                Some(WaitPidFlag::WNOHANG | WaitPidFlag::__WALL),
            );
            match wait {
                Ok(WaitStatus::StillAlive) => {
                    running = false;
                }
                Ok(WaitStatus::Exited(_, _)) => {
                    self.wait_queue.push(wait.unwrap());
                    result = Ok(Some(TestState::Stopped));
                    running = false;
                }
                Ok(WaitStatus::PtraceEvent(_, _, _)) => {
                    self.wait_queue.push(wait.unwrap());
                    result = Ok(Some(TestState::Stopped));
                    running = false;
                }
                Ok(s) => {
                    self.wait_queue.push(s);
                    result = Ok(Some(TestState::Stopped));
                }
                Err(e) => {
                    running = false;
                    result = Err(RunError::TestRuntime(format!(
                        "An error occurred while waiting for response from test: {}",
                        e
                    )))
                }
            }
        }
        if !self.wait_queue.is_empty() {
            println!("Result queue is {:?}", self.wait_queue);
        }
        result
    }

    fn stop(&mut self) -> Result<TestState, RunError> {
        println!("Stop");
        let mut actions = Vec::new();
        let mut pcs = HashSet::new();
        let mut result = Ok(TestState::wait_state());
        let pending = self.wait_queue.clone();
        self.wait_queue.clear();
        for status in &pending {
            let state = match status {
                WaitStatus::PtraceEvent(c, s, e) => match self.handle_ptrace_event(*c, *s, *e) {
                    Ok(s) => Ok(s),
                    Err(e) => Err(RunError::TestRuntime(format!(
                        "Error occurred when handling ptrace event: {:?}",
                        e
                    ))),
                },
                WaitStatus::Stopped(c, Signal::SIGTRAP) => {
                    self.timeline
                        .add_event(Event::new(*c, "SIGTRAP".to_string()));
                    self.current = *c;
                    match self.collect_coverage_data(&mut pcs) {
                        Ok(s) => Ok(s),
                        Err(e) => Err(RunError::TestRuntime(format!(
                            "Error when collecting coverage: {:?}",
                            e
                        ))),
                    }
                }
                WaitStatus::Stopped(child, Signal::SIGSTOP) => {
                    self.timeline
                        .add_event(Event::new(*child, "SIGSTOP".to_string()));
                    Ok((
                        TestState::wait_state(),
                        TracerAction::Continue(child.into()),
                    ))
                }
                WaitStatus::Stopped(c, Signal::SIGSEGV) => {
                    self.timeline
                        .add_event(Event::new(*c, "SIGSEGV".to_string()));
                    println!("Trying to save");
                    self.timeline.save_graph("output.png");
                    Err(RunError::TestRuntime(
                        "A segfault occurred while executing tests".to_string(),
                    ))
                }
                WaitStatus::Stopped(child, Signal::SIGILL) => {
                    self.timeline
                        .add_event(Event::new(*child, "SIGILL".to_string()));
                    let pc = current_instruction_pointer(*child).unwrap_or_else(|_| 1) - 1;
                    println!("SIGILL raised. Child program counter is: 0x{:x}", pc);
                    Err(RunError::TestRuntime(format!(
                        "Error running test - SIGILL raised in {}",
                        child
                    )))
                }
                WaitStatus::Stopped(c, _) => {
                    let info = ProcessInfo::new(*c, None);
                    Ok((TestState::wait_state(), TracerAction::TryContinue(info)))
                }
                WaitStatus::Signaled(c, s, f) => {
                    if let Ok(s) = self.handle_signaled(c, s, *f) {
                        Ok(s)
                    } else {
                        Err(RunError::TestRuntime(
                            "Attempting to handle tarpaulin being signaled".to_string(),
                        ))
                    }
                }
                WaitStatus::Exited(child, ec) => {
                    self.timeline
                        .add_event(Event::new(*child, format!("EXITED {}", ec)));
                    for ref mut value in self.breakpoints.values_mut() {
                        value.thread_killed(*child);
                    }
                    println!("Exited {:?} parent {:?}", child, self.parent);
                    if child == &self.parent {
                        Ok((TestState::End(*ec), TracerAction::Nothing))
                    } else {
                        // Process may have already been destroyed. This is just incase
                        Ok((
                            TestState::wait_state(),
                            TracerAction::TryContinue(self.parent.into()),
                        ))
                    }
                }
                _ => Err(RunError::TestRuntime(
                    "An unexpected signal has been caught by tarpaulin!".to_string(),
                )),
            };
            match state {
                Ok((TestState::Waiting { .. }, action)) => {
                    actions.push(action);
                }
                Ok((state, action)) => {
                    result = Ok(state);
                    actions.push(action);
                }
                Err(e) => result = Err(e),
            }
        }
        let mut continued = false;
        for a in &actions {
            println!("Executing action {:?}", a);
            match a {
                TracerAction::TryContinue(t) => {
                    self.timeline
                        .add_event(Event::new(t.pid, "TryContinue".to_string()));
                    continued = true;
                    let _ = continue_exec(t.pid, t.signal);
                }
                TracerAction::Continue(t) => {
                    self.timeline
                        .add_event(Event::new(t.pid, "Continue".to_string()));
                    continued = true;
                    continue_exec(t.pid, t.signal)?;
                }
                TracerAction::Step(t) => {
                    self.timeline
                        .add_event(Event::new(t.pid, "Step".to_string()));
                    continued = true;
                    single_step(t.pid)?;
                }
                TracerAction::Detach(t) => {
                    self.timeline
                        .add_event(Event::new(t.pid, "Detach".to_string()));
                    continued = true;
                    detach_child(t.pid)?;
                }
                _ => {}
            }
        }
        if !continued {
            println!("No action suggested to continue tracee. Attempting a continue");
            let _ = continue_exec(self.parent, None);
        }
        result
    }
}

impl<'a> LinuxData<'a> {
    pub fn new(traces: &'a mut [Trace]) -> LinuxData<'a> {
        LinuxData {
            wait_queue: Vec::new(),
            current: Pid::from_raw(0),
            parent: Pid::from_raw(0),
            breakpoints: HashMap::new(),
            traces,
            thread_count: 0,
            timeline: Timeline::new(),
        }
    }

    fn handle_ptrace_event(
        &mut self,
        child: Pid,
        sig: Signal,
        event: i32,
    ) -> Result<(TestState, TracerAction<ProcessInfo>), RunError> {
        use nix::libc::*;
        let rip = match current_instruction_pointer(child) {
            Ok(pc) => (pc - 1),
            Err(_) => std::i64::MIN,
        };

        if sig == Signal::SIGTRAP {
            match event {
                PTRACE_EVENT_CLONE => match get_event_data(child) {
                    Ok(t) => {
                        self.timeline.add_event(Event::new_thread(
                            rip,
                            child,
                            Pid::from_raw(t as pid_t),
                        ));
                        println!("New thread spawned {}", t);
                        self.thread_count += 1;
                        Ok((
                            TestState::wait_state(),
                            TracerAction::Continue(child.into()),
                        ))
                    }
                    Err(e) => {
                        println!("Error in clone event {:?}", e);
                        Err(RunError::TestRuntime(
                            "Error occurred upon test executable thread creation".to_string(),
                        ))
                    }
                },
                PTRACE_EVENT_FORK | PTRACE_EVENT_VFORK => {
                    println!("Caught fork event");
                    self.timeline
                        .add_event(Event::new(child, "Fork Event".to_string()));
                    Ok((
                        TestState::wait_state(),
                        TracerAction::Continue(child.into()),
                    ))
                }
                PTRACE_EVENT_EXEC => {
                    self.timeline
                        .add_event(Event::new(child, "Exec Event".to_string()));
                    println!("Child execed other process - detaching ptrace");
                    Ok((TestState::wait_state(), TracerAction::Detach(child.into())))
                }
                PTRACE_EVENT_EXIT => {
                    println!("Child exiting");
                    self.timeline
                        .add_event(Event::new(child, "EXIT".to_string()));
                    self.thread_count -= 1;
                    Ok((
                        TestState::wait_state(),
                        TracerAction::TryContinue(child.into()),
                    ))
                }
                _ => Err(RunError::TestRuntime(format!(
                    "Unrecognised ptrace event {}",
                    event
                ))),
            }
        } else {
            println!("Unexpected signal with ptrace event {}", event);
            println!("Signal: {:?}", sig);
            Err(RunError::TestRuntime("Unexpected signal".to_string()))
        }
    }

    fn collect_coverage_data(
        &mut self,
        visited_pcs: &mut HashSet<u64>,
    ) -> Result<UpdateContext, RunError> {
        let mut action = None;
        if let Ok(rip) = current_instruction_pointer(self.current) {
            let rip = (rip - 1) as u64;
            println!("Hit address 0x{:x}", rip);
            if self.breakpoints.contains_key(&rip) {
                let bp = &mut self.breakpoints.get_mut(&rip).unwrap();
                let updated = if visited_pcs.contains(&rip) {
                    let _ = bp.jump_to(self.current);
                    (true, TracerAction::Continue(self.current.into()))
                } else {
                    let enable = false;
                    // Don't reenable if multithreaded as can't yet sort out segfault issue
                    if let Ok(x) = bp.process(self.current, enable) {
                        x
                    } else {
                        // So failed to process a breakpoint.. Still continue to avoid
                        // stalling
                        (false, TracerAction::Continue(self.current.into()))
                    }
                };
                if updated.0 {
                    for t in self.traces.iter_mut() {
                        if t.address == Some(rip) {
                            t.count += 1;
                        }
                    }
                }
                action = Some(updated.1);
            }
        }
        let action = action.unwrap_or_else(|| TracerAction::Continue(self.current.into()));
        Ok((TestState::wait_state(), action))
    }

    fn handle_signaled(
        &mut self,
        pid: &Pid,
        sig: &Signal,
        flag: bool,
    ) -> Result<UpdateContext, RunError> {
        match (sig, flag) {
            (Signal::SIGTRAP, true) => {
                Ok((TestState::wait_state(), TracerAction::Continue(pid.into())))
            }
            _ => Err(RunError::StateMachine("Unexpected stop".to_string())),
        }
    }
}
