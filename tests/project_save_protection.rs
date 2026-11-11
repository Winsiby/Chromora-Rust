use std::{fs, path::Path};

use chromora::core::{
    CoreError, PhotoDocument, import_image, load_project, save_project, set_exposure,
};
use image::{Rgb, RgbImage};
use tempfile::tempdir;

fn document(root: &Path) -> PhotoDocument {
    let source = root.join("原片 source Sample.png");
    RgbImage::from_pixel(3, 2, Rgb([60, 90, 120]))
        .save(&source)
        .unwrap();
    import_image(&source, &root.join("原片副本 originals")).unwrap()
}

fn assert_protected(doc: &PhotoDocument, destination: &Path) {
    let source_before = fs::read(&doc.source_path).unwrap();
    let copy = doc.original_copy_path.as_ref().unwrap();
    let copy_before = fs::read(copy).unwrap();
    let destination_before = fs::read(destination).unwrap();
    let result = save_project(doc, destination);
    assert!(
        matches!(&result, Err(CoreError::ProtectedOriginalDestination(_))),
        "expected original protection for {destination:?}, got {result:?}"
    );
    assert_eq!(fs::read(&doc.source_path).unwrap(), source_before);
    assert_eq!(fs::read(copy).unwrap(), copy_before);
    assert_eq!(fs::read(destination).unwrap(), destination_before);
}

#[test]
fn dot_segment_aliases_of_original_and_copy_are_rejected() {
    let temp = tempdir().unwrap();
    let doc = document(temp.path());
    for original in [&doc.source_path, doc.original_copy_path.as_ref().unwrap()] {
        let parent = original.parent().unwrap();
        fs::create_dir(parent.join("临时 子目录")).unwrap();
        let alias = parent
            .join("临时 子目录")
            .join("..")
            .join(".")
            .join(original.file_name().unwrap());
        assert_protected(&doc, &alias);
    }
}

#[test]
fn relative_and_absolute_aliases_are_rejected_without_changing_process_directory() {
    let cwd = std::env::current_dir().unwrap();
    let temp = tempfile::tempdir_in(&cwd).unwrap();
    let doc = document(temp.path());
    for original in [&doc.source_path, doc.original_copy_path.as_ref().unwrap()] {
        assert_protected(&doc, original.strip_prefix(&cwd).unwrap());
    }

    let mut relative_doc = doc.clone();
    relative_doc.source_path = doc.source_path.strip_prefix(&cwd).unwrap().to_path_buf();
    relative_doc.original_copy_path = Some(
        doc.original_copy_path.as_ref().unwrap().strip_prefix(&cwd).unwrap().to_path_buf(),
    );
    for original in [&doc.source_path, doc.original_copy_path.as_ref().unwrap()] {
        assert_protected(&relative_doc, original);
    }
}

#[cfg(windows)]
#[test]
fn windows_case_aliases_of_original_and_copy_are_rejected() {
    let temp = tempdir().unwrap();
    let doc = document(temp.path());
    for original in [&doc.source_path, doc.original_copy_path.as_ref().unwrap()] {
        let alias = original.with_file_name(
            original.file_name().unwrap().to_str().unwrap().to_ascii_uppercase(),
        );
        assert_ne!(&alias, original);
        assert_protected(&doc, &alias);
    }
}

#[test]
fn hard_links_to_original_and_copy_are_rejected_even_with_project_extension() {
    let temp = tempdir().unwrap();
    let doc = document(temp.path());
    for (index, original) in [&doc.source_path, doc.original_copy_path.as_ref().unwrap()]
        .into_iter()
        .enumerate()
    {
        let alias = temp.path().join(format!("alias_{index}.chromora"));
        fs::hard_link(original, &alias).unwrap();
        assert_protected(&doc, &alias);
    }
}

#[test]
fn distinct_project_file_can_be_created_and_updated_without_extension_restrictions() {
    let temp = tempdir().unwrap();
    let mut doc = document(temp.path());
    let source_before = fs::read(&doc.source_path).unwrap();
    let copy = doc.original_copy_path.clone().unwrap();
    let copy_before = fs::read(&copy).unwrap();
    let project = temp.path().join("合法 工程.data");
    save_project(&doc, &project).unwrap();

    // Identical bytes in a distinct file must not be mistaken for file identity.
    fs::copy(&doc.source_path, &project).unwrap();
    set_exposure(&mut doc, 0.5).unwrap();
    save_project(&doc, &project).unwrap();
    assert_eq!(load_project(&project).unwrap().exposure_ev, 0.5);
    set_exposure(&mut doc, -0.75).unwrap();
    save_project(&doc, &project).unwrap();
    assert_eq!(load_project(&project).unwrap().exposure_ev, -0.75);
    assert_eq!(fs::read(&doc.source_path).unwrap(), source_before);
    assert_eq!(fs::read(copy).unwrap(), copy_before);
}
