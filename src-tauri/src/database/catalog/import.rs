//! The local snapshot owns legacy inputs; the storage owns recording identities.
//! Matches are durable locally, including B -> A for plans frozen by older versions.
use super::*;
use recording_catalog::ImportInventory;
use sqlx::{Executor, Sqlite, Transaction};
use std::collections::{HashMap, HashSet};

pub const SCHEMA_SQL: &str = "
    CREATE TABLE catalog_legacy_matches (
        import_id TEXT NOT NULL, live_id TEXT NOT NULL, recording_uuid TEXT NOT NULL,
        record TEXT, frozen_ordinal INTEGER CHECK(frozen_ordinal>=0),
        CHECK((record IS NULL) != (frozen_ordinal IS NULL)),
        PRIMARY KEY(import_id,live_id), UNIQUE(import_id,frozen_ordinal),
        FOREIGN KEY(import_id,frozen_ordinal) REFERENCES catalog_legacy_entries(import_id,ordinal)
    );
    CREATE INDEX catalog_legacy_matches_live ON catalog_legacy_matches(live_id);
    CREATE INDEX catalog_legacy_matches_uuid ON catalog_legacy_matches(recording_uuid);
    CREATE INDEX catalog_record_keys_uuid ON catalog_record_keys(recording_uuid);
    CREATE INDEX catalog_record_links_uuid ON catalog_record_links(recording_uuid);
    CREATE INDEX recording_storage_bindings_live ON recording_storage_bindings(live_id);
";

#[derive(Default, Debug, Serialize)]
pub struct LegacyImportReport {
    pub inserted: u64,
    pub adopted: u64,
    pub skipped: u64,
    pub differences: Vec<LegacyDifference>,
}

#[derive(Debug, Serialize)]
pub struct LegacyDifference {
    pub platform: String,
    pub room_id: String,
    pub live_id: String,
    pub fields: Vec<&'static str>,
}

impl LegacyImportReport {
    fn adopted(&mut self, input: &CatalogRecord, storage: &CatalogRecord) {
        self.adopted += 1;
        let fields: Vec<_> = [
            ("标题", input.title != storage.title),
            ("父录播", input.parent_id != storage.parent_id),
            ("创建时间", input.created_at_ms != storage.created_at_ms),
            ("时长", input.length != storage.length),
            ("大小", input.size != storage.size),
            ("封面", input.cover != storage.cover),
            (
                "摘要",
                input.results.get("summary").filter(|v| !v.is_null())
                    != storage.results.get("summary").filter(|v| !v.is_null()),
            ),
        ]
        .into_iter()
        .filter_map(|(label, differs)| differs.then_some(label))
        .collect();
        if !fields.is_empty() {
            self.differences.push(LegacyDifference {
                platform: input.platform.clone(),
                room_id: input.room_id.clone(),
                live_id: input.live_id.clone(),
                fields,
            });
        }
    }
}

pub(super) async fn check_binding(
    tx: &mut Transaction<'_, Sqlite>,
    alias: &str,
    uuid: Uuid,
    record: &CatalogRecord,
    target: Uuid,
) -> Result<(), DatabaseError> {
    let conflict: Option<String> = sqlx::query_scalar(
        "SELECT recording_uuid FROM catalog_record_keys WHERE live_id=?1 AND recording_uuid!=?5
         UNION ALL SELECT live_id FROM catalog_record_keys WHERE recording_uuid=?5 AND live_id!=?1
         UNION ALL SELECT recording_uuid FROM catalog_legacy_matches WHERE live_id=?1 AND recording_uuid!=?5
         UNION ALL SELECT live_id FROM catalog_legacy_matches WHERE recording_uuid=?5 AND live_id!=?1
         UNION ALL SELECT storage_uuid || '/' || recording_uuid FROM catalog_record_links WHERE live_id=?1
             AND (storage_uuid!=?6 OR recording_uuid!=?5)
         UNION ALL SELECT live_id FROM catalog_record_links WHERE recording_uuid=?5 AND live_id!=?1
         UNION ALL SELECT storage_id || '/' || platform || '/' || room_id FROM recording_storage_bindings
             WHERE live_id=?1 AND (storage_id!=?4 OR platform!=?2 OR room_id!=?3)
         UNION ALL SELECT platform || '/' || room_id FROM records WHERE live_id=?1
             AND (platform!=?2 OR room_id!=?3 OR NOT EXISTS(
                 SELECT 1 FROM recording_records WHERE live_id=?1 AND storage_backend_id=?4)) LIMIT 1",
    ).bind(&record.live_id).bind(&record.platform).bind(&record.room_id).bind(alias)
        .bind(target.to_string()).bind(uuid.to_string()).fetch_optional(&mut **tx).await?;
    if let Some(other) = conflict {
        return Err(error(format!(
            "录播 {}/{}/{} 与本机绑定 {other} 冲突，不能指向 {alias}/{target}",
            record.platform, record.room_id, record.live_id
        )));
    }
    Ok(())
}

