use std::{collections::HashMap, path::PathBuf, sync::Arc};
use chromora::core::{self, AdjustmentSettings, CoreError, CoreResult, Lut3d, PhotoDocument, ProjectEditState, ProjectHistory};

#[derive(Clone)]
pub struct EditState {
    pub settings: AdjustmentSettings,
    pub lut: Option<Arc<Lut3d>>,
    pub lut_path: Option<PathBuf>,
}
impl PartialEq for EditState {
    fn eq(&self, other: &Self) -> bool {
        self.settings == other.settings && self.lut_path == other.lut_path
            && self.lut.as_ref().map(|v| &v.sha256) == other.lut.as_ref().map(|v| &v.sha256)
    }
}
impl EditState {
    pub fn capture(doc: &PhotoDocument) -> Self {
        Self { settings: doc.settings.clone(), lut: doc.lut.clone(), lut_path: doc.lut_path.clone() }
    }
    pub fn restore(self, doc: &mut PhotoDocument) {
        doc.exposure_ev = self.settings.exposure;
        doc.settings = self.settings;
        doc.lut = self.lut;
        doc.lut_path = self.lut_path;
    }
}

#[derive(Default)]
pub struct EditHistory {
    undo: Vec<EditState>,
    redo: Vec<EditState>,
    pending: Option<EditState>,
}
impl EditHistory {
    /// Pending edits are committed by the controller before saving a project.
    pub fn to_project(&self) -> ProjectHistory {
        let saved = |state: &EditState| ProjectEditState { settings: state.settings.clone(),
            lut_path: state.lut_path.clone(), lut_sha256: state.lut.as_ref().map(|lut| lut.sha256.clone()) };
        ProjectHistory { undo: self.undo.iter().map(saved).collect(), redo: self.redo.iter().map(saved).collect() }
    }
    pub fn from_project(saved: ProjectHistory) -> CoreResult<Self> {
        if saved.undo.len() + saved.redo.len() > 100 {
            return Err(CoreError::InvalidProject("撤销及重做历史总计超过100步".into()));
        }
        let mut cache = HashMap::<(PathBuf, String), Arc<Lut3d>>::new();
        let mut restore = |saved: ProjectEditState| -> CoreResult<EditState> {
            saved.settings.validate()?;
            let ev = saved.settings.exposure;
            if ((ev / core::EXPOSURE_STEP_EV).round() * core::EXPOSURE_STEP_EV - ev).abs() > 0.0001 {
                return Err(CoreError::InvalidExposure(ev));
            }
            let lut = match (&saved.lut_path, &saved.lut_sha256) {
                (Some(path), Some(hash)) => {
                    let key = (path.clone(), hash.clone());
                    if let Some(lut) = cache.get(&key) { Some(lut.clone()) }
                    else {
                        let lut = core::load_cube(path)?;
                        if lut.sha256 != *hash { return Err(CoreError::MissingOrChangedSource(path.clone())); }
                        let lut = Arc::new(lut); cache.insert(key, lut.clone()); Some(lut)
                    }
                }
                (None, None) => None,
                _ => return Err(CoreError::InvalidProject("历史风格缺少路径或校验信息".into())),
            };
            Ok(EditState { settings: saved.settings, lut, lut_path: saved.lut_path })
        };
        let undo = saved.undo.into_iter().map(&mut restore).collect::<CoreResult<Vec<_>>>()?;
        let redo = saved.redo.into_iter().map(&mut restore).collect::<CoreResult<Vec<_>>>()?;
        Ok(Self { undo, redo, pending: None })
    }
    pub fn can_undo(&self) -> bool { !self.undo.is_empty() || self.pending.is_some() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() && self.pending.is_none() }
    pub fn begin(&mut self, state: EditState) { if self.pending.is_none() { self.pending = Some(state); } }
    fn push(&mut self, state: EditState) {
        if self.undo.len() == 100 { self.undo.remove(0); }
        self.undo.push(state); self.redo.clear();
    }
    pub fn changed(&mut self, before: EditState, current: &EditState) {
        if self.pending.is_none() && before != *current { self.push(before); }
    }
    pub fn commit(&mut self, current: &EditState) {
        if let Some(before) = self.pending.take() { if before != *current { self.push(before); } }
    }
    pub fn cancel(&mut self) -> Option<EditState> { self.pending.take() }
    pub fn undo(&mut self, current: EditState) -> Option<EditState> {
        self.commit(&current);
        let state = self.undo.pop()?; self.redo.push(current); Some(state)
    }
    pub fn redo(&mut self, current: EditState) -> Option<EditState> {
        if self.pending.is_some() { return None; }
        let state = self.redo.pop()?;
        if self.undo.len() == 100 { self.undo.remove(0); }
        self.undo.push(current); Some(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(exposure: f32) -> EditState {
        EditState { settings: AdjustmentSettings { exposure, ..Default::default() }, lut: None, lut_path: None }
    }
    #[test]
    fn drag_is_one_step_and_new_edit_discards_redo() {
        let mut history = EditHistory::default(); history.begin(state(0.0));
        history.changed(state(0.0), &state(0.5)); history.changed(state(0.5), &state(1.0));
        history.commit(&state(1.0));
        assert_eq!(history.undo(state(1.0)).unwrap().settings.exposure, 0.0);
        assert!(!history.can_undo()); assert!(history.can_redo());
        assert_eq!(history.redo(state(0.0)).unwrap().settings.exposure, 1.0);
        history.undo(state(1.0)); history.changed(state(0.0), &state(0.25));
        assert!(!history.can_redo());
    }
    #[test]
    fn cancellation_and_history_limit_do_not_store_pixels() {
        let mut history = EditHistory::default(); history.begin(state(0.0));
        assert_eq!(history.cancel().unwrap().settings.exposure, 0.0);
        assert!(!history.can_undo());
        for i in 0..150 { history.changed(state(i as f32), &state(i as f32 + 1.0)); }
        assert_eq!(history.undo.len(), 100);
    }
    #[test]
    fn persisted_history_restores_undo_redo_and_reuses_same_lut() {
        let temp = tempfile::tempdir().unwrap(); let path = temp.path().join("look.cube");
        std::fs::write(&path, "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n").unwrap();
        let lut = Arc::new(core::load_cube(&path).unwrap());
        let mut history = EditHistory::default();
        for ev in [0.0, 0.5] { history.undo.push(EditState { settings: state(ev).settings, lut: Some(lut.clone()), lut_path: Some(path.clone()) }); }
        history.redo.push(state(1.5));
        let saved = history.to_project();
        let mut reopened = EditHistory::from_project(saved.clone()).unwrap();
        assert!(Arc::ptr_eq(reopened.undo[0].lut.as_ref().unwrap(), reopened.undo[1].lut.as_ref().unwrap()));
        assert_eq!(reopened.undo(state(1.0)).unwrap().settings.exposure, 0.5);
        assert_eq!(reopened.redo(state(0.5)).unwrap().settings.exposure, 1.0);
        std::fs::write(&path, b"changed").unwrap();
        assert!(EditHistory::from_project(saved).is_err());
    }
}
