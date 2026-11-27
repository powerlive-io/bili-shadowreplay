use super::*;

/// A mount incarnation, not just an alias. Delayed requests from an old mount
/// cannot publish data into a newly mounted storage using the same name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogMount {
    pub alias: String,
    pub storage_uuid: Uuid,
    pub epoch: Uuid,
}

impl Database {
    /// A storage write and the compatibility projection cannot be committed
    /// atomically. On reconnect, repair a crash between them from the local
    /// document cache instead of rewriting every recording on every boot.
    pub async fn catalog_projection_matches(
        &self,
        mount: &CatalogMount,
        scan: &CatalogScan,
    ) -> Result<bool, DatabaseError> {
        let published: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM catalog_cache_heads WHERE storage_uuid=? AND projected_epoch=?)")
            .bind(mount.storage_uuid.to_string()).bind(mount.epoch.to_string()).fetch_one(&self.pool).await?;
        if !published {
            return Ok(false);
        }
        let rows: Vec<(Option<String>, Option<i64>)> = sqlx::query_as(
            "SELECT l.recording_uuid,l.revision FROM recording_records r \
             LEFT JOIN catalog_record_links l ON l.live_id=r.live_id AND l.storage_uuid=? \
             WHERE r.storage_backend_id=?",
        )
        .bind(scan.storage_uuid().to_string())
        .bind(&mount.alias)
        .fetch_all(&self.pool)
        .await?;
        if rows.len() != scan.entries().len() {
            return Ok(false);
        }
        let revisions: std::collections::HashMap<_, _> = rows
            .into_iter()
            .filter_map(|(id, revision)| Some((id?, revision?)))
            .collect();
        Ok(scan.entries().iter().all(|entry| {
            revisions
                .get(&entry.record.recording_id.to_string())
                .is_some_and(|&revision| u64::try_from(revision) == Ok(entry.revision))
        }))
    }

    pub async fn mount_catalog(
        &self,
        alias: &str,
        storage_uuid: Uuid,
    ) -> Result<CatalogMount, DatabaseError> {
        if alias.is_empty() || storage_uuid.is_nil() {
            return Err(error("Invalid catalog mount"));
        }
        let mount = CatalogMount {
            alias: alias.into(),
            storage_uuid,
            epoch: Uuid::new_v4(),
        };
        let mut tx = self.pool.begin().await?;
        // UNIQUE(storage_uuid) prevents double counting aliases for one root.
        sqlx::query("INSERT INTO catalog_mounts (alias, storage_uuid, epoch, error) VALUES (?, ?, ?, 'Catalog has not been refreshed')")
            .bind(alias).bind(storage_uuid.to_string()).bind(mount.epoch.to_string()).execute(&mut *tx).await?;
        sqlx::query("INSERT OR IGNORE INTO catalog_projection_heads (storage_uuid) VALUES (?)")
            .bind(storage_uuid.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(mount)
    }

    /// Build under a new generation, then atomically swap the active pointer.
    /// EOF is required. Errors/cancellation never publish a partial/empty scan.
    pub async fn rebuild_catalog_projection(
        &self,
        mount: CatalogMount,
        scan: CatalogScan,
        allow_revision_reset: bool,
    ) -> Result<(), DatabaseError> {
        if scan.storage_uuid() != mount.storage_uuid {
            return Err(error("Catalog scan belongs to another storage"));
        }
        let pool = self.pool.clone();
        tokio::spawn(async move {
            let generation = Uuid::new_v4().to_string();
            let result = rebuild(&pool, &mount, &generation, scan, allow_revision_reset).await;
            // Only discard this attempt if it did not become the active head.
            // This also handles a lost SQLite commit acknowledgement.
            let cleanup = sqlx::query(
                "DELETE FROM catalog_projection_entries WHERE storage_uuid = ?1 AND generation = ?2 \
                 AND NOT EXISTS(SELECT 1 FROM catalog_projection_heads WHERE storage_uuid = ?1 AND generation = ?2)",
            ).bind(mount.storage_uuid.to_string()).bind(&generation).execute(&pool).await;
            result?;
            cleanup?;
            Ok(())
        }).await.map_err(error)?
    }
}

async fn rebuild(
    pool: &SqlitePool,
    mount: &CatalogMount,
    generation: &str,
    mut scan: CatalogScan,
    allow_revision_reset: bool,
) -> Result<(), DatabaseError> {
    let uuid = mount.storage_uuid.to_string();
    let revision = i64::try_from(scan.revision()).map_err(error)?;
    let previous: Option<String> = sqlx::query_scalar(
        "SELECT h.generation FROM catalog_projection_heads h JOIN catalog_mounts m ON m.storage_uuid = h.storage_uuid \
         WHERE m.alias = ? AND m.storage_uuid = ? AND m.epoch = ?",
    ).bind(&mount.alias).bind(&uuid).bind(mount.epoch.to_string()).fetch_one(pool).await?;
    loop {
        let entries = scan.next_page(128).await.map_err(error)?;
        if entries.is_empty() {
            break;
        }
        let mut tx = pool.begin().await?;
        for entry in entries {
            sqlx::query(
                "INSERT INTO catalog_projection_entries \
                 (storage_uuid, generation, recording_uuid, created_at_ms, length, size, entry) VALUES (?, ?, ?, ?, ?, ?, ?)",
            ).bind(&uuid).bind(generation).bind(entry.record.recording_id.to_string())
                .bind(entry.record.created_at_ms).bind(entry.record.length)
                .bind(i64::try_from(entry.record.size).map_err(error)?).bind(json(&entry)?)
                .execute(&mut *tx).await?;
        }
        tx.commit().await?;
    }
    let mut tx = pool.begin().await?;
    let changed = sqlx::query(
        "UPDATE catalog_projection_heads SET generation = ?1, revision = ?6 \
         WHERE storage_uuid = ?2 AND generation IS ?3 AND (revision <= ?6 OR ?7) \
         AND EXISTS(SELECT 1 FROM catalog_mounts WHERE alias = ?4 AND storage_uuid = ?2 AND epoch = ?5)",
    ).bind(generation).bind(&uuid).bind(&previous).bind(&mount.alias).bind(mount.epoch.to_string())
        .bind(revision).bind(allow_revision_reset).execute(&mut *tx).await?.rows_affected();
    if changed != 1 {
        return Err(error("Catalog mount or projection changed during refresh"));
    }
    sqlx::query("UPDATE catalog_mounts SET online = 1, error = NULL WHERE storage_uuid = ?")
        .bind(&uuid)
        .execute(&mut *tx)
        .await?;
    if let Some(previous) = previous {
        sqlx::query(
            "DELETE FROM catalog_projection_entries WHERE storage_uuid = ? AND generation = ?",
        )
        .bind(&uuid)
        .bind(previous)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
