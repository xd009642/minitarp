use gnuplot::{AutoOption, AxesCommon, Coordinate, Figure, LabelOption, MarginSide, PlotOption};
use libc::*;
use serde::{Deserialize, Serialize};
use std::cmp::max;
use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;
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

impl Event {
    fn get_pid(&self) -> Option<pid_t> {
        if let Event::Trace(t) = &self {
            t.pid.clone()
        } else {
            None
        }
    }
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

fn generate_palette(len: usize) -> Vec<String> {
    let mut res = vec![];
    let phase_factor = PI * 2.0 / 3.0;
    for i in 0..len {
        let i_f = i as f64;
        let r = ((PI / (len as f64) * 2.0 * i_f).sin() * 127.0).floor() as u8 + 128;
        let g = ((PI / (len as f64) * 2.0 * i_f + phase_factor).sin() * 127.0).floor() as u8 + 128;
        let b = ((PI / (len as f64) * 2.0 * i_f + 2.0 * phase_factor).sin() * 127.0).floor() as u8
            + 128;
        res.push(format!("#{:x}{:x}{:x}", r, g, b));
    }
    res
}

impl EventLog {
    pub fn save_graph(&self, path: &str) {
        let mut figure = Figure::new();
        let pids = self
            .events
            .iter()
            .filter_map(|e| e.get_pid())
            .collect::<HashSet<pid_t>>();
        let mut palette = generate_palette(pids.len() + 1);
        let mut colour_map = HashMap::new();
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
                            axes.label(
                                &trace.description,
                                Coordinate::Axis(i as f64),
                                Coordinate::Axis(pid as f64),
                                opts,
                            );
                            let colour = if colour_map.contains_key(&pid) {
                                let c = colour_map.get(&pid).cloned().unwrap();
                                c
                            } else if !palette.is_empty() {
                                let c = palette.remove(0);
                                colour_map.insert(pid, c.clone());
                                c
                            } else {
                                "#000000".to_string()
                            };
                            if pid < y_min {
                                y_min = pid;
                            }
                            if pid > y_max {
                                y_max = pid;
                            }
                            axes.lines_points(
                                &[i as f64, i as f64 + 1.0],
                                &[pid as f64, pid as f64],
                                &[PlotOption::Color(&colour)],
                            );
                            if let Some(child) = trace.child {
                                let x = &[i as f64 - 0.5, i as f64 + 0.5];
                                let y = &[pid as f64, child as f64];
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
