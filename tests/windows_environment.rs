//! Supplemental Windows checks; all files are generated inside tempfile directories.
//! Copy into tests/ and run: cargo test --test windows_environment -- --nocapture --test-threads=1
//! Run the optional measurement separately with: --ignored --nocapture --test-threads=1

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
    thread,
    time::Instant,
};

use chromora::core::{
    CoreError, DEFAULT_JPEG_QUALITY, PhotoDocument, export_jpeg, import_image, load_project,
    open_image, render_preview, save_project, set_exposure,
};
use image::{ImageDecoder, ImageReader, Rgb, RgbImage};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn file_sha256(path: &Path) -> String {
    chromora::core::hex_lower(&Sha256::digest(fs::read(path).unwrap()))
}

fn make_document(root: &Path) -> (PathBuf, PhotoDocument) {
    fs::create_dir_all(root).unwrap();
    let source = root.join("原片 sample.png");
    RgbImage::from_fn(32, 24, |x, y| Rgb([(x * 7) as u8, (y * 9) as u8, 80]))
        .save(&source)
        .unwrap();
    let doc = import_image(&source, &root.join("原片副本 originals")).unwrap();
    (source, doc)
}

#[test]
fn unicode_space_paths_round_trip_preserves_originals_and_updates_project() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("中文 空格目录").join("人像与风景");
    let (source, mut doc) = make_document(&root);
    let copy = doc.original_copy_path.clone().unwrap();
    let source_hash = file_sha256(&source);
    let copy_hash = file_sha256(&copy);
    assert_eq!(source_hash, copy_hash);
    let original_pixels = doc.original_rgb.as_ref().clone();
    let project = root.join("工程 保存").join("照片 project.chromora");

    set_exposure(&mut doc, 0.5).unwrap();
    save_project(&doc, &project).unwrap();
    set_exposure(&mut doc, -0.75).unwrap();
    save_project(&doc, &project).unwrap();
    let reopened = load_project(&project).unwrap();
    assert_eq!(reopened.exposure_ev, -0.75);
    assert_eq!((reopened.width(), reopened.height()), (32, 24));
    assert_eq!(render_preview(&reopened).unwrap(), render_preview(&doc).unwrap());
    assert_eq!(reopened.original_rgb.as_ref(), &original_pixels);

    let output = root.join("导出 文件").join("成片 sample.jpg");
    export_jpeg(&reopened, &output, DEFAULT_JPEG_QUALITY).unwrap();
    assert_eq!(image::open(&output).unwrap().to_rgb8().dimensions(), (32, 24));
    assert_eq!(file_sha256(&source), source_hash);
    assert_eq!(file_sha256(&copy), copy_hash);
}

#[test]
fn valid_managed_copy_recovers_missing_or_changed_source() {
    let temp = tempdir().unwrap();
    let (source, mut doc) = make_document(temp.path());
    let copy = doc.original_copy_path.clone().unwrap();
    let copy_hash = file_sha256(&copy);
    set_exposure(&mut doc, 0.5).unwrap();
    let expected = render_preview(&doc).unwrap();
    let project = temp.path().join("copy_recovery.chromora");
    save_project(&doc, &project).unwrap();

    fs::remove_file(&source).unwrap();
    let reopened = load_project(&project).unwrap();
    assert_eq!(reopened.source_path, source);
    assert_eq!(render_preview(&reopened).unwrap(), expected);
    assert!(!source.exists());

    fs::write(&source, b"changed temporary source").unwrap();
    assert_eq!(render_preview(&load_project(&project).unwrap()).unwrap(), expected);
    assert_eq!(fs::read(&source).unwrap(), b"changed temporary source");
    assert_eq!(file_sha256(&copy), copy_hash);
}

#[test]
fn valid_source_recovers_missing_or_corrupted_managed_copy() {
    let temp = tempdir().unwrap();
    let (source, doc) = make_document(temp.path());
    let copy = doc.original_copy_path.clone().unwrap();
    let source_hash = file_sha256(&source);
    let expected = render_preview(&doc).unwrap();
    let project = temp.path().join("source_recovery.chromora");
    save_project(&doc, &project).unwrap();

    fs::remove_file(&copy).unwrap();
    assert_eq!(render_preview(&load_project(&project).unwrap()).unwrap(), expected);
    assert!(!copy.exists());

    fs::write(&copy, b"corrupted temporary copy").unwrap();
    assert_eq!(render_preview(&load_project(&project).unwrap()).unwrap(), expected);
    assert_eq!(fs::read(&copy).unwrap(), b"corrupted temporary copy");
    assert_eq!(file_sha256(&source), source_hash);
}

#[test]
fn project_rejects_both_source_and_copy_changed() {
    let temp = tempdir().unwrap();
    let (source, doc) = make_document(temp.path());
    let copy = doc.original_copy_path.clone().unwrap();
    let project = temp.path().join("both_changed.chromora");
    save_project(&doc, &project).unwrap();
    fs::write(&source, b"changed source").unwrap();
    fs::write(&copy, b"changed copy").unwrap();
    assert!(matches!(load_project(&project), Err(CoreError::MissingOrChangedSource(_))));
    assert_eq!(fs::read(&source).unwrap(), b"changed source");
    assert_eq!(fs::read(&copy).unwrap(), b"changed copy");
}

