use std::{error::Error, path::Path};

pub fn open_in_file_manager<P>(path: P) -> Result<(), String>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();

    if !path.exists() {
        return Err("File or directory does not exist".into());
    }

    if path.is_dir() {
        return open_dir_with_fallback(path, open_with_opener, open_with_explorer);
    } else {
        showfile::show_path_in_file_manager(path);
        return Ok(());
    }
}

// opener::open uses ShellExecute's "open" verb, which fails (SE_ERR_NOASSOC)
// when a broken `Directory\shell\open` registry key exists, so fall back.
fn open_dir_with_fallback<F, G>(path: &Path, primary: F, fallback: G) -> Result<(), String>
where
    F: FnOnce(&Path) -> Result<(), String>,
    G: FnOnce(&Path) -> Result<(), String>,
{
    let primary_err = match primary(path) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };

    log::warn!(
        "Failed to open directory, trying fallback ({}): {}",
        path.display(),
        primary_err
    );

    fallback(path).map_err(|fallback_err| {
        let err = format!("{primary_err} / fallback: {fallback_err}");
        log::error!("Failed to open directory ({}): {}", path.display(), err);
        err
    })
}

fn open_with_opener(path: &Path) -> Result<(), String> {
    opener::open(path).map_err(|e| match e.source() {
        Some(source) => format!("{e}: {source}"),
        None => e.to_string(),
    })
}

#[cfg(windows)]
fn open_with_explorer(path: &Path) -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn open_with_explorer(_path: &Path) -> Result<(), String> {
    Err("no fallback available on this platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn falls_back_when_primary_fails() {
        let fallback_called = Cell::new(false);

        let result = open_dir_with_fallback(
            Path::new("C:/dummy"),
            |_| Err("IO error: no association".to_string()),
            |_| {
                fallback_called.set(true);
                Ok(())
            },
        );

        assert_eq!(result, Ok(()));
        assert!(fallback_called.get());
    }

    #[test]
    fn does_not_call_fallback_when_primary_succeeds() {
        let fallback_called = Cell::new(false);

        let result = open_dir_with_fallback(
            Path::new("C:/dummy"),
            |_| Ok(()),
            |_| {
                fallback_called.set(true);
                Ok(())
            },
        );

        assert_eq!(result, Ok(()));
        assert!(!fallback_called.get());
    }

    #[test]
    fn reports_both_errors_when_both_fail() {
        let result = open_dir_with_fallback(
            Path::new("C:/dummy"),
            |_| Err("IO error: no association".to_string()),
            |_| Err("explorer not found".to_string()),
        );

        let err = result.unwrap_err();
        assert!(err.contains("IO error: no association"), "{err}");
        assert!(err.contains("explorer not found"), "{err}");
    }
}
