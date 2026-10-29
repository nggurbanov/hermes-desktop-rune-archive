//! Cross-process transactions merge only edited keys, never another editor's snapshot.
use super::*;
use std::{
    fs::{File, OpenOptions},
    path::Path,
};
fn lock(path: &Path) -> Result<File, String> {
    let parent = path.parent().ok_or("Invalid draft storage path")?;
    std::fs::create_dir_all(parent).map_err(|_| "Draft storage directory could not be created")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Draft storage privacy could not be established")?;
    }
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path.with_extension("lock"))
        .map_err(|_| "Draft storage lock could not be opened")?;
    file.lock()
        .map_err(|_| "Draft storage could not be locked")?;
    Ok(file)
}
fn read(path: &Path) -> Result<State, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(State {
                version: 4,
                ..Default::default()
            });
        }
        Err(_) => return Err("Saved drafts could not be read".into()),
    };
    let state: State = serde_json::from_slice(&bytes)
        .map_err(|_| "Saved drafts are invalid; their file was retained")?;
    if !matches!(state.version, 1..=4) {
        return Err("Saved drafts use a newer client format".into());
    }
    if state.version == 4 && !state.drafts.is_empty() {
        return Err("Saved drafts contain mixed formats; their file was retained".into());
    }
    let mut keys = HashSet::new();
    for entry in &state.entries {
        if entry.revision > state.revision
            || !keys.insert((entry.editor.clone(), entry.key()))
            || entry.draft.as_ref().is_some_and(|d| d.key() != entry.key())
        {
            return Err("Saved draft revisions are invalid; their file was retained".into());
        }
    }
    let mut keys = HashSet::new();
    if state
        .favorite_entries
        .iter()
        .any(|f| f.revision > state.revision || !keys.insert((&f.provider, &f.model)))
    {
        return Err("Saved favorite revisions are invalid; their file was retained".into());
    }
    let mut slots = HashSet::new();
    if state
        .owner_slots
        .iter()
        .any(|slot| !slots.insert((&slot.window, &slot.profile, &slot.session)))
    {
        return Err("Saved editor slots are ambiguous; their file was retained".into());
    }
    Ok(state)
}
fn revision(state: &mut State) -> Result<u64, String> {
    state.revision = state
        .revision
        .checked_add(1)
        .ok_or("Saved draft revision is exhausted")?;
    Ok(state.revision)
}
fn save(path: &Path, state: &mut State) -> Result<(), String> {
    state.favorites = state
        .favorite_entries
        .iter()
        .filter(|f| f.enabled)
        .map(|f| (f.provider.clone(), f.model.clone()))
        .collect();
    state.favorites.sort();
    let bytes = serde_json::to_vec(state).map_err(|_| "Drafts could not be encoded")?;
    crate::client::private_write(path, &bytes)
}
fn migrate(state: &mut State, editor: &EditorId) -> Result<bool, String> {
    if state.version == 4 {
        return Ok(false);
    }
    if !state.entries.is_empty() || !state.favorite_entries.is_empty() {
        return Err("Saved draft migration is ambiguous; their file was retained".into());
    }
    let mut keys = HashSet::new();
    for draft in std::mem::take(&mut state.drafts) {
        let key = draft.key();
        if !keys.insert(key.clone()) {
            return Err("Saved drafts contain duplicate owners; their file was retained".into());
        }
        let at = revision(state)?;
        state.entries.push(Entry {
            editor: editor.clone(),
            profile: key.profile,
            session: key.session,
            revision: at,
            draft: Some(draft),
        });
    }
    let mut keys = HashSet::new();
    for (provider, model) in std::mem::take(&mut state.favorites) {
        if !keys.insert((provider.clone(), model.clone())) {
            continue;
        }
        let at = revision(state)?;
        state.favorite_entries.push(Favorite {
            provider,
            model,
            revision: at,
            enabled: true,
        });
    }
    state.version = 4;
    Ok(true)
}
pub(super) fn load(path: &Path, editor: &EditorId) -> Result<State, String> {
    let _lock = lock(path)?;
    let mut state = read(path)?;
    if migrate(&mut state, editor)? {
        save(path, &mut state)?;
    }
    Ok(state)
}
pub(super) fn commit(
    path: &Path,
    editor: &EditorId,
    before: &Snapshot,
    after: &Snapshot,
    seen: &Observed,
) -> Result<Observed, String> {
    let _lock = lock(path)?;
    let mut state = read(path)?;
    let mut changed = migrate(&mut state, editor)?;
    let mut observed = seen.clone();
    let keys: HashSet<_> = before
        .drafts
        .keys()
        .chain(after.drafts.keys())
        .cloned()
        .collect();
    for key in keys {
        let wanted = after.drafts.get(&key);
        if wanted == before.drafts.get(&key) {
            continue;
        }
        let index = state
            .entries
            .iter()
            .position(|e| &e.editor == editor && e.key() == key);
        let current = index.map(|i| &state.entries[i]);
        let at = current.map_or(0, |e| e.revision);
        if at != seen.drafts.get(&key).copied().unwrap_or(0)
            && current.and_then(|e| e.draft.as_ref()) != wanted
        {
            return Err("This editor has a newer saved draft. Its unsaved text was kept; reopen it before replacing the saved copy.".into());
        }
        if current.and_then(|e| e.draft.as_ref()) == wanted {
            observed.drafts.insert(key, at);
            continue;
        }
        let at = revision(&mut state)?;
        let entry = Entry {
            editor: editor.clone(),
            profile: key.profile.clone(),
            session: key.session.clone(),
            revision: at,
            draft: wanted.cloned(),
        };
        if let Some(index) = index {
            state.entries[index] = entry
        } else {
            state.entries.push(entry)
        }
        observed.drafts.insert(key, at);
        changed = true;
    }
    let keys: HashSet<_> = before.favorites.union(&after.favorites).cloned().collect();
    for key in keys {
        let wanted = after.favorites.contains(&key);
        if wanted == before.favorites.contains(&key) {
            continue;
        }
        let index = state
            .favorite_entries
            .iter()
            .position(|f| (&f.provider, &f.model) == (&key.0, &key.1));
        let current = index.map(|i| &state.favorite_entries[i]);
        let at = current.map_or(0, |f| f.revision);
        if at != seen.favorites.get(&key).copied().unwrap_or(0)
            && current.is_some_and(|f| f.enabled) != wanted
        {
            return Err(
                "Model favorites changed in another window; the newer saved choice was kept."
                    .into(),
            );
        }
        if current.is_some_and(|f| f.enabled) == wanted {
            observed.favorites.insert(key, at);
            continue;
        }
        let at = revision(&mut state)?;
        let favorite = Favorite {
            provider: key.0.clone(),
            model: key.1.clone(),
            revision: at,
            enabled: wanted,
        };
        if let Some(index) = index {
            state.favorite_entries[index] = favorite
        } else {
            state.favorite_entries.push(favorite)
        }
        observed.favorites.insert(key, at);
        changed = true;
    }
    if changed {
        save(path, &mut state)?;
    }
    Ok(observed)
}
pub(super) fn primary(path: &Path) -> Result<EditorId, String> {
    let _lock = lock(path)?;
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "Saved primary editor identity is invalid; its file was retained".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let id = EditorId::new();
            let bytes =
                serde_json::to_vec(&id).map_err(|_| "Editor identity could not be encoded")?;
            crate::client::private_write(path, &bytes)?;
            Ok(id)
        }
        Err(_) => Err("Saved primary editor identity could not be read".into()),
    }
}