#[test]
fn malformed_and_tampered_projects_are_rejected_without_touching_images() {
    let temp = tempdir().unwrap();
    let (source, doc) = make_document(temp.path());
    let copy = doc.original_copy_path.clone().unwrap();
    let source_hash = file_sha256(&source);
    let copy_hash = file_sha256(&copy);
    let valid_path = temp.path().join("valid.chromora");
    save_project(&doc, &valid_path).unwrap();
    let valid_bytes = fs::read(&valid_path).unwrap();
    let valid: Value = serde_json::from_slice(&valid_bytes).unwrap();
    let bad_path = temp.path().join("bad.chromora");

    for bytes in [b"".as_slice(), b"{\"schema_version\":1,".as_slice()] {
        fs::write(&bad_path, bytes).unwrap();
        let result = load_project(&bad_path);
        assert!(matches!(&result, Err(CoreError::Project(_))), "{result:?}");
        assert_eq!(fs::read(&bad_path).unwrap(), bytes);
    }
    for version in [Value::Null, json!(-1), json!(4_294_967_296_u64)] {
        let mut value = valid.clone();
        value["schema_version"] = version;
        fs::write(&bad_path, serde_json::to_vec(&value).unwrap()).unwrap();
        let result = load_project(&bad_path);
        assert!(matches!(&result, Err(CoreError::InvalidProject(_))), "{result:?}");
    }
    let mut missing_version = valid.clone();
    missing_version.as_object_mut().unwrap().remove("schema_version");
    fs::write(&bad_path, serde_json::to_vec(&missing_version).unwrap()).unwrap();
    assert!(matches!(load_project(&bad_path), Err(CoreError::InvalidProject(_))));

    let mut unknown_version = valid.clone();
    unknown_version["schema_version"] = json!(999);
    fs::write(&bad_path, serde_json::to_vec(&unknown_version).unwrap()).unwrap();
    assert!(matches!(load_project(&bad_path), Err(CoreError::UnsupportedProjectVersion(999))));

    for ev in [3.05, 0.03] {
        let mut value = valid.clone();
        value["photos"][0]["settings"]["exposure"] = json!(ev);
        fs::write(&bad_path, serde_json::to_vec(&value).unwrap()).unwrap();
        let result = load_project(&bad_path);
        assert!(matches!(&result, Err(CoreError::InvalidExposure(_))), "{result:?}");
    }
    let mut wrong_size = valid;
    wrong_size["photos"][0]["width"] = json!(33);
    fs::write(&bad_path, serde_json::to_vec(&wrong_size).unwrap()).unwrap();
    assert!(matches!(load_project(&bad_path), Err(CoreError::MissingOrChangedSource(_))));
    assert_eq!(fs::read(&valid_path).unwrap(), valid_bytes);
    assert_eq!(file_sha256(&source), source_hash);
    assert_eq!(file_sha256(&copy), copy_hash);
}

#[cfg(windows)]
#[test]
fn windows_exclusive_project_lock_preserves_previous_file_on_save_failure() {
    use std::os::windows::fs::OpenOptionsExt;

    let temp = tempdir().unwrap();
    let (_, mut doc) = make_document(temp.path());
    let project = temp.path().join("locked.chromora");
    set_exposure(&mut doc, 0.25).unwrap();
    save_project(&doc, &project).unwrap();
    let before = fs::read(&project).unwrap();
    let entries_before = fs::read_dir(temp.path()).unwrap().count();
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&project)
        .unwrap();
    set_exposure(&mut doc, 0.5).unwrap();
    let result = save_project(&doc, &project);
    drop(lock);

    assert!(matches!(&result, Err(CoreError::Io(_))), "{result:?}");
    assert_eq!(fs::read(&project).unwrap(), before);
    assert_eq!(load_project(&project).unwrap().exposure_ev, 0.25);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), entries_before);
}