pub(super) async fn save_match(
    tx: &mut Transaction<'_, Sqlite>,
    plan: &ImportPlan,
    record: &CatalogRecord,
    target: Uuid,
    ordinal: Option<i64>,
) -> Result<(), DatabaseError> {
    // An old plan already has the exact input B at its original ordinal.
    // Store a copy of the input only for matches outside the remote plan.
    let saved = sqlx::query(
        "INSERT INTO catalog_legacy_matches(import_id,live_id,recording_uuid,record,frozen_ordinal)
        VALUES (?,?,?,?,?) ON CONFLICT(import_id,live_id) DO UPDATE SET recording_uuid=excluded.recording_uuid
        WHERE catalog_legacy_matches.recording_uuid=excluded.recording_uuid
          AND catalog_legacy_matches.frozen_ordinal IS excluded.frozen_ordinal",
    )
    .bind(plan.import_id.to_string())
    .bind(&record.live_id)
    .bind(target.to_string())
    .bind(if ordinal.is_none() { Some(json(record)?) } else { None })
    .bind(ordinal)
    .execute(&mut **tx)
    .await?.rows_affected();
    if saved != 1 {
        return Err(error(format!(
            "录播 {}/{}/{} 的认领目标或冻结序号已改变，未修改存储",
            record.platform, record.room_id, record.live_id
        )));
    }
    Ok(())
}

async fn entry_page<'e>(
    executor: impl Executor<'e, Database = Sqlite>,
    plan: &ImportPlan,
    index: u64,
) -> Result<Vec<(i64, String)>, DatabaseError> {
    Ok(sqlx::query_as(
        "SELECT ordinal,record FROM catalog_legacy_entries WHERE import_id=? AND ordinal>=?
        ORDER BY ordinal LIMIT 128",
    )
    .bind(plan.import_id.to_string())
    .bind(i64::try_from(index).map_err(error)?)
    .fetch_all(executor)
    .await?)
}

impl Database {
    pub async fn import_legacy_catalog(
        &self,
        alias: &str,
        writer: &CatalogWriter,
    ) -> Result<LegacyImportReport, DatabaseError> {
        let inventory = writer.import_inventory().await.map_err(error)?;
        self.prepare_legacy_catalog_import(alias, &inventory)
            .await?;
        self.resume_legacy_catalog_import(alias, writer, &inventory)
            .await
    }

