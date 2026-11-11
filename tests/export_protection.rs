use std::{fs, path::{Path, PathBuf}};
use chromora::core::{self, CoreError, CoreResult, PhotoDocument};
use image::{Rgb, RgbImage};

type Export = fn(&PhotoDocument, &Path) -> CoreResult<()>;
fn jpeg(doc: &PhotoDocument, path: &Path) -> CoreResult<()> { core::export_jpeg(doc, path, 96) }
const EXPORTS: [(&str, Export); 5] = [
    ("jpeg", jpeg), ("png", core::export_png), ("tiff", core::export_tiff),
    ("psd", core::export_psd), ("patchy", core::open_in_patchy),
];

fn document(root: &Path) -> (PhotoDocument, Vec<PathBuf>) {
    let source = root.join("原片 source.png");
    RgbImage::from_pixel(3,2,Rgb([70,100,130])).save(&source).unwrap();
    let mut doc = core::import_image(&source, &root.join("原片副本")).unwrap();
    let cache = root.join("显影 cache.png");
    fs::copy(&source,&cache).unwrap();
    doc.render_source_path = Some(cache.clone());
    doc.render_source_sha256 = Some(doc.source_sha256.clone());
    let lut = root.join("current.cube");
    fs::write(&lut,"LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n").unwrap();
    core::set_lut(&mut doc,Some(&lut)).unwrap();
    let targets = vec![source,doc.original_copy_path.clone().unwrap(),cache,lut];
    (doc,targets)
}

#[test]
fn every_export_rejects_missing_protected_paths_before_rendering_or_tool_lookup() {
    for evicted in [false,true] {
        let temp = tempfile::tempdir().unwrap();
        let (mut doc,targets) = document(temp.path());
        for target in &targets { fs::remove_file(target).unwrap(); }
        if evicted { doc.evict_pixels(); }
        for (format,export) in EXPORTS {
            for target in &targets {
                let result = export(&doc,target);
                assert!(matches!(result,Err(CoreError::ProtectedOriginalDestination(_))),
                    "{format} must protect {target:?} before rendering, evicted={evicted}: {result:?}");
                assert!(!target.exists(),"{format} created {target:?}");
            }
        }
    }
}

#[test]
fn every_export_preserves_existing_assets_and_their_file_aliases() {
    let temp = tempfile::tempdir().unwrap();
    let (doc,targets) = document(temp.path());
    for (index,target) in targets.iter().enumerate() {
        let before = fs::read(target).unwrap();
        let sub = target.parent().unwrap().join("子目录");
        fs::create_dir_all(&sub).unwrap();
        let dotted = sub.join("..").join(".").join(target.file_name().unwrap());
        let hard_link = temp.path().join(format!("alias-{index}.data"));
        fs::hard_link(target,&hard_link).unwrap();
        let mut aliases = vec![target.clone(),dotted,hard_link];
        #[cfg(windows)]
        aliases.push(target.with_file_name(target.file_name().unwrap().to_string_lossy().to_ascii_uppercase()));
        for (format,export) in EXPORTS {
            for alias in &aliases {
                let result = export(&doc,alias);
                assert!(matches!(result,Err(CoreError::ProtectedOriginalDestination(_))),
                    "{format} failed to protect {alias:?}: {result:?}");
                assert_eq!(fs::read(alias).unwrap(),before);
            }
        }
        assert_eq!(fs::read(target).unwrap(),before);
    }
}

#[test]
fn an_existing_independent_destination_is_preserved_for_every_image_format() {
    let temp = tempfile::tempdir().unwrap();
    let (doc,_) = document(temp.path());
    for (format,export) in EXPORTS.into_iter().filter(|(format,_)| *format != "patchy") {
        let target = temp.path().join(format!("existing-{format}.data"));
        fs::write(&target,b"preserve an existing user file").unwrap();
        let result = export(&doc,&target);
        assert!(matches!(result,Err(CoreError::Io(ref error)) if error.kind()==std::io::ErrorKind::AlreadyExists),
            "{format}: {result:?}");
        assert_eq!(fs::read(target).unwrap(),b"preserve an existing user file");
    }
}

#[test]
fn missing_protected_paths_keep_dot_and_windows_case_alias_protection() {
    let temp=tempfile::tempdir().unwrap();
    let (doc,targets)=document(temp.path());
    for target in &targets {fs::remove_file(target).unwrap();}
    for target in &targets {
        let sub=target.parent().unwrap().join("子目录");fs::create_dir_all(&sub).unwrap();
        let mut aliases=vec![sub.join("..").join(target.file_name().unwrap())];
        #[cfg(windows)]
        aliases.push(target.with_file_name(target.file_name().unwrap().to_string_lossy().to_uppercase()));
        for (format,export) in EXPORTS {
            for alias in &aliases {
                assert!(matches!(export(&doc,alias),Err(CoreError::ProtectedOriginalDestination(_))),"{format}: {alias:?}");
                assert!(!alias.exists());
            }
        }
    }
}
