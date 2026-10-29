//! A writer submits only its own changed JSON leaves under an OS file lock.
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::{fs::File, path::Path};

/// Loading does not create a file. Callers retain this value as their save baseline.
pub(crate) fn load<T: DeserializeOwned + Default>(
    path: &Path,
    validate: impl Fn(&T) -> Result<(), String>,
) -> Result<T, String> {
    match read(path)? {
        Some(value) => decode(&value, &validate),
        None => {
            let value = T::default();
            validate(&value)?;
            Ok(value)
        }
    }
}

/// Merge changes since `base` into the latest file. Objects merge recursively;
/// arrays/scalars are one field and an absent key is an explicit removal.
/// The caller advances its baseline only after success. Unknown raw fields survive.
pub(crate) fn save_patch<T: Serialize + DeserializeOwned + Default>(
    path: &Path,
    base: &T,
    next: &T,
    validate: impl Fn(&T) -> Result<(), String>,
) -> Result<T, String> {
    save_patch_forced(path, base, next, &[], validate)
}

/// Explicit user choices can name leaves even when `base == next` locally.
/// An absent next leaf removes that key; unrelated and unknown siblings survive.
pub(crate) fn save_patch_forced<T: Serialize + DeserializeOwned + Default>(
    path: &Path,
    base: &T,
    next: &T,
    forced_paths: &[Vec<String>],
    validate: impl Fn(&T) -> Result<(), String>,
) -> Result<T, String> {
    if forced_paths.iter().any(Vec::is_empty) {
        return Err("A forced preference write must name a field".into());
    }
    validate(base)?;
    validate(next)?;
    let before = encode(base)?;
    let after = encode(next)?;
    let _lock = lock(path)?;
    let mut current = match read(path)? {
        Some(value) => value,
        None => encode(&T::default())?,
    };
    let _: T = decode(&current, &validate)?;
    merge(&mut current, &before, &after);
    for path in forced_paths {
        let value = path.iter().try_fold(&after, |value, key| value.get(key));
        force(&mut current, path, value);
    }
    let merged = decode(&current, &validate)?;
    let bytes = serde_json::to_vec_pretty(&current)
        .map_err(|_| "Client preferences could not be encoded")?;
    super::private_write(path, &bytes)?;
    Ok(merged)
}

fn encode(value: &impl Serialize) -> Result<Value, String> {
    let value =
        serde_json::to_value(value).map_err(|_| "Client preferences could not be encoded")?;
    if !value.is_object() {
        return Err("Client preferences must be an object".into());
    }
    Ok(value)
}
fn decode<T: DeserializeOwned>(
    value: &Value,
    validate: &impl Fn(&T) -> Result<(), String>,
) -> Result<T, String> {
    if !value.is_object() {
        return Err("Saved client preferences are invalid; their file was preserved".into());
    }
    let result = serde_json::from_value(value.clone())
        .map_err(|_| "Saved client preferences are invalid; their file was preserved")?;
    validate(&result)?;
    Ok(result)
}
fn read(path: &Path) -> Result<Option<Value>, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "Saved client preferences are invalid; their file was preserved".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => {
            Err("Saved client preferences could not be read; their file was preserved".into())
        }
    }
}
fn lock(path: &Path) -> Result<File, String> {
    let parent = path.parent().ok_or("Invalid client preferences path")?;
    std::fs::create_dir_all(parent)
        .map_err(|_| "Client preferences directory could not be created")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Client preferences privacy could not be established")?;
    }
    let name = path.file_name().ok_or("Invalid client preferences path")?;
    let mut name = name.to_os_string();
    name.push(".lock");
    let mut options = std::fs::OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(parent.join(name))
        .map_err(|_| "Client preferences lock could not be opened")?;
    file.lock()
        .map_err(|_| "Client preferences could not be locked")?;
    Ok(file)
}
fn merge(current: &mut Value, before: &Value, after: &Value) {
    if before == after {
        return;
    }
    let (Some(before), Some(after)) = (before.as_object(), after.as_object()) else {
        *current = after.clone();
        return;
    };
    let Some(current) = current.as_object_mut() else {
        *current = Value::Object(after.clone());
        return;
    };
    merge_object(current, before, after);
}
fn merge_object(
    current: &mut Map<String, Value>,
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) {
    for key in before.keys().filter(|key| !after.contains_key(*key)) {
        current.remove(key);
    }
    for (key, value) in after {
        match before.get(key) {
            Some(previous) if previous == value => {}
            Some(previous) => match current.get_mut(key) {
                Some(current) => merge(current, previous, value),
                // A defaulted object omitted from the actual document needs
                // the complete new object, not an incomplete leaf fragment.
                None => {
                    current.insert(key.clone(), value.clone());
                }
            },
            None => {
                current.insert(key.clone(), value.clone());
            }
        }
    }
}

fn force(current: &mut Value, path: &[String], value: Option<&Value>) {
    let mut current = current;
    for key in &path[..path.len() - 1] {
        if !current.is_object() {
            if value.is_none() {
                return;
            }
            *current = Value::Object(Map::new());
        }
        let object = current.as_object_mut().unwrap();
        if value.is_none() && !object.contains_key(key) {
            return;
        }
        current = object
            .entry(key)
            .or_insert_with(|| Value::Object(Map::new()));
    }
    if !current.is_object() {
        if value.is_none() {
            return;
        }
        *current = Value::Object(Map::new());
    }
    let object = current.as_object_mut().unwrap();
    let key = path.last().unwrap();
    match value {
        Some(value) => {
            object.insert(key.clone(), value.clone());
        }
        None => {
            object.remove(key);
        }
    }
}

#[cfg(test)]
mod tests;