    pub async fn adopted_recording_ids(&self, uuid: Uuid) -> Result<HashSet<Uuid>, DatabaseError> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT m.recording_uuid FROM catalog_legacy_matches m
            JOIN catalog_legacy_imports i USING(import_id) WHERE i.storage_uuid=?",
        )
        .bind(uuid.to_string())
        .fetch_all(&self.pool)
        .await?;
        ids.into_iter()
            .map(|id| id.parse().map_err(error))
            .collect()
    }

    /// Read every original input and binding before the first remote journal write.
    /// In particular do not re-freeze an old B or renumber its acknowledged prefix.
    async fn preflight_import(
        &self,
        alias: &str,
        plan: &ImportPlan,
        inventory: &ImportInventory,
    ) -> Result<(LegacyImportReport, HashMap<u64, Uuid>), DatabaseError> {
        if plan.storage_uuid != inventory.storage_uuid {
            return Err(error("Legacy snapshot belongs to another storage UUID"));
        }
        let mut next = 0;
        if let Some(p) = &inventory.progress {
            if p.plan == *plan {
                next = p.next_index;
                if next == 0 && !p.prefix_matches(&ImportDigest::default()) {
                    return Err(error("存储的初始导入摘要不匹配，未修改存储"));
                }
            } else if !p.complete || p.plan.import_id == plan.import_id {
                return Err(error("存储的导入计划与本机冻结快照不匹配，未修改存储"));
            }
        }
        let mut tx = self.pool.begin().await?;
        for record in inventory.active_records() {
            inventory
                .resolve(
                    &record.platform,
                    &record.room_id,
                    &record.live_id,
                    Some(record.recording_id),
                )
                .map_err(error)?;
            check_binding(
                &mut tx,
                alias,
                plan.storage_uuid,
                record,
                record.recording_id,
            )
            .await?;
        }
        Self::verify_frozen_entries(&mut tx, alias, plan, inventory, next).await?;
        let resolved = Self::verify_matches(&mut tx, alias, plan, inventory).await?;
        tx.commit().await?;
        Ok(resolved)
    }

    /// For an older plan even a newly adopted item is still part of its original digest.
    async fn verify_frozen_entries(
        tx: &mut Transaction<'_, Sqlite>,
        alias: &str,
        plan: &ImportPlan,
        inventory: &ImportInventory,
        next: u64,
    ) -> Result<(), DatabaseError> {
        let mut digest = ImportDigest::default();
        let mut index = 0;
        loop {
            let rows = entry_page(&mut **tx, plan, index).await?;
            if rows.is_empty() {
                break;
            }
            for (ordinal, bytes) in rows {
                if u64::try_from(ordinal).ok() != Some(index) {
                    return Err(error("Legacy snapshot has a gap"));
                }
                let record: CatalogRecord = from_json(&bytes)?;
                digest.append(&record).map_err(error)?;
                let target = inventory
                    .resolve(
                        &record.platform,
                        &record.room_id,
                        &record.live_id,
                        Some(record.recording_id),
                    )
                    .map_err(error)?;
                let uuid = target.map_or(record.recording_id, |t| t.record.recording_id);
                check_binding(tx, alias, plan.storage_uuid, &record, uuid).await?;
                if let Some(t) = target {
                    if !t.has_receipt(plan, index, &record).map_err(error)? {
                        save_match(tx, plan, &record, uuid, Some(ordinal)).await?;
                    }
                } else if index >= next
                    && sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM recording_storage_deletions
                    WHERE platform=? AND room_id=? AND live_id=?)",
                    )
                    .bind(&record.platform)
                    .bind(&record.room_id)
                    .bind(&record.live_id)
                    .fetch_one(&mut **tx)
                    .await?
                {
                    return Err(error(format!(
                        "旧录播 {}/{}/{} 在冻结快照后已标记删除，不能补入存储",
                        record.platform, record.room_id, record.live_id
                    )));
                }
                index += 1;
                if index == next
                    && inventory
                        .progress
                        .as_ref()
                        .is_some_and(|p| !p.prefix_matches(&digest))
                {
                    return Err(error(
                        "存储上已导入部分与本机冻结快照的摘要不匹配，未修改存储",
                    ));
                }
            }
        }
        if index != plan.count || digest != plan.digest {
            return Err(error("旧库冻结快照的数量或摘要已改变，未修改存储"));
        }
        Ok(())
    }

    /// Recheck durable matches on retry rather than redirecting A to today's occupant.
    async fn verify_matches(
        tx: &mut Transaction<'_, Sqlite>,
        alias: &str,
        plan: &ImportPlan,
        inventory: &ImportInventory,
    ) -> Result<(LegacyImportReport, HashMap<u64, Uuid>), DatabaseError> {
        let mut report = LegacyImportReport::default();
        let mut acknowledgements = HashMap::new();
        let mut after = String::new();
        loop {
            let rows: Vec<(String, String, String, Option<i64>)> = sqlx::query_as(
                "SELECT m.live_id,COALESCE(e.record,m.record),m.recording_uuid,m.frozen_ordinal
                 FROM catalog_legacy_matches m LEFT JOIN catalog_legacy_entries e
                 ON e.import_id=m.import_id AND e.ordinal=m.frozen_ordinal
                 WHERE m.import_id=? AND m.live_id>? ORDER BY m.live_id LIMIT 128",
            )
            .bind(plan.import_id.to_string())
            .bind(&after)
            .fetch_all(&mut **tx)
            .await?;
            if rows.is_empty() {
                break;
            }
            for (live, bytes, id, ordinal) in rows {
                after = live;
                let record: CatalogRecord = from_json(&bytes)?;
                let target = inventory
                    .resolve(
                        &record.platform,
                        &record.room_id,
                        &after,
                        Some(record.recording_id),
                    )
                    .map_err(error)?
                    .filter(|t| t.record.recording_id.to_string() == id && record.live_id == after)
                    .ok_or_else(|| {
                        error(format!(
                            "录播 {}/{}/{after} 的认领目标 {id} 已改变，未修改存储",
                            record.platform, record.room_id
                        ))
                    })?;
                check_binding(
                    tx,
                    alias,
                    plan.storage_uuid,
                    &record,
                    target.record.recording_id,
                )
                .await?;
                if target.active {
                    report.adopted(&record, &target.record);
                } else {
                    report.skipped += 1;
                }
                if let Some(i) = ordinal {
                    acknowledgements
                        .insert(u64::try_from(i).map_err(error)?, target.record.recording_id);
                }
            }
        }
        report.inserted = plan
            .count
            .checked_sub(acknowledgements.len() as u64)
            .ok_or_else(|| error("Invalid frozen import resolutions"))?;
        Ok((report, acknowledgements))
    }

    pub(super) async fn resume_legacy_catalog_import(
        &self,
        alias: &str,
        writer: &CatalogWriter,
        inventory: &ImportInventory,
    ) -> Result<LegacyImportReport, DatabaseError> {
        let (saved, complete): (String, bool) =
            sqlx::query_as("SELECT plan,complete FROM catalog_legacy_imports WHERE source_id=?")
                .bind(alias)
                .fetch_one(&self.pool)
                .await?;
        let plan: ImportPlan = from_json(&saved)?;
        let (report, matched) = self.preflight_import(alias, &plan, inventory).await?;
        // A plan with no missing rows is entirely local unless a matching journal
        // already exists (including older plans that froze B before discovering A).
        if (!complete && report.inserted != 0)
            || inventory
                .progress
                .as_ref()
                .is_some_and(|p| p.plan == plan && !p.complete)
        {
            let progress = writer
                .begin_checked_import(plan.clone())
                .await
                .map_err(error)?;
            if !progress.complete {
                let mut index = progress.next_index;
                while index < plan.count {
                    let rows = entry_page(&self.pool, &plan, index).await?;
                    if rows.is_empty() {
                        return Err(error("Legacy snapshot is incomplete"));
                    }
                    for (ordinal, bytes) in rows {
                        if u64::try_from(ordinal).ok() != Some(index) {
                            return Err(error("Legacy snapshot has a gap"));
                        }
                        let record = from_json(&bytes)?;
                        match matched.get(&index) {
                            Some(&id) => {
                                writer
                                    .acknowledge_existing(plan.clone(), index, record, id)
                                    .await
                            }
                            None => writer.import_record(plan.clone(), index, record).await,
                        }
                        .map_err(error)?;
                        index += 1;
                    }
                }
                writer.finish_import(plan.clone()).await.map_err(error)?;
            }
        }
        let acknowledged = sqlx::query(
            "UPDATE catalog_legacy_imports SET complete=1 WHERE import_id=? AND plan=?",
        )
        .bind(plan.import_id.to_string())
        .bind(saved)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if acknowledged != 1 {
            return Err(error("确认导入时本机冻结计划已改变，请核对同步状态"));
        }
        Ok(report)
    }
}
