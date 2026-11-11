use std::{io::ErrorKind, path::{Path, PathBuf}};
use super::{CoreError, CoreResult, PhotoDocument};
pub use super::workspace::{WorkspaceDocument, load_workspace, save_workspace};

pub fn save_project(doc: &PhotoDocument, path: &Path) -> CoreResult<()> {
    save_workspace(&WorkspaceDocument { name: String::new(), photos: vec![doc.clone()], current_index: 0 }, path)
}

fn canonicalize_if_exists(path: &Path) -> std::io::Result<Option<PathBuf>> {
    match path.canonicalize() {
        Ok(path) => Ok(Some(path)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

// Resolve existing ancestors as well, so a missing filename still has a stable identity.
fn destination_identity(path: &Path) -> std::io::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut current = absolute.as_path();
    let mut missing = Vec::new();
    loop {
        if let Some(mut resolved) = canonicalize_if_exists(current)? {
            for component in missing.iter().rev() { resolved.push(component); }
            return Ok(resolved);
        }
        // A removed source drive can be absent while its managed copy is valid.
        let Some(name) = current.file_name() else { return Ok(absolute); };
        missing.push(name.to_os_string());
        let Some(parent) = current.parent() else { return Ok(absolute); };
        current = parent;
    }
}

fn same_destination(left: &Path, right: &Path) -> std::io::Result<bool> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name="kernel32")]
        unsafe extern "system" { fn CompareStringOrdinal(left:*const u16,left_len:i32,right:*const u16,right_len:i32,ignore_case:i32)->i32; }
        let left:Vec<u16>=left.as_os_str().encode_wide().collect();
        let right:Vec<u16>=right.as_os_str().encode_wide().collect();
        let left_len=i32::try_from(left.len()).map_err(|_|std::io::Error::new(ErrorKind::InvalidInput,"路径过长"))?;
        let right_len=i32::try_from(right.len()).map_err(|_|std::io::Error::new(ErrorKind::InvalidInput,"路径过长"))?;
        let comparison=unsafe {CompareStringOrdinal(left.as_ptr(),left_len,right.as_ptr(),right_len,1)};
        if comparison==0 {Err(std::io::Error::last_os_error())}else{Ok(comparison==2)}
    }
    #[cfg(not(windows))]
    { Ok(left==right) }
}

pub(crate) fn protect_original_destinations(doc: &PhotoDocument, destination: &Path) -> CoreResult<()> {
    let absolute_destination = std::path::absolute(destination)?;
    let canonical_destination = canonicalize_if_exists(destination)?;
    let identity = destination_identity(destination)?;
    for original in std::iter::once(doc.source_path.as_path())
        .chain(doc.original_copy_path.as_deref())
        .chain(doc.render_source_path.as_deref())
        .chain(doc.lut_path.as_deref())
    {
        let protected = if absolute_destination == std::path::absolute(original)?
            || same_destination(&identity, &destination_identity(original)?)? {
            true
        } else if let (Some(destination), Some(original)) = (
            canonical_destination.as_ref(),
            canonicalize_if_exists(original)?,
        ) {
            // Canonical paths resolve aliases; file identity also catches hard links.
            destination == &original || same_file::is_same_file(destination, &original)?
        } else {
            false
        };
        if protected {
            return Err(CoreError::ProtectedOriginalDestination(destination.to_path_buf()));
        }
    }
    Ok(())
}

pub fn load_project(path: &Path) -> CoreResult<PhotoDocument> {
    let mut workspace = load_workspace(path)?;
    Ok(workspace.photos.remove(workspace.current_index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{import_image, render_preview, set_exposure};
    use image::{Rgb, RgbImage};
    use std::{fs, path::PathBuf};
    use tempfile::tempdir;

    #[test]
    fn project_round_trip_keeps_pixels_exposure_and_original_copy() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("fixture.png");
        let source_img = RgbImage::from_fn(3, 2, |x, y| Rgb([(x * 50) as u8, (y * 70) as u8, 20]));
        source_img.save(&source).unwrap();
        let originals = temp.path().join("originals");
        let mut doc = import_image(&source, &originals).unwrap();
        set_exposure(&mut doc, 0.5).unwrap();
        let project_path = temp.path().join("project.chromora");
        save_project(&doc, &project_path).unwrap();
        let reopened = load_project(&project_path).unwrap();
        assert_eq!(reopened.exposure_ev, 0.5);
        assert_eq!(reopened.width(), 3);
        assert_eq!(reopened.height(), 2);
        assert_eq!(
            render_preview(&doc).unwrap(),
            render_preview(&reopened).unwrap()
        );
        assert_eq!(reopened.source_sha256, doc.source_sha256);
        assert!(reopened.original_copy_path.unwrap().is_file());
    }

    #[test]
    fn source_modified_after_project_save_is_rejected() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("fixture.png");
        RgbImage::from_pixel(1, 1, Rgb([1, 2, 3]))
            .save(&source)
            .unwrap();
        let originals = temp.path().join("originals");
        let doc = import_image(&source, &originals).unwrap();
        let project_path = temp.path().join("p.chromora");
        save_project(&doc, &project_path).unwrap();
        // Remove the managed copy too, so only the changed user source remains.
        fs::remove_file(doc.original_copy_path.unwrap()).unwrap();
        RgbImage::from_pixel(1, 1, Rgb([4, 5, 6]))
            .save(&source)
            .unwrap();
        assert!(matches!(
            load_project(&project_path),
            Err(CoreError::MissingOrChangedSource(_))
        ));
    }

    #[test]
    fn project_with_unknown_schema_is_rejected() {
        let temp = tempdir().unwrap();
        let path: PathBuf = temp.path().join("bad.chromora");
        fs::write(&path, r#"{"schema_version":999}"#).unwrap();
        assert!(matches!(
            load_project(&path),
            Err(CoreError::UnsupportedProjectVersion(999))
        ));
    }
}
