//! Display-only rows observed in this startup scan. Never feed these into the
//! authoritative projection, migration, recorder bindings or file operations.
use super::{CatalogRecord, RecordRow};
use std::{cmp::Reverse, collections::BTreeMap, sync::RwLock};

// Newest recording first, with live ID and storage alias as stable tie-breakers.
type PreviewKey = (Reverse<i64>, String, String);

#[derive(Default)]
pub struct StartupPreview(RwLock<BTreeMap<PreviewKey, RecordRow>>);

impl StartupPreview {
    pub fn observe(&self, alias: &str, record: &CatalogRecord) {
        let (Some(created), Ok(size)) = (
            chrono::DateTime::from_timestamp_millis(record.created_at_ms),
            i64::try_from(record.size),
        ) else {
            return;
        };
        self.0.write().unwrap().insert(
            (
                Reverse(record.created_at_ms),
                record.live_id.clone(),
                alias.into(),
            ),
            RecordRow {
                platform: record.platform.clone(),
                room_id: record.room_id.clone(),
                live_id: record.live_id.clone(),
                parent_id: record.parent_id.clone(),
                title: record.title.clone(),
                length: record.length,
                size,
                created_at: created.to_rfc3339(),
                cover: None,
                storage_backend_id: alias.into(),
                storage_available: false,
                storage_unavailable_reason: Some("正在加载存储目录，仅可查看录播信息".into()),
            },
        );
    }

    pub fn discard(&self, alias: &str) {
        self.0.write().unwrap().retain(|(_, _, id), _| id != alias);
    }

    pub fn clear(&self) {
        self.0.write().unwrap().clear();
    }

    pub fn page(
        &self,
        room: &str,
        platform: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Vec<RecordRow> {
        let rows = self.0.read().unwrap();
        let mut seen = std::collections::HashSet::new();
        rows.values()
            .filter(|r| {
                (room.is_empty() || r.room_id == room) && platform.is_none_or(|p| r.platform == p)
            })
            .filter(|r| seen.insert(&r.live_id))
            .skip(offset.max(0) as usize)
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
            .cloned()
            .collect()
    }

    pub fn totals(&self) -> (i64, f64, i64) {
        let today = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        let rows = self.0.read().unwrap();
        let mut seen = std::collections::HashSet::new();
        let mut totals = (0i64, 0.0, 0);
        for ((Reverse(time), live, _), row) in rows.iter() {
            if seen.insert(live) {
                totals.0 = totals.0.saturating_add(row.size);
                totals.1 += row.length;
                totals.2 += i64::from(*time >= today);
            }
        }
        totals
    }
}
