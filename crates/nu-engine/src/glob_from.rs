#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

use nu_glob::MatchOptions;
use nu_path::{canonicalize_with, expand_path_with};
use nu_protocol::{ShellError, Span, Spanned};

/// This function is like `nu_glob::glob` from the `glob` crate, except it is relative to a given cwd.
///
/// It returns a tuple of two values: the first is an optional prefix that the expanded filenames share.
/// This prefix can be removed from the front of each value to give an approximation of the relative path
/// to the user
///
/// The second of the two values is an iterator over the matching filepaths.
#[allow(clippy::type_complexity)]
pub fn glob_from(
    pattern: &Spanned<String>,
    cwd: &Path,
    span: Span,
    options: Option<MatchOptions>,
) -> Result<
    (
        Option<PathBuf>,
        Box<dyn Iterator<Item = Result<PathBuf, ShellError>> + Send>,
    ),
    ShellError,
> {
    let path = PathBuf::from(&pattern.item);
    let path = expand_path_with(path, cwd);

    let (prefix, pattern) = if path.to_string_lossy().contains('*') {
        // Path is a glob pattern => do not check for existence
        // Select the longest prefix until the first '*'
        let mut p = PathBuf::new();
        for c in path.components() {
            if let Component::Normal(os) = c {
                if os.to_string_lossy().contains('*') {
                    break;
                }
            }
            p.push(c);
        }
        (Some(p), path)
    } else {
        let path = if let Ok(p) = canonicalize_with(path, &cwd) {
            p
        } else {
            return Err(ShellError::DirectoryNotFound(pattern.span, None));
        };

        if path.is_dir() {
            if permission_denied(&path) {
                #[cfg(unix)]
                let error_msg = format!(
                    "The permissions of {:o} do not allow access for this user",
                    path.metadata()
                        .expect("this shouldn't be called since we already know there is a dir")
                        .permissions()
                        .mode()
                        & 0o0777
                );

                #[cfg(not(unix))]
                let error_msg = String::from("Permission denied");

                return Err(ShellError::GenericError(
                    "Permission denied".into(),
                    error_msg,
                    Some(pattern.span),
                    None,
                    Vec::new(),
                ));
            }

            if is_empty_dir(&path) {
                return Ok((Some(path), Box::new(vec![].into_iter())));
            }

            (Some(path.clone()), path.join("*"))
        } else {
            (path.parent().map(|parent| parent.to_path_buf()), path)
        }
    };

    let pattern = pattern.to_string_lossy().to_string();
    let glob_options = options.unwrap_or_else(MatchOptions::new);

    let glob = nu_glob::glob_with(&pattern, glob_options).map_err(|err| {
        nu_protocol::ShellError::GenericError(
            "Error extracting glob pattern".into(),
            err.to_string(),
            Some(span),
            None,
            Vec::new(),
        )
    })?;

    Ok((
        prefix,
        Box::new(glob.map(move |x| match x {
            Ok(v) => Ok(v),
            Err(err) => Err(nu_protocol::ShellError::GenericError(
                "Error extracting glob pattern".into(),
                err.to_string(),
                Some(span),
                None,
                Vec::new(),
            )),
        })),
    ))
}

fn permission_denied(dir: impl AsRef<Path>) -> bool {
    match dir.as_ref().read_dir() {
        Err(e) => matches!(e.kind(), std::io::ErrorKind::PermissionDenied),
        Ok(_) => false,
    }
}

fn is_empty_dir(dir: impl AsRef<Path>) -> bool {
    match dir.as_ref().read_dir() {
        Err(_) => true,
        Ok(mut s) => s.next().is_none(),
    }
}
