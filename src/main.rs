//! octofan-watchdog: keeps an Octominer rig's fan-controller board from
//! resetting it, by feeding the board's hardware watchdog the way OctOS and
//! HiveOS do. Without them (plain Linux), nothing feeds it and the board
//! resets the rig every 25 minutes.
//!
//! The watchdog still does its job: if this machine hangs (or the health
//! checks fail), the feeding stops and the board resets the rig.

mod octofan;
mod usb;

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use octofan::Board;

const USAGE: &str = "\
octofan-watchdog: feeds the Octominer fan-controller board's hardware watchdog

Usage:
  octofan-watchdog status                 the board and its watchdog state
  octofan-watchdog feed                   feed the watchdog once
  octofan-watchdog set-timeouts SHORT LONG
                                          set the timeouts (seconds; LONG is
                                          kept in minutes, 60 s at least)
  octofan-watchdog run [options]          feed it until stopped (the service)

run options:
  --interval SECS        seconds between feeds (default 10)
  --check HOST:PORT      feed only while this TCP port accepts connections
                         (repeatable; e.g. 127.0.0.1:22 as HiveOS does)
  --set-timeouts S,L     set the timeouts first (e.g. 120,1500 as HiveOS)

Needs write access to the board's /dev/bus/usb node (root).
";

macro_rules! log {
    ($($a:tt)*) => { println!($($a)*) };
}

fn open() -> Result<Board, String> {
    let found = octofan::find();
    let Some(at) = found.into_iter().next() else {
        return Err(format!(
            "no Octominer fan controller ({:04x}:{:04x} \"{}\" \"{}\") on USB",
            octofan::VID,
            octofan::PID,
            octofan::MANUFACTURER,
            octofan::PRODUCT
        ));
    };
    let node = at.node.clone();
    Board::open(at).map_err(|e| format!("{}: {e}", node.display()))
}

fn parse_timeouts(s: &str, l: &str) -> Result<(u16, u32), String> {
    let short: u16 = s
        .parse()
        .map_err(|_| format!("bad short timeout {s:?}"))?;
    let long: u32 = l.parse().map_err(|_| format!("bad long timeout {l:?}"))?;
    if short == 0 {
        return Err("the short timeout must be above 0".into());
    }
    octofan::long_index(long)?;
    Ok((short, long))
}

/// `run`'s settings.
struct RunConfig {
    interval: Duration,
    checks: Vec<String>,
    timeouts: Option<(u16, u32)>,
}

