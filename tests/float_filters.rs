use chromora::core::{self, ColorPipeline, CoreError, RenderCancellation, SkinRegion};
use std::fs;

fn uniform(root: &std::path::Path, pipeline: ColorPipeline) -> core::PhotoDocument {
    let path = root.join("uniform.png");
    fs::write(&path, include_bytes!("../fixtures/color08/uniform-rgb16.png")).unwrap();
    core::open_image_with_pipeline(&path, pipeline).unwrap()
}

#[test]
fn new_float_clarity_and_portrait_keep_a_deep_uniform_field_within_one_lsb() {
    let temp = tempfile::tempdir().unwrap();
    let mut doc = uniform(temp.path(), ColorPipeline::FloatSrgb);
    let source = doc.working_image().pixels.clone();
    for clarity in [-100., 0., 100.] {
        for portrait in [false,true] {
            let settings = core::AdjustmentSettings { clarity,
                smoothing:if portrait {100.} else {0.}, skin_tone:if portrait {100.} else {0.},
                texture:if portrait {100.} else {0.}, blemishes:if portrait {100.} else {0.},
                skin_region:portrait.then_some(SkinRegion{x:0.,y:0.,width:1.,height:1.}), ..Default::default() };
            core::set_settings(&mut doc,settings).unwrap();
            let rendered = core::render_rgb_f32(&doc,None).unwrap();
            for (actual,expected) in rendered.as_raw().iter().zip(source.as_raw()) {
                assert!((actual-expected).abs()*65535. <= 1., "clarity={clarity}, portrait={portrait}, actual={actual}, expected={expected}");
            }
        }
    }
}

#[test]
fn old_modes_keep_the_frozen_uniform_clarity_result() {
    let temp = tempfile::tempdir().unwrap();
    for pipeline in [ColorPipeline::LegacySrgb,ColorPipeline::HqSrgb,ColorPipeline::AccurateSrgb] {
        let mut doc = uniform(temp.path(),pipeline);
        let mut settings = doc.settings.clone(); settings.clarity=100.;
        core::set_settings(&mut doc,settings).unwrap();
        let image = core::render_rgb_f32(&doc,None).unwrap();
        assert!(image.as_raw().iter().all(|v| v.to_bits()==0.014895857_f32.to_bits()));
    }
}

#[test]
fn neutral_new_pipeline_is_exact_and_cancelled_work_never_returns_an_image() {
    let temp = tempfile::tempdir().unwrap();
    let mut doc = uniform(temp.path(),ColorPipeline::FloatSrgb);
    let before = doc.working_image().pixels.clone();
    assert_eq!(core::render_rgb_f32(&doc,None).unwrap(), *before);
    let token=RenderCancellation::new(); token.cancel();
    assert!(matches!(core::render_rgb_f32_cancellable(&doc,None,&token),Err(CoreError::Cancelled)));
    assert!(matches!(core::render_preview_at_cancellable(&doc,Some(1440),&token),Err(CoreError::Cancelled)));
    assert!(matches!(core::render_original_cancellable(&doc,None,&token),Err(CoreError::Cancelled)));
    assert!(matches!(core::prepare_preview_cancellable(&mut doc,&token),Err(CoreError::Cancelled)));
    assert_eq!(*doc.working_image().pixels,*before);
}

#[test]
fn workspace_progress_cancellation_aborts_loading_without_changing_the_file() {
    let temp=tempfile::tempdir().unwrap();
    let doc=uniform(temp.path(),ColorPipeline::FloatSrgb);
    let workspace=core::WorkspaceDocument{name:"cancel".into(),photos:vec![doc.clone(),doc],current_index:0};
    let path=temp.path().join("project.chromora");
    core::save_workspace(&workspace,&path).unwrap();
    let before=fs::read(&path).unwrap();
    let token=RenderCancellation::new();let signal=token.clone();let mut records=vec![];
    let loaded=core::load_workspace_with_progress(&path,&token,|completed,total,current|{
        records.push((completed,total,current.to_path_buf()));
        if completed==1 {signal.cancel();}
    });
    assert!(matches!(loaded,Err(CoreError::Cancelled)));
    assert!(records.iter().any(|(done,total,_)|*done==1&&*total==2));
    assert!(!records.iter().any(|(done,_,_)|*done==2));
    assert_eq!(fs::read(&path).unwrap(),before);
    let normal=core::load_workspace_with_progress(&path,&RenderCancellation::new(),|_,_,_|{}).unwrap();
    assert_eq!(normal.workspace.photos.len(),2);
}
