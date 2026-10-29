//! Serialized native Git calls run off the UI thread and use argument arrays, never a shell.
mod process;
mod shipping;
mod status;
#[cfg(test)]
mod tests;
mod worktrees;
use process::{Git, run_program};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
};
pub struct Worker {
    send: Sender<(u64, u64, String, String, Value)>,
    receive: Receiver<(u64, u64, String, Result<Value, String>)>,
}
impl Worker {
    pub fn new(waker: rune::Waker) -> Self {
        let (send, requests) = mpsc::channel::<(u64, u64, String, String, Value)>();
        let (replies, receive) = mpsc::channel();
        std::thread::spawn(move || {
            for (epoch, generation, op, route, args) in requests {
                let result = call(&route, &args);
                if replies.send((epoch, generation, op, result)).is_err() {
                    break;
                }
                waker();
            }
        });
        Self { send, receive }
    }
    pub fn request(
        &self,
        epoch: u64,
        generation: u64,
        op: String,
        route: String,
        args: Value,
    ) -> Result<(), String> {
        self.send
            .send((epoch, generation, op, route, args))
            .map_err(|_| "The native Git service stopped".into())
    }
    pub fn replies(&self) -> Vec<(u64, u64, String, Result<Value, String>)> {
        self.receive.try_iter().collect()
    }
}
fn call(route: &str, args: &Value) -> Result<Value, String> {
    let path = args["path"]
        .as_str()
        .ok_or("Choose the local repository path first")?;
    let git = Git::new(path)?;
    match route {
        "status" => status::repo_status(&git),
        "review/list" => status::review_list(&git, args),
        "review/diff" | "file-diff" => status::diff(&git, args),
        "review/rev-parse" => Ok(
            json!({"sha":git.optional(&["rev-parse","--verify",args["ref"].as_str().unwrap_or("HEAD")])}),
        ),
        "review/stage" | "review/unstage" | "review/revert" => status::mutate(&git, route, args),
        "review/commit-context" => shipping::context(&git),
        "review/commit" => shipping::commit(&git, args),
        "review/push" => shipping::push(&git),
        "review/ship-info" => shipping::ship_info(&git),
        "review/pr-list" => shipping::pr_list(&git, args),
        "review/create-pr" => shipping::create_pr(&git),
        "worktrees" => worktrees::list(&git),
        "branches" => worktrees::branches(&git, false),
        "base-branches" => worktrees::branches(&git, true),
        "branch/switch" => worktrees::switch(&git, args),
        "worktree/add" => worktrees::add(&git, args),
        "worktree/remove" => worktrees::remove(&git, args),
        _ => Err("Unsupported native Git operation".into()),
    }
}
fn file(args: &Value) -> Result<Option<&str>, String> {
    let Some(file) = args["file"].as_str().filter(|p| !p.is_empty()) else {
        return Ok(None);
    };
    if Path::new(file)
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
        || file.split(['/', '\\']).any(|p| p == ".git" || p == "..")
        || file.contains('\0')
    {
        return Err("Choose a repository-relative file path".into());
    }
    Ok(Some(file))
}
fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").trim().to_owned()
}
