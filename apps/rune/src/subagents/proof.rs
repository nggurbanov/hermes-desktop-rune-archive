//! Explicit native acceptance lane, restricted to a private synthetic loopback peer.
use super::*;
use std::io::BufReader;
fn output(value: Value) {
    println!("{value}");
}
fn wait(
    manager: &mut Subagents,
    theme: Option<bool>,
    condition: impl Fn(&process::NativeWindow) -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        manager.poll(theme);
        if manager.windows.values().any(&condition) {
            return Ok(());
        }
        if manager.windows.is_empty() {
            return Err("The native spectator exited before its receipt".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err("The native spectator acknowledgement timed out".into())
}
fn next(input: &mut impl std::io::BufRead) -> Result<(), String> {
    let value = process::frame(input)?.ok_or("The acceptance controller closed")?;
    if value["type"] != "next" {
        return Err("Invalid native acceptance step".into());
    }
    Ok(())
}
pub(super) fn run(args: &[String]) -> Result<(), String> {
    let endpoint = args
        .iter()
        .position(|a| a == "--gateway")
        .and_then(|i| args.get(i + 1))
        .ok_or("Choose a private synthetic --gateway endpoint")?;
    let url = url::Url::parse(endpoint).map_err(|_| "Invalid native acceptance endpoint")?;
    if url.scheme() != "http"
        || !url.host_str().is_some_and(|host| {
            host.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
        })
    {
        return Err("Native window acceptance only accepts a numeric loopback HTTP peer".into());
    }
    let connection = Connection::Url {
        endpoint: endpoint.clone(),
        token: None,
    };
    let scope = DraftScope {
        connection: 1,
        profile: "work".into(),
        session: Some("parent".into()),
    };
    let mut manager = Subagents::new();
    manager.event(&scope,"subagent.start",&json!({"subagent_id":"proof-child","child_session_id":"child","goal":"Compare all 80 source images and summarize the differences","task_index":0}));
    manager.open(&connection, &scope, "proof-child", Some(false))?;
    wait(&mut manager, Some(false), |window| window.ready())?;
    let pid = manager
        .windows
        .values()
        .next()
        .ok_or("The spectator registry is empty")?
        .id();
    output(json!({"phase":"opened","pid":pid,"windows":manager.windows.len()}));
    let mut input = BufReader::new(std::io::stdin());
    next(&mut input)?;
    manager.open(&connection, &scope, "proof-child", Some(false))?;
    wait(&mut manager, Some(false), |window| {
        window.focus_receipts > 0
    })?;
    if manager.windows.len() != 1 || manager.windows.values().next().map(|w| w.id()) != Some(pid) {
        return Err("Reopening duplicated the native subagent window".into());
    }
    wait(&mut manager, Some(true), |window| {
        window.theme_receipt == Some(json!(true))
    })?;
    output(
        json!({"phase":"reused","pid":pid,"windows":manager.windows.len(),"focus":true,"dark":true}),
    );
    next(&mut input)?;
    manager.close_profile(1, "work");
    if !manager.windows.is_empty() {
        return Err("Retiring the profile retained its spectator".into());
    }
    output(json!({"phase":"closed","same_pid":true,"windows":0}));
    Ok(())
}
