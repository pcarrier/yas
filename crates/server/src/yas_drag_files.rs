//! Files materialized by native Selection GET, retained for the session so a
//! destination can open a dropped URI after finishing its Wayland offer.

use std::fs::{self, File};
use std::io::{self, Seek, Write};
use std::path::PathBuf;

pub(crate) struct DragFiles {
    directory: tempfile::TempDir,
    files: Vec<Option<(PathBuf, Option<File>)>>,
}

impl DragFiles {
    pub(crate) fn new() -> io::Result<Self> {
        Ok(Self {
            directory: tempfile::Builder::new().prefix("yas-drag-").tempdir()?,
            files: Vec::new(),
        })
    }

    /// Reserve the final paths before hover. Unknown names can be filled in
    /// at DROP; each item has a separate directory, including duplicate names.
    pub(crate) fn prepare(&mut self, names: &[&str]) -> io::Result<Vec<Option<PathBuf>>> {
        self.files.resize_with(names.len(), || None);
        for (index, name) in names.iter().enumerate() {
            if name.is_empty() || self.files[index].is_some() {
                continue;
            }
            // Selection names describe files, not paths to write on the host.
            // Keep only the basename, including when the source uses Windows
            // separators. Hold the created file open through materialization.
            let basename = name.rsplit(['/', '\\']).next().unwrap();
            if basename.is_empty() || basename == "." || basename == ".." {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid drag filename",
                ));
            }
            let directory = self.directory.path().join(index.to_string());
            fs::create_dir(&directory)?;
            let path = directory.join(basename);
            let file = File::create_new(&path)?;
            self.files[index] = Some((path, Some(file)));
        }
        Ok(self
            .files
            .iter()
            .map(|file| file.as_ref().map(|(path, _)| path.clone()))
            .collect())
    }

    /// Called only after every selected body has validated. Write through the
    /// original handle rather than following a path the destination has seen.
    pub(crate) fn write(&mut self, index: usize, bytes: &[u8]) -> io::Result<()> {
        if let Some((_, handle)) = self.files.get_mut(index).and_then(Option::as_mut) {
            let mut file = handle
                .take()
                .ok_or_else(|| io::Error::other("drag file already populated"))?;
            file.rewind()?;
            file.set_len(0)?;
            file.write_all(bytes)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_paths_exist_during_hover_and_hold_the_selected_bytes_at_drop() {
        let mut files = DragFiles::new().unwrap();
        let paths = files.prepare(&["0.png", ""]).unwrap();
        let screenshot = paths[0].as_ref().unwrap();
        assert_eq!(screenshot.file_name().unwrap(), "0.png");
        assert_eq!(fs::read(screenshot).unwrap(), b"");
        assert!(paths[1].is_none());
        let materialized = files.prepare(&["0.png", "0.png"]).unwrap();
        assert_eq!(materialized[0], paths[0]);
        assert_ne!(materialized[0], materialized[1]);
        files.write(0, b"screenshot PNG").unwrap();
        files.write(1, b"another PNG").unwrap();
        assert_eq!(fs::read(screenshot).unwrap(), b"screenshot PNG");
        assert_eq!(
            fs::read(materialized[1].as_ref().unwrap()).unwrap(),
            b"another PNG"
        );
        drop(files);
        assert!(!screenshot.exists());
    }

    #[test]
    fn filenames_cannot_escape_the_drag_directory() {
        let mut files = DragFiles::new().unwrap();
        let paths = files
            .prepare(&["../../shot.png", "C:\\shots\\shot.png"])
            .unwrap();
        for path in paths.into_iter().flatten() {
            assert!(path.starts_with(files.directory.path()));
            assert_eq!(path.file_name().unwrap(), "shot.png");
        }
        assert!(DragFiles::new().unwrap().prepare(&[".."]).is_err());
    }
}