#[test]
fn jpeg_quality_dimensions_and_embedded_profile_boundaries() {
    let temp = tempdir().unwrap();
    let (_, doc) = make_document(temp.path());
    for quality in [1, 100] {
        let output = temp.path().join(format!("quality_{quality}.jpg"));
        export_jpeg(&doc, &output, quality).unwrap();
        assert_eq!(image::open(&output).unwrap().to_rgb8().dimensions(), (32, 24));
        let mut decoder = ImageReader::open(&output)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .into_decoder()
            .unwrap();
        let profile = decoder.icc_profile().unwrap().unwrap();
        moxcms::ColorProfile::new_from_slice(&profile).unwrap();
        let before = file_sha256(&output);
        assert!(matches!(export_jpeg(&doc, &output, quality), Err(CoreError::Io(ref e))
            if e.kind() == std::io::ErrorKind::AlreadyExists));
        assert_eq!(file_sha256(&output), before);
    }
    let invalid = temp.path().join("quality_zero.jpg");
    assert!(matches!(export_jpeg(&doc, &invalid, 0), Err(CoreError::InvalidJpeg(_))));
    assert!(!invalid.exists());

    for (width, height) in [(65_536, 1), (1, 65_536)] {
        let source = temp.path().join(format!("oversize_{width}_{height}.png"));
        RgbImage::from_pixel(width, height, Rgb([80, 90, 100])).save(&source).unwrap();
        let oversized = open_image(&source).unwrap();
        let output = source.with_extension("jpg");
        assert!(matches!(export_jpeg(&oversized, &output, 96), Err(CoreError::JpegDimensionsTooLarge)));
        assert!(!output.exists());
    }
}

#[test]
fn concurrent_jpeg_exports_publish_one_complete_file_without_overwrite() {
    let temp = tempdir().unwrap();
    let (_, mut first) = make_document(temp.path());
    let mut second = first.clone();
    set_exposure(&mut first, -1.0).unwrap();
    set_exposure(&mut second, 1.0).unwrap();
    let docs = [first, second];
    let expected: Vec<Vec<u8>> = docs.iter().enumerate().map(|(index, doc)| {
        let path = temp.path().join(format!("expected_{index}.jpg"));
        export_jpeg(doc, &path, 96).unwrap();
        fs::read(path).unwrap()
    }).collect();
    let output = temp.path().join("concurrent.jpg");
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = docs.into_iter().enumerate().map(|(index, doc)| {
        let output = output.clone();
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            (index, export_jpeg(&doc, &output, 96))
        })
    }).collect();
    barrier.wait();
    let mut winners = Vec::new();
    for handle in handles {
        let (index, result) = handle.join().unwrap();
        match result {
            Ok(()) => winners.push(index),
            Err(CoreError::Io(error)) => assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists),
            other => panic!("unexpected concurrent export result: {other:?}"),
        }
    }
    assert_eq!(winners.len(), 1);
    assert!(fs::read(&output).unwrap() == expected[winners[0]], "published JPEG differs from the winner");
    assert_eq!(image::open(&output).unwrap().to_rgb8().dimensions(), (32, 24));
}

#[test]
fn project_save_must_reject_original_source_as_destination() {
    let temp = tempdir().unwrap();
    let (source, doc) = make_document(temp.path());
    let before = file_sha256(&source);
    let result = save_project(&doc, &source);
    let after = file_sha256(&source);
    assert!(result.is_err() && before == after,
        "original source protection failed: result={result:?}, before_sha256={before}, after_sha256={after}");
}

#[test]
fn project_save_must_reject_managed_original_copy_as_destination() {
    let temp = tempdir().unwrap();
    let (_, doc) = make_document(temp.path());
    let copy = doc.original_copy_path.clone().unwrap();
    let before = file_sha256(&copy);
    let result = save_project(&doc, &copy);
    let after = file_sha256(&copy);
    assert!(result.is_err() && before == after,
        "managed original protection failed: result={result:?}, before_sha256={before}, after_sha256={after}");
}

#[test]
#[ignore = "24MP core timing only; run separately with --ignored --nocapture --test-threads=1"]
fn measure_24mp_core_open_exposure_and_export_without_performance_threshold() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("synthetic_6000x4000.jpg");
    let fixture = RgbImage::from_fn(6000, 4000, |x, y| {
        Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });
    fixture.save(&source).unwrap();
    drop(fixture);
    let source_hash = file_sha256(&source);

    let started = Instant::now();
    let mut doc = open_image(&source).unwrap();
    let open_ms = started.elapsed().as_millis();
    let started = Instant::now();
    set_exposure(&mut doc, 1.0).unwrap();
    let preview = render_preview(&doc).unwrap();
    let exposure_render_ms = started.elapsed().as_millis();
    assert_eq!(preview.dimensions(), (6000, 4000));
    drop(preview);

    let output = temp.path().join("synthetic_24mp_plus1ev.jpg");
    let started = Instant::now();
    export_jpeg(&doc, &output, DEFAULT_JPEG_QUALITY).unwrap();
    let export_including_render_ms = started.elapsed().as_millis();
    assert_eq!(image::open(&output).unwrap().to_rgb8().dimensions(), (6000, 4000));
    assert_eq!(file_sha256(&source), source_hash);
    println!("CHROMORA_24MP_CORE_TIMING {}", json!({
        "width": 6000,
        "height": 4000,
        "open_ms": open_ms,
        "exposure_plus1ev_render_ms": exposure_render_ms,
        "jpeg_export_including_render_ms": export_including_render_ms,
        "jpeg_quality": DEFAULT_JPEG_QUALITY,
        "output_bytes": fs::metadata(&output).unwrap().len(),
        "performance_threshold": null,
        "scope": "synthetic JPEG, core only; fixture generation and UI excluded"
    }));
}
