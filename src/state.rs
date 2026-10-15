use std::env;
use std::fs;
use std::path::PathBuf;

/// Location of the file remembering the last picker choice.
fn last_choice_path() -> Option<PathBuf> {
    let base = env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("tylomon").join("last-choice"))
}

/// Key of the target selected last time, if any.
pub fn load_last_choice() -> Option<String> {
    let content = fs::read_to_string(last_choice_path()?).ok()?;
    let key = content.trim();
    (!key.is_empty()).then(|| key.to_string())
}

/// Persist the selected target key. Failures are ignored: remembering is a convenience.
pub fn save_last_choice(key: &str) {
    let Some(path) = last_choice_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, key);
}
