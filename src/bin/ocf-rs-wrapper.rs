use std::env;
use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};

use log::{error, info};
use signal_hook::iterator::Signals;

const EXIT_CODE_SUCCESS: i32 = 0;
const EXIT_CODE_FAILURE: i32 = 1;
const OCF_SUCCESS: i32 = EXIT_CODE_SUCCESS;

static TERMINATE: AtomicBool = AtomicBool::new(false);

// this should be
// fn main() -> Result<ExitCode> {
// with std::process::ExitCode
// unfortunately we need to support old Rust versions, so create a simple type and a wrapper for main
struct ExitCode {
    code: i32,
}
impl ExitCode {
    fn new(code: i32) -> Self {
        Self { code }
    }
}

fn main() {
    match _main() {
        Ok(c) => std::process::exit(c.code),
        Err(e) => {
            eprintln!("{:#}", e);
            std::process::exit(EXIT_CODE_FAILURE);
        }
    }
}

fn _main() -> Result<ExitCode> {
    setup_logger()?;

    let mut signals = Signals::new(&[libc::SIGINT, libc::SIGTERM])?;
    thread::spawn(move || {
        for _ in signals.forever() {
            TERMINATE.store(true, Ordering::Relaxed);
        }
    });

    let agent = env::var("AGENT").context("'AGENT' has to be set")?;
    let agent = Path::new(&agent);
    let _ = env::var("OCF_ROOT").context("'OCF_ROOT' has to be set")?;
    let ocf_resource_instance =
        env::var("OCF_RESOURCE_INSTANCE").context("'OCF_RESOURCE_INSTANCE' has to be set")?;

    let key = "NOTIFY_SOCKET";
    let notify_socket = env::var(key).ok();
    if notify_socket.is_some() {
        // keep the original, but unset for children
        env::remove_var(key);
    }

    let action = env::args()
        .nth(1)
        .ok_or(anyhow::anyhow!("Could not get action as first argument"))?;

    match action.as_str() {
        "stop" => stop(agent, &ocf_resource_instance, &notify_socket),
        "start-and-monitor" => start_and_monitor(agent, &ocf_resource_instance, &notify_socket),
        _ => Err(anyhow::anyhow!("Action '{action}' not implemented")),
    }
}

fn stop(
    agent: &Path,
    ocf_resource_instance: &str,
    notify_socket: &Option<String>,
) -> Result<ExitCode> {
    // we might get called from ExecStopPost for cleanup a second time, in this case don't execute a second time
    // if we are called from ExecStopPost, we can expect some "magic" systemd variables
    if systemd_done() {
        return Ok(ExitCode::new(EXIT_CODE_SUCCESS));
    }

    let ai = agent_instance(agent, ocf_resource_instance);
    let msg = format!("{ai}: about to exec stop");
    info!("{}", msg);
    if let Some(socket) = notify_socket {
        systemd_notify(socket, &format!("STOPPING=1\nSTATUS={msg}"))?;
    }

    let code = Command::new(agent)
        .arg("stop")
        .status()?
        .code()
        .ok_or(anyhow::anyhow!("{ai},stop: could not get exit code"))?;
    Ok(ExitCode::new(code))
}

fn start_and_monitor(
    agent: &Path,
    ocf_resource_instance: &str,
    notify_socket: &Option<String>,
) -> Result<ExitCode> {
    let ai = agent_instance(agent, ocf_resource_instance);
    let code = Command::new(agent)
        .arg("start")
        .status()?
        .code()
        .ok_or(anyhow::anyhow!("{ai},start: could not get exit code"))?;
    if code != OCF_SUCCESS {
        let msg = format!("{ai},s-a-m,start: FAILED with exit code {code}");
        error!("{}", msg);
        if let Some(socket) = notify_socket {
            systemd_notify(socket, &format!("STATUS={msg}"))?;
        }
        return Ok(ExitCode::new(code));
    }

    let monitor_interval = env::var("monitor_interval").unwrap_or_default();
    let monitor_interval: u64 = match monitor_interval.parse() {
        Ok(i) if i < 5 => 5, // should be sane and empty ranges panic gen_range()
        Ok(i) => i,
        Err(_) => 30,
    };
    let msg = format!("{ai}: monitoring every {monitor_interval} seconds");
    info!("{}", msg);
    if let Some(socket) = notify_socket {
        systemd_notify(socket, &format!("READY=1\nSTATUS={msg}"))?;
    }

    sleep_max(monitor_interval);
    while !TERMINATE.load(Ordering::Relaxed) {
        let output = Command::new(agent).arg("monitor").output()?;
        let code = output.status.code().ok_or(anyhow::anyhow!(
            "{ai},start-and-monitor: could not get status"
        ))?;

        if code == OCF_SUCCESS {
            sleep_max(monitor_interval);
            continue;
        }

        // we failed: write logs and print stdout and stderr and bye
        let msg = format!("{ai},s-a-m,monitor: FAILED with exit code {code}");
        error!("{}", msg);
        if let Some(socket) = notify_socket {
            systemd_notify(socket, &format!("STATUS={msg}"))?;
        }
        error!(
            "stdout: '{}'; stderr: '{}'",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(ExitCode::new(code));
    }

    // got signal, try to stop
    stop(agent, ocf_resource_instance, notify_socket)
}

fn setup_logger() -> Result<()> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {}",
                record.level(),
                record.target(),
                message,
            ))
        })
        .chain(std::io::stdout())
        .apply()?;

    Ok(())
}

fn sleep_max(secs: u64) {
    // 1s steps to break early if signal received
    for _ in 0..secs {
        if TERMINATE.load(Ordering::Relaxed) {
            return;
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn systemd_done() -> bool {
    let exit_code = env::var("EXIT_CODE").unwrap_or_default();
    let exit_status = env::var("EXIT_STATUS").unwrap_or_default();

    exit_code == "exited" && exit_status == "0"
}

fn systemd_notify(socket: &str, msg: &str) -> Result<()> {
    let sock = UnixDatagram::unbound()?;
    let msg_complete = format!("{msg}\n");
    if sock.send_to(msg_complete.as_bytes(), socket)? != msg_complete.len() {
        Err(anyhow::anyhow!(
            "systemd notify: could not completely write '{}' to '{}",
            msg,
            socket
        ))
    } else {
        Ok(())
    }
}

fn agent_instance(agent: &Path, instance: &str) -> String {
    let base = match agent.file_name() {
        Some(base) => base,
        None => agent.as_os_str(),
    }
    .to_string_lossy();

    format!("{base}:{instance}")
}
