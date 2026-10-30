//! Isolated subreaper fixtures: no SSH executable, user homes or global SIGCHLD policy.
use super::*;
use rustix::process::{Pid, Signal, WaitOptions};
use std::{fs, os::fd::AsRawFd, path::Path, sync::mpsc, thread};
const HELPER: &str = "fleet::inventory::ssh::process_tests::inventory_process_fixture";
const ROLE: &str = "RUNE_TEST_INVENTORY_ROLE";
const ROOT: &str = "RUNE_TEST_INVENTORY_ROOT";
const SCENARIO: &str = "RUNE_TEST_INVENTORY_SCENARIO";

fn contract(scenario: &str) {
    let root = std::env::temp_dir().join(format!("hermes-inventory-tree-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            HELPER,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ROLE, "supervisor")
        .env(ROOT, &root)
        .env(SCENARIO, scenario)
        .output()
        .unwrap();
    fs::remove_dir_all(&root).unwrap();
    assert!(
        output.status.success(),
        "{scenario} fixture failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn inventory_cancellation_terminates_inherited_stderr_helper_without_waiting_for_eof() {
    contract("cancel");
}
#[test]
fn inventory_deadline_terminates_inherited_stderr_helper_without_waiting_for_eof() {
    contract("deadline");
}
#[test]
fn inventory_successful_leader_exit_terminates_inherited_stderr_helper() {
    contract("exit");
}

#[test]
#[ignore = "only executed as an isolated child by the process-tree contracts"]
fn inventory_process_fixture() {
    match std::env::var(ROLE).unwrap().as_str() {
        "helper" => loop {
            thread::park_timeout(Duration::from_secs(60));
        },
        "supervisor" => {
            // This changes only this private supervisor process, never the app
            // or the multithreaded parent test process.
            rustix::process::set_child_subreaper(Pid::from_raw(1)).unwrap();
            supervise(
                Path::new(&std::env::var(ROOT).unwrap()),
                &std::env::var(SCENARIO).unwrap(),
            );
        }
        other => panic!("unexpected private fixture role {other}"),
    }
}
fn supervise(root: &Path, scenario: &str) {
    let record = root.join("owned-pids");
    let script = r#"
cat >/dev/null
RUNE_TEST_INVENTORY_ROLE=helper "$1" --ignored --exact "$4" --nocapture --test-threads=1 >/dev/null &
helper=$!
printf '%s %s\n' "$$" "$helper" > "$2"
printf 'HERMES_INVENTORY_V1\nP\tdefault\nP\tresearch\n'
[ "$3" != exit ] || exit 0
wait "$helper"
"#;
    let mut command = Command::new("sh");
    command
        .args(["-c", script, "private-inventory-fixture"])
        .arg(std::env::current_exe().unwrap())
        .arg(&record)
        .arg(scenario)
        .arg(HELPER)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let children = Arc::new(Children::default());
    let cancelled = Arc::new(AtomicBool::new(false));
    let (sent, completed) = mpsc::channel();
    let started = Instant::now();
    let deadline = started + Duration::from_millis(if scenario == "deadline" { 400 } else { 1500 });
    let worker = {
        let children = children.clone();
        let cancelled = cancelled.clone();
        thread::spawn(move || {
            let result = read_command(command, deadline, cancelled, &children);
            sent.send(result).unwrap();
        })
    };
    let ready = Instant::now() + Duration::from_secs(3);
    let helper = loop {
        if let Ok(value) = fs::read_to_string(&record) {
            if let Some(pid) = value
                .split_whitespace()
                .nth(1)
                .and_then(|v| v.parse::<i32>().ok())
            {
                break Pid::from_raw(pid).unwrap();
            }
        }
        if Instant::now() >= ready {
            children.shutdown();
            panic!("the private leader did not publish its owned helper");
        }
        thread::sleep(Duration::from_millis(2));
    };
    let helper_fd =
        rustix::process::pidfd_open(helper, rustix::process::PidfdFlags::empty()).unwrap();
    if scenario == "cancel" {
        cancelled.store(true, Ordering::Release);
    }
    let observed = completed.recv_timeout(Duration::from_millis(if scenario == "deadline" {
        900
    } else {
        500
    }));
    let settled = Instant::now() + Duration::from_millis(200);
    while observed.is_ok() && !exited(helper_fd.as_raw_fd()) && Instant::now() < settled {
        thread::sleep(Duration::from_millis(2));
    }
    let helper_exited_before_cleanup = exited(helper_fd.as_raw_fd());
    // Always clean up the RED case before asserting. pidfd targets the captured
    // helper incarnation even if it has already exited; no argv/PID guessing.
    let _ = rustix::process::pidfd_send_signal(&helper_fd, Signal::KILL);
    children.shutdown();
    let result = observed.as_ref().ok();
    if observed.is_err() {
        let _ = completed.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    worker.join().unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    while !exited(helper_fd.as_raw_fd()) {
        assert!(
            Instant::now() < until,
            "the private helper was not terminated"
        );
        thread::sleep(Duration::from_millis(2));
    }
    let reaped = rustix::process::waitpid(Some(helper), WaitOptions::NOHANG);
    assert!(
        matches!(reaped, Ok(Some(_)) | Err(rustix::io::Errno::CHILD)),
        "the helper must be reaped by its leader or this isolated subreaper: {reaped:?}"
    );
    assert!(children.is_empty());
    assert!(
        observed.is_ok(),
        "{scenario} remained blocked on an inherited stderr pipe"
    );
    assert!(
        helper_exited_before_cleanup,
        "{scenario} left its owned helper alive"
    );
    if scenario == "exit" {
        assert_eq!(
            result.unwrap().as_ref().unwrap().profiles,
            ["default", "research"]
        );
    } else {
        assert!(
            result
                .unwrap()
                .as_ref()
                .unwrap_err()
                .contains("cancelled or timed out")
        );
    }
}
fn exited(fd: std::os::fd::RawFd) -> bool {
    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }
    unsafe extern "C" {
        fn poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32;
    }
    let mut fds = PollFd {
        fd,
        events: 1,
        revents: 0,
    };
    // Linux pidfd readiness is an exact-incarnation exit notification.
    unsafe { poll(&mut fds, 1, 0) == 1 && fds.revents & 1 != 0 }
}
