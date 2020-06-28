use gnuplot::{AutoOption, AxesCommon, Coordinate, Figure, LabelOption, MarginSide};
use libc::*;
use serde::{Deserialize, Serialize};
use std::cmp::max;
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Deserialize, Serialize)]
pub enum RunType {
    Tests,
    Doctests,
    Benchmarks,
    Examples,
    Lib,
    Bins,
    AllTargets,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub struct TestBinary {
    path: PathBuf,
    ty: Option<RunType>,
    cargo_dir: Option<PathBuf>,
    pkg_name: Option<String>,
    pkg_version: Option<String>,
    pkg_authors: Option<Vec<String>>,
    should_panic: bool,
}

#[derive(Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Event {
    ConfigLaunch(String),
    BinaryLaunch(TestBinary),
    Trace(TraceEvent),
}

#[derive(Clone, Default, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TraceEvent {
    pid: Option<pid_t>,
    child: Option<pid_t>,
    signal: Option<String>,
    addr: Option<u64>,
    return_val: Option<i64>,
    description: String,
}

#[derive(Clone, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EventLog {
    events: Vec<Event>,
}

impl EventLog {
    pub fn save_graph(&self, path: &str) {
        let mut figure = Figure::new();
        let mut pids = HashSet::new();
        let mut y_min = pid_t::max_value();
        let mut y_max = 0;
        {
            let opts = &[LabelOption::Rotate(90.0)];
            let axes = figure.axes2d();
            axes.set_x_ticks(Some((AutoOption::Fix(1.0), 0)), &[], &[]);
            axes.set_x_grid(true);
            axes.set_margins(&[MarginSide::MarginTop(0.05), MarginSide::MarginBottom(0.80)]);
            for (i, event) in self.events.iter().enumerate() {
                match event {
                    Event::ConfigLaunch(name) => {
                        axes.label(
                            &format!("Running config {}", name),
                            Coordinate::Axis(i as f64),
                            Coordinate::Axis(0.0),
                            opts,
                        );
                    }
                    Event::BinaryLaunch(binary) => {
                        axes.label(
                            &format!("Launching {}", binary.path.display()),
                            Coordinate::Axis(i as f64),
                            Coordinate::Axis(0.0),
                            opts,
                        );
                    }
                    Event::Trace(trace) => {
                        if let Some(pid) = trace.pid {
                            pids.insert(pid);
                            axes.label(
                                &trace.description,
                                Coordinate::Axis(i as f64),
                                Coordinate::Axis(pid as f64),
                                opts,
                            );
                            if pid < y_min {
                                y_min = pid;
                            }
                            if pid > y_max {
                                y_max = pid;
                            }
                            axes.lines_points(
                                &[i as f64, i as f64 + 1.0],
                                &[pid as f64, pid as f64],
                                &[],
                            );
                            if let Some(child) = trace.child {
                                let x = &[i as f64 - 0.5, i as f64 + 0.5];
                                let y = &[pid as f64, child as f64];
                                pids.insert(child);
                                axes.lines_points(x, y, &[]);
                            }
                        }
                    }
                }
            }
            axes.set_y_range(
                AutoOption::Fix(y_min as f64 - 0.1),
                AutoOption::Fix(y_max as f64 + 0.5),
            );
            /*  for pid in self.pids.iter() {
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

            // vertical lines!
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
            }*/
        }
        let w = self.events.len() * 20;
        let h = max(pids.len() * 200, 100);
        println!("Events {}, height {}", self.events.len(), pids.len());
        figure.set_terminal(&format!("pngcairo size {},{}", w, h), path);
        figure
            .show()
            .expect("Failed to start GNU plot")
            .wait()
            .expect("GNU plot stalled");
    }
}