pub(super) fn slot(path: &Path, window: &EditorId, key: &Key) -> Result<EditorId, String> {
    let _lock = lock(path)?;
    let mut state = read(path)?;
    let migrated = migrate(&mut state, window)?;
    if let Some(slot) = state.owner_slots.iter().find(|slot| {
        &slot.window == window && slot.profile == key.profile && slot.session == key.session
    }) {
        let editor = slot.editor.clone();
        if migrated {
            save(path, &mut state)?;
        }
        return Ok(editor);
    }
    let editor = EditorId::new();
    revision(&mut state)?;
    state.owner_slots.push(Slot {
        window: window.clone(),
        profile: key.profile.clone(),
        session: key.session.clone(),
        editor: editor.clone(),
    });
    save(path, &mut state)?;
    Ok(editor)
}
pub(super) fn rename_profile(path: &Path, from: &str, to: &str) -> Result<(), String> {
    let _lock = lock(path)?;
    let mut state = read(path)?;
    // Legacy migration belongs to the primary editor, not an arbitrary mutation callback.
    if state.version != 4 {
        return Err("Open saved drafts before renaming their profile".into());
    }
    let mut keys = HashSet::new();
    if state.owner_slots.iter().any(|slot| {
        !keys.insert((
            &slot.window,
            if slot.profile == from {
                to
            } else {
                &slot.profile
            },
            &slot.session,
        ))
    }) {
        return Err("The renamed profile already has a saved editor slot".into());
    }
    let originals: Vec<_> = state
        .entries
        .iter()
        .filter(|e| e.profile == from && e.draft.is_some())
        .cloned()
        .collect();
    if originals.iter().any(|e| {
        state.entries.iter().any(|n| {
            n.editor == e.editor && n.profile == to && n.session == e.session && n.draft.is_some()
        })
    }) {
        return Err("The renamed profile already has saved drafts".into());
    }
    for original in originals {
        let at = revision(&mut state)?;
        let old = state
            .entries
            .iter_mut()
            .find(|e| e.editor == original.editor && e.key() == original.key())
            .unwrap();
        old.draft = None;
        old.revision = at;
        let mut next = original;
        next.profile = to.into();
        next.draft.as_mut().unwrap().profile = to.into();
        next.revision = revision(&mut state)?;
        if let Some(old) = state
            .entries
            .iter_mut()
            .find(|e| e.editor == next.editor && e.key() == next.key())
        {
            *old = next
        } else {
            state.entries.push(next)
        }
    }
    for slot in &mut state.owner_slots {
        if slot.profile == from {
            slot.profile = to.into();
        }
    }
    revision(&mut state)?;
    save(path, &mut state)
}
pub(super) fn delete_profile(path: &Path, profile: &str) -> Result<(), String> {
    let _lock = lock(path)?;
    let mut state = read(path)?;
    if state.version != 4 {
        return Err("Open saved drafts before deleting their profile".into());
    }
    let at = revision(&mut state)?;
    for entry in &mut state.entries {
        if entry.profile == profile {
            entry.draft = None;
            entry.revision = at;
        }
    }
    state.owner_slots.retain(|slot| slot.profile != profile);
    save(path, &mut state)
}
pub(super) fn rehome_slot(
    path: &Path,
    window: &EditorId,
    origin: &Key,
    destination: &Key,
    editor: &EditorId,
) -> Result<(), String> {
    let _lock = lock(path)?;
    let mut state = read(path)?;
    if state.version != 4 {
        return Err("Open saved drafts before rebinding their editor".into());
    }
    state.owner_slots.retain(|slot| {
        !(&slot.window == window
            && slot.profile == origin.profile
            && slot.session == origin.session
            && &slot.editor == editor)
    });
    if !state.owner_slots.iter().any(|slot| {
        &slot.window == window
            && slot.profile == destination.profile
            && slot.session == destination.session
    }) {
        state.owner_slots.push(Slot {
            window: window.clone(),
            profile: destination.profile.clone(),
            session: destination.session.clone(),
            editor: editor.clone(),
        });
    }
    revision(&mut state)?;
    save(path, &mut state)
}
