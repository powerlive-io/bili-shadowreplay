//! The editor previews an unsaved source and publishes all its edits together.
use super::{
    ensure_editable, new_upload, output_dir, source, upload_dir, upload_view, write_cover,
    UploadView,
};
use crate::{
    events::DataChange,
    ffmpeg::runner::TempArtifact,
    project::{RenderSettings, Timeline},
    state::State,
    state_type,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
#[cfg(feature = "gui")]
use tauri::State as TauriState;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftSource {
    pub source: String,
    #[serde(default)]
    pub source_id: i64,
    #[serde(default)]
    pub file: String,
}

impl DraftSource {
    pub(crate) async fn row(
        self,
        state: &State,
        profile: Value,
    ) -> Result<crate::database::upload::UploadRow, String> {
        new_upload(state, self.source, self.source_id, self.file, profile).await
    }
}

async fn default_file_cover(file: &Path) -> Option<String> {
    let duration = ffmpeg_utils::extract_video_metadata(file)
        .await
        .ok()?
        .duration;
    let time = crate::ffmpeg::get_random_thumbnail_timestamp(duration);
    let bytes = ffmpeg_utils::extract_jpeg_scaled(file, time, Some(2), Some(1280))
        .await
        .ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn preview_upload(
    state: state_type!(),
    source: DraftSource,
    profile: Value,
) -> Result<UploadView, String> {
    let row = source.row(&state, profile).await?;
    let mut view = upload_view(&state, row).await;
    if view.upload.source == "file" {
        view.display_cover = default_file_cover(Path::new(&view.upload.file))
            .await
            .unwrap_or_default();
    }
    Ok(view)
}

#[derive(Deserialize)]
pub struct Change<T> {
    base: T,
    value: T,
}

impl<T: PartialEq> Change<T> {
    fn apply(self, current: &mut T) -> Result<(), String> {
        if self.base != *current && self.value != *current {
            return Err("压制设置已在其他窗口修改，请重新打开草稿。".into());
        }
        *current = self.value;
        Ok(())
    }
}

#[derive(Deserialize)]
pub struct Covers {
    cover: String,
    cover43: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftEdit {
    id: i64,
    source: Option<DraftSource>,
    expected_at: Option<String>,
    uid: String,
    profile: Value,
    covers: Option<Covers>,
    danmu: Option<Change<bool>>,
    render_settings: Option<Change<RenderSettings>>,
}

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn save_upload_draft(
    state: state_type!(),
    edit: DraftEdit,
) -> Result<UploadView, String> {
    #[cfg(feature = "gui")]
    let state = (*state).clone();
    // Keep the source/cover guards alive through the commit acknowledgement
    // even when the caller disconnects.
    tokio::spawn(async move { save(&state, edit).await })
        .await
        .map_err(|e| e.to_string())?
}

async fn save(state: &State, edit: DraftEdit) -> Result<UploadView, String> {
    let _operation = source::source_operation().await;
    let output = output_dir(state).await;
    let mut row = if edit.id == 0 {
        edit.source
            .ok_or("缺少投稿来源")?
            .row(state, edit.profile.clone())
            .await?
    } else {
        let row = state.db.get_upload(edit.id).await?;
        ensure_editable(&row)?;
        row
    };
    row.uid = edit.uid;
    row.profile = edit.profile.to_string();
    row.updated_at = chrono::Local::now().to_rfc3339();
    let mut project = None;
    if edit.danmu.is_some() || edit.render_settings.is_some() {
        if let Some(change) = &edit.render_settings {
            change.value.validate()?;
        }
        match row.source.as_str() {
            "project" => {
                let original = state.db.get_project(row.source_id).await?.timeline;
                let mut timeline = Timeline::parse(&original)?;
                if let Some(change) = edit.danmu {
                    change.apply(&mut timeline.danmu)?;
                }
                if let Some(change) = edit.render_settings {
                    change.apply(&mut timeline.render_settings)?;
                }
                project = Some((original, timeline.to_json()));
            }
            "material" => {
                if let Some(change) = edit.danmu {
                    change.apply(&mut row.danmu)?;
                }
                if let Some(change) = edit.render_settings {
                    let mut settings =
                        serde_json::from_str(&row.render_settings_json).unwrap_or_default();
                    change.apply(&mut settings)?;
                    row.render_settings_json =
                        serde_json::to_string(&settings).map_err(|e| e.to_string())?;
                }
            }
            _ => return Err("视频文件直接投稿，无需压制设置".into()),
        }
    }
    let staged = if row.id == 0 {
        source::stage_source(&mut row, state.temp_storage.clone(), &output).await?
    } else {
        None
    };
    let old = [row.cover.clone(), row.cover43.clone()];
    let (mut tx, id) = state
        .db
        .begin_upload_edit(
            &row,
            edit.expected_at.as_deref(),
            project.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
        )
        .await?;
    row.id = id;
    let mut artifacts = Vec::new();
    if let Some(covers) = edit.covers {
        let name = uuid::Uuid::new_v4();
        row.cover = format!("{}/cover_{name}.jpg", upload_dir(id));
        row.cover43.clear();
        if covers.cover43.is_some() {
            row.cover43 = format!("{}/cover43_{name}.jpg", upload_dir(id));
        }
        for (relative, data) in [
            (&row.cover, Some(covers.cover)),
            (&row.cover43, covers.cover43),
        ] {
            if let Some(data) = data {
                let path = output.join(relative);
                artifacts.push(TempArtifact::new(path.clone()));
                write_cover(&path, &data).await?;
            }
        }
        sqlx::query("UPDATE uploads SET cover=?, cover43=? WHERE id=?")
            .bind(&row.cover)
            .bind(&row.cover43)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    if let Some(dir) = staged {
        let _ = dir.keep();
    }
    for artifact in artifacts {
        artifact.keep();
    }
    state.db.notify_change(DataChange::Uploads);
    if project.is_some() {
        state.db.notify_change(DataChange::Projects);
    }
    for file in old {
        if file.starts_with(&format!("{}/", upload_dir(id)))
            && file != row.cover
            && file != row.cover43
        {
            let _ = tokio::fs::remove_file(output.join(file)).await;
        }
    }
    Ok(upload_view(state, row).await)
}