fn parse_run(args: &[String]) -> Result<RunConfig, String> {
    let mut c = RunConfig {
        interval: Duration::from_secs(10),
        checks: Vec::new(),
        timeouts: None,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().cloned().ok_or(format!("{name} needs a value"));
        match a.as_str() {
            "--interval" => {
                let v = val("--interval")?;
                let s: f64 = v.parse().map_err(|_| format!("bad --interval {v:?}"))?;
                if !(1.0..=60.0).contains(&s) {
                    return Err("--interval: 1 to 60 seconds".into());
                }
                c.interval = Duration::from_secs_f64(s);
            }
            "--check" => c.checks.push(val("--check")?),
            "--set-timeouts" => {
                let v = val("--set-timeouts")?;
                let (s, l) = v
                    .split_once(',')
                    .ok_or(format!("--set-timeouts SHORT,LONG, not {v:?}"))?;
                c.timeouts = Some(parse_timeouts(s, l)?);
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    if let Some((short, _)) = c.timeouts
        && c.interval.as_secs_f64() * 3.0 > f64::from(short)
    {
        return Err(format!(
            "--interval {:?} is too long for a {short} s short timeout (feed at least 3 times per timeout)",
            c.interval
        ));
    }
    Ok(c)
}

/// The first failing check, if any.
fn failing_check(checks: &[String]) -> Option<String> {
    for c in checks {
        let addrs: Vec<SocketAddr> = match c.to_socket_addrs() {
            Ok(a) => a.collect(),
            Err(e) => return Some(format!("{c}: {e}")),
        };
        let ok = addrs
            .iter()
            .any(|a| TcpStream::connect_timeout(a, Duration::from_secs(1)).is_ok());
        if !ok {
            return Some(format!("{c}: not accepting connections"));
        }
    }
    None
}

fn run(cfg: RunConfig) -> Result<(), String> {
    log!(
        "octofan-watchdog {}: feeding every {:?}{}",
        env!("CARGO_PKG_VERSION"),
        cfg.interval,
        if cfg.checks.is_empty() {
            String::new()
        } else {
            format!(" while {} answer", cfg.checks.join(", "))
        }
    );
    let mut board: Option<Board> = None;
    let mut fed = 0u64;
    let mut first = true;
    let mut last_problem: Option<String> = None;
    let mut last_problem_log = Instant::now() - Duration::from_secs(3600);
    let mut last_summary = Instant::now();
    loop {
        // (Re)open the board when needed.
        if board.is_none() {
            match open() {
                Ok(b) => {
                    log!("board at {} ({})", b.at.node.display(), b.at.sysfs.display());
                    match b.status() {
                        Ok(s) => log!("watchdog: {s}"),
                        Err(e) => log!("watchdog status: {e}"),
                    }
                    if let Some((s, l)) = cfg.timeouts {
                        match b.set_timeouts(s, l) {
                            Ok(()) => log!("timeouts set: short {s} s, long {l} s"),
                            Err(e) => log!("setting the timeouts failed: {e}"),
                        }
                    }
                    board = Some(b);
                }
                Err(e) => {
                    let p = Some(e);
                    if p != last_problem || last_problem_log.elapsed() >= Duration::from_secs(300) {
                        log!("{}", p.as_deref().unwrap_or_default());
                        last_problem_log = Instant::now();
                    }
                    last_problem = p;
                }
            }
        }
        if let Some(b) = &board {
            let problem = failing_check(&cfg.checks).map(|c| format!("not feeding: {c}"));
            let problem = match problem {
                Some(p) => Some(p),
                None => match b.feed() {
                    Ok(()) => {
                        fed += 1;
                        if first {
                            log!("fed the watchdog; feeding every {:?}", cfg.interval);
                            first = false;
                        }
                        None
                    }
                    Err(e) => {
                        let p = format!("feeding failed: {e}; reopening the board");
                        board = None;
                        Some(p)
                    }
                },
            };
            match &problem {
                Some(p)
                    if problem != last_problem
                        || last_problem_log.elapsed() >= Duration::from_secs(60) =>
                {
                    log!("{p}");
                    last_problem_log = Instant::now();
                }
                None if last_problem.is_some() => log!("feeding again"),
                _ => {}
            }
            last_problem = problem;
        }
        if last_summary.elapsed() >= Duration::from_secs(3600) {
            last_summary = Instant::now();
            let state = board
                .as_ref()
                .and_then(|b| b.status().ok())
                .map_or_else(|| "status unavailable".to_string(), |s| s.to_string());
            log!("fed {fed} times so far; watchdog: {state}");
        }
        std::thread::sleep(cfg.interval);
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("status") => open().and_then(|b| {
            println!("board: {} ({})", b.at.node.display(), b.at.sysfs.display());
            let s = b.status().map_err(|e| format!("watchdog status: {e}"))?;
            println!("watchdog: {s}");
            Ok(())
        }),
        Some("feed") => open().and_then(|b| {
            b.feed().map_err(|e| format!("feeding: {e}"))?;
            println!("fed the watchdog");
            Ok(())
        }),
        Some("set-timeouts") => match (args.get(1), args.get(2)) {
            (Some(s), Some(l)) => parse_timeouts(s, l).and_then(|(s, l)| {
                let b = open()?;
                b.set_timeouts(s, l).map_err(|e| format!("setting the timeouts: {e}"))?;
                println!("timeouts set: short {s} s, long {l} s");
                Ok(())
            }),
            _ => Err("set-timeouts SHORT LONG (seconds)".into()),
        },
        Some("run") => parse_run(&args[1..]).and_then(run),
        Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            Ok(())
        }
        Some("-V" | "--version") => {
            println!("octofan-watchdog {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err(format!("give a command\n\n{USAGE}")),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("octofan-watchdog: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn run_options() {
        let c = parse_run(&args("--interval 5 --check 127.0.0.1:22 --set-timeouts 120,1500")).unwrap();
        assert_eq!(c.interval, Duration::from_secs(5));
        assert_eq!(c.checks, ["127.0.0.1:22"]);
        assert_eq!(c.timeouts, Some((120, 1500)));
        assert_eq!(parse_run(&[]).unwrap().interval, Duration::from_secs(10));
        // Too slow for the short timeout, or nonsense.
        assert!(parse_run(&args("--interval 50 --set-timeouts 120,1500")).is_err());
        assert!(parse_run(&args("--interval 0")).is_err());
        assert!(parse_run(&args("--set-timeouts 0,1500")).is_err());
        assert!(parse_run(&args("--bogus")).is_err());
    }

    #[test]
    fn checks() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let up = l.local_addr().unwrap().to_string();
        assert_eq!(failing_check(std::slice::from_ref(&up)), None);
        drop(l);
        assert!(failing_check(&[up]).is_some());
    }
}
