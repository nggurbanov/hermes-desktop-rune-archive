//! Remote ignore files are read through the captured agent, never the client filesystem.
use super::*;
impl Files {
    pub(super) fn ignore_request(&mut self, path: &str) -> Vec<PageEffect> {
        if self.local {
            return vec![];
        }
        let present = self.folders.get(path).is_some_and(|f| {
            f.entries
                .iter()
                .any(|e| e.name == ".gitignore" && !e.directory)
        });
        let file = format!("{}/.gitignore", path.trim_end_matches(['/', '\\']));
        if !present {
            self.ignore_files.remove(&file);
            return vec![];
        }
        if self
            .ignore_next
            .get(&file)
            .is_some_and(|next| *next > self.clock)
        {
            return vec![];
        }
        self.ignore_next.insert(file.clone(), self.clock + 10.);
        let Some(owner) = &self.owner else {
            return vec![];
        };
        vec![(
            format!("files.ignore.{}", crate::pages::segment(&file)),
            self.read_request("/api/fs/read-text", &file, owner),
        )]
    }
    pub(super) fn ignore_reply(&mut self, op: &str, result: Result<Value, String>) {
        let Some(encoded) = op.strip_prefix("files.ignore.") else {
            return;
        };
        let file = decode(encoded);
        match result {
            Ok(v) if v["truncated"] != true && v["binary"] != true => {
                if let Some(text) = v["text"].as_str() {
                    self.ignore_files.insert(file, text.into());
                }
            }
            _ => {
                if let Some(parent) = std::path::Path::new(&file).parent() {
                    if let Some(folder) = self.folders.get_mut(parent.to_string_lossy().as_ref()) {
                        folder.error=Some("Ignore rules could not be refreshed; the last confirmed rules are retained".into());
                    }
                }
            }
        }
    }
    pub(super) fn remote_ignore(&self, directory: &str) -> Option<::ignore::gitignore::Gitignore> {
        if self.local || self.ignore_files.is_empty() {
            return None;
        }
        let root = self.root.replace('\\', "/");
        let directory = directory.replace('\\', "/");
        let mut rules: Vec<_> = self
            .ignore_files
            .iter()
            .filter(|(p, _)| {
                std::path::Path::new(&directory).starts_with(
                    std::path::Path::new(&p.replace('\\', "/"))
                        .parent()
                        .unwrap_or(std::path::Path::new("")),
                )
            })
            .collect();
        rules.sort_by_key(|(p, _)| p.len());
        let mut builder = ::ignore::gitignore::GitignoreBuilder::new(&root);
        for (file, text) in rules {
            for line in text.lines() {
                let _ = builder.add_line(
                    Some(std::path::PathBuf::from(file.replace('\\', "/"))),
                    line,
                );
            }
        }
        builder.build().ok()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_ignore_reads_stay_on_owner_and_apply_nested_overrides() {
        let owner = DraftScope {
            connection: 1,
            profile: "research".into(),
            session: Some("stored".into()),
        };
        let mut state = Files::new();
        state.bind(owner, false, Some("/remote/project".into()));
        let effects=state.apply("files.tree.%2Fremote%2Fproject",Ok(json!({"entries":[{"name":".gitignore","path":"/remote/project/.gitignore","isDirectory":false},{"name":"hide.log","path":"/remote/project/hide.log","isDirectory":false},{"name":"keep.log","path":"/remote/project/keep.log","isDirectory":false},{"name":".editorconfig","path":"/remote/project/.editorconfig","isDirectory":false}]})));
        assert_eq!(effects.len(), 1);
        assert!(matches!(effects[0].1.method, crate::pages::PageMethod::Get));
        assert_eq!(effects[0].1.profile.as_deref(), Some("research"));
        assert!(
            effects[0]
                .1
                .query
                .iter()
                .any(|(k, v)| k == "session_id" && v == "stored")
        );
        state.ignore_reply(&effects[0].0, Ok(json!({"text":"*.log\n!keep.log\n"})));
        let names: Vec<_> = state
            .visible("/remote/project")
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert!(!names.contains(&"hide.log"));
        assert!(names.contains(&"keep.log"));
        assert!(names.contains(&".editorconfig"));
        state.show_ignored = true;
        assert_eq!(state.visible("/remote/project").len(), 4);
    }
}
