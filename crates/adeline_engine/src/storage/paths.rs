//! Path checks that keep project storage safe: working directories outside it,
//! and real directories and files, not links, before anything is read or deleted.
use super::*;

pub(super) fn outside_project_storage(directory: &Path, root: &Path) -> Result<()> {
    let root_path = fs::canonicalize(root).with_context(|| root.display().to_string())?;
    if directory.starts_with(root)
        || (directory.exists()
            && fs::canonicalize(directory)
                .with_context(|| directory.display().to_string())?
                .starts_with(root_path))
    {
        bail!(
            "{}: working directory cannot be inside Adeline's projects storage folder.",
            directory.display()
        );
    }
    Ok(())
}

pub(super) fn valid_working_directory(directory: &Path) -> Result<()> {
    if !directory.is_absolute() {
        bail!("Choose an absolute working directory.");
    }
    if !directory.is_dir() {
        bail!(
            "{}: working directory does not exist or is not a directory.",
            directory.display()
        );
    }
    Ok(())
}
#[cfg(windows)]
pub(super) fn is_link(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    // Junctions are reparse points too, but FileType::is_symlink can miss them.
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
pub(super) fn is_link(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

pub(super) fn safe_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).with_context(|| path.display().to_string())?;
    if metadata.file_type().is_dir() && !is_link(&metadata) {
        Ok(())
    } else {
        Err(anyhow!(
            "{}: expected a real directory, not a link.",
            path.display()
        ))
    }
}
pub(super) fn safe_conversation_path(file: &Path) -> Result<()> {
    for ancestor in file.ancestors().skip(1).take(4) {
        safe_directory(ancestor)?;
    }
    Ok(())
}

pub(super) fn ensure_regular_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).with_context(|| path.display().to_string())?;
    if metadata.file_type().is_file() && !is_link(&metadata) {
        Ok(())
    } else {
        Err(anyhow!(
            "{}: expected a regular file, not a link.",
            path.display()
        ))
    }
}

pub(super) fn check_contents(folder: &Path, allowed: &[&str]) -> Result<()> {
    for entry in fs::read_dir(folder).with_context(|| folder.display().to_string())? {
        let entry = entry.with_context(|| folder.display().to_string())?;
        let path = entry.path();
        let name = entry.file_name();
        let metadata = fs::symlink_metadata(&path).with_context(|| path.display().to_string())?;
        if !allowed
            .iter()
            .any(|allowed| name == std::ffi::OsStr::new(*allowed))
            || is_link(&metadata)
        {
            bail!(
                "{}: unexpected file or link prevents safe deletion.",
                path.display()
            );
        }
    }
    for name in allowed {
        if !folder.join(name).exists() {
            bail!(
                "{}: missing saved project data.",
                folder.join(name).display()
            );
        }
    }
    Ok(())
}
