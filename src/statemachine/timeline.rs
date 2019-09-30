use crate::ptrace_control::*;
use nix::unistd::*;
use std::collections::HashSet;

use gnuplot::{AxesCommon, Coordinate, Figure, LabelOption, MarginSide, AutoOption};

#[derive(Clone, Debug)]
pub struct Event {
    pid: Pid,
    child: Option<Pid>,
    addr: i64,
    descr: String,
    mem_before: Option<u64>,
    mem_after: Option<u64>,
}

impl Event {
    pub fn new(pid: Pid, descr: String) -> Self {
        let addr = match current_instruction_pointer(pid) {
            Ok(pc) => (pc - 1),
            Err(_) => std::i64::MIN,
        };
        Event {
            pid,
            child: None,
            addr,
            descr,
            mem_before: None,
            mem_after: None,
        }
    }

    pub fn new_thread(addr: i64, parent: Pid, child: Pid) -> Self {
        Event {
            addr,
            pid: parent,
            child: Some(child),
            descr: format!("New Thread {}", child),
            mem_before: None,
            mem_after: None,
        }
    }
}

pub struct Timeline {
    pids: HashSet<Pid>,
    events: Vec<Event>,
}

impl Timeline {
    pub fn new() -> Self {
        Self {
            pids: HashSet::new(),
            events: vec![],
        }
    }

    pub fn add_event(&mut self, event: Event) {
        self.pids.insert(event.pid);
        self.events.push(event);
    }

    pub fn save_graph(&self, path: &str) {
        let mut figure = Figure::new();
        {
            let axes = figure.axes2d();
            axes.set_x_ticks(Some((AutoOption::Fix(1.0), 0)), &[], &[]);
            axes.set_x_grid(true);
            axes.set_margins(&[MarginSide::MarginTop(0.05), MarginSide::MarginBottom(0.85)]);
            for pid in self.pids.iter() {
                let samples = self
                    .events
                    .iter()
                    .enumerate()
                    .filter(|(_, x)| x.pid == *pid)
                    .collect::<Vec<_>>();
                let len = samples.len();
                let opts = &[LabelOption::Rotate(90.0)];
                for (i, s) in samples.iter() {
                    let description = format!("{}: {}", s.pid, s.descr);
                    axes.label(
                        &description,
                        Coordinate::Axis(*i as f64),
                        Coordinate::Axis(libc::pid_t::from(s.pid) as f64),
                        opts,
                    );
                }

                let xs = samples.iter().map(|(i, _)| *i).collect::<Vec<_>>();
                let mut ys = Vec::new();
                ys.resize(len, libc::pid_t::from(*pid));

                axes.lines_points(&xs[..], &ys[..], &[]);
            }

            let samples = self
                .events
                .iter()
                .enumerate()
                .filter(|(_, x)| x.child.is_some())
                .collect::<Vec<_>>();
            for (i, s) in samples.iter() {
                let child = libc::pid_t::from(s.child.unwrap());
                let x_end = self
                    .events
                    .iter()
                    .enumerate()
                    .find(|(_, x)| x.pid == Pid::from_raw(child))
                    .map_or_else(|| *i + 1, |(idx, _)| idx);
                let x = &[*i, *i + 1, x_end];
                let y = &[libc::pid_t::from(s.pid), child, child];

                axes.lines_points(x, y, &[]);
            }
        }
        figure.set_terminal("pngcairo size 3840,2160", path);
        figure.show().close();
    }
}
