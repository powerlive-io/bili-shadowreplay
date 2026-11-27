use std::{
    collections::{BTreeSet, HashSet},
    io,
    path::PathBuf,
    sync::Arc,
};

use crate::{database::Database, handlers::utils::decode_data_url};
use recorder::{entry::EntryStore, platforms::PlatformType};
use recording_catalog::{CatalogRecord, CatalogWriter};
use recording_storage::{RecordingStorage, RecordingStore, LOG_METADATA_DIR};

/// Run only during an explicit writable sync, while the selected backend is
/// detached. Write through the catalog writer, not the live recording API:
/// importing media must not start a recording or reroute it to the default.
pub async fn migrate_legacy_archives(
    db: &Database,
    alias: &str,
    store: &RecordingStore,
    writer: &CatalogWriter,
    present_before_import: &HashSet<String>,
) -> io::Result<()> {
    // SQLite import (if needed) and remote scanning have already completed.
    // Preserve their UUIDs, summaries and receipts when converting old fields.
    let mut known = writer.known_live_ids().await?;
    let adopted = db
        .adopted_recording_ids(writer.identity().storage_uuid)
        .await
        .map_err(io::Error::other)?;
    let mut scan = writer.scan().await?;
    loop {
        let page = scan.next_page(128).await?;
        if page.is_empty() {
            break;
        }
        for entry in page {
            let mut record = entry.record;
            if present_before_import.contains(&record.live_id)
                || adopted.contains(&record.recording_id)
            {
                continue; // Neither adoption nor recovery should rewrite existing JSON.
            }
            let media = store.recording(&record.platform, &record.room_id, &record.live_id)?;
            let mut changed = false;
            if record.parent_id.is_empty() {
                record.parent_id.clone_from(&record.live_id);
                changed = true;
            }
            if let Some(data) = record.cover.as_deref().filter(|v| v.starts_with("data:")) {
                match decode_data_url(data) {
                    Ok(bytes) => {
                        media.write("cover.jpg", &bytes).await?;
                        record.cover = Some(format!(
                            "{}/{}/{}/cover.jpg",
                            record.platform, record.room_id, record.live_id
                        ));
                        changed = true;
                    }
                    Err(error) => {
                        log::warn!("Skipping invalid cover for {}: {error}", record.live_id)
                    }
                }
            }
            if (record.size == 0 || !media.exists("playlist.m3u8").await?)
                && media.log_exists("entries.log").await?
            {
                let entries = EntryStore::from_storage(&media).await?;
                if !entries.is_empty() {
                    ensure_playlist(&media, &entries).await?;
                    if record.size == 0
                        && (record.length != entries.total_duration() || entries.total_size() != 0)
                    {
                        record.length = entries.total_duration();
                        record.size = entries.total_size();
                        changed = true;
                    }
                }
            }
            if changed {
                writer.put(record, Some(entry.revision)).await?;
            }
        }
    }

    // A returned storage may contain rooms never configured on this machine.
    // Only actual entry logs qualify. Don't delete empty/damaged directories
    // or recreate tombstones from media left behind by a delete or transfer.
    for platform in PlatformType::ALL {
        let scope = store.scope(platform.as_str())?;
        let mut candidates = BTreeSet::new();
        let mut cursor = None;
        loop {
            let page = scope.list("", cursor.as_deref(), 1000).await?;
            for key in page.keys {
                let mut parts = key.splitn(3, '/');
                if let (Some(room), Some(live), Some(file)) =
                    (parts.next(), parts.next(), parts.next())
                {
                    if !room.starts_with('.')
                        && !live.starts_with('.')
                        && !known.contains(live)
                        && (file == "entries.log"
                            || file.split('/').next() == Some(LOG_METADATA_DIR))
                    {
                        candidates.insert((room.to_owned(), live.to_owned()));
                    }
                }
            }
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        for (room, live) in candidates {
            if known.contains(&live)
                || db
                    .legacy_live_id_reserved(alias, &live)
                    .await
                    .map_err(io::Error::other)?
            {
                continue;
            }
            let media = store.recording(platform.as_str(), &room, &live)?;
            let entries = EntryStore::from_storage(&media).await?;
            if entries.is_empty() {
                continue;
            }
            ensure_playlist(&media, &entries).await?;
            writer
                .put(
                    CatalogRecord {
                        recording_id: uuid::Uuid::new_v4(),
                        platform: platform.as_str().into(),
                        room_id: room,
                        live_id: live.clone(),
                        parent_id: live.clone(),
                        title: format!("UnknownLive {live}"),
                        created_at_ms: chrono::Utc::now().timestamp_millis(),
                        length: entries.total_duration(),
                        size: entries.total_size(),
                        cover: None,
                        results: serde_json::Value::Null,
                    },
                    None,
                )
                .await?;
            known.insert(live);
        }
    }
    Ok(())
}

async fn ensure_playlist(media: &RecordingStorage, entries: &EntryStore) -> io::Result<()> {
    if !media.exists("playlist.m3u8").await? {
        media
            .write(
                "playlist.m3u8",
                entries.manifest(true, true, None).as_bytes(),
            )
            .await?;
    }
    Ok(())
}

// Clip outputs are local project files, not recordings on a storage backend.
pub async fn try_convert_clip_covers(
    db: &Arc<Database>,
    output_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let videos = db.get_all_videos().await?;
    log::debug!("videos: {}", videos.len());
    for video in &videos {
        let cover = video.cover.clone();
        if cover.starts_with("data:") {
            let bytes = match decode_data_url(&cover) {
                Ok(bytes) => bytes,
                Err(e) => {
                    log::warn!("Skipping invalid cover for {}: {e}", video.file);
                    continue;
                }
            };

            let video_file_path = output_path.join(video.file.clone());
            let cover_file_path = video_file_path.with_extension("jpg");
            log::debug!("cover_file_path: {}", cover_file_path.display());
            tokio::fs::write(&cover_file_path, bytes).await?;

            log::info!("convert clip cover: {}", cover_file_path.display());
            let cover_name = cover_file_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| format!("Invalid cover path: {}", cover_file_path.display()))?;
            db.update_video_cover(video.id, cover_name).await?;
        }
    }
    Ok(())
}
