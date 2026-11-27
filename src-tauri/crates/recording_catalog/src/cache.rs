//! A local mirror of complete storage documents, including tombstones and receipts.
//! A committed head distinguishes an empty catalog from one never fully scanned.
use super::*;
use sqlx::SqlitePool;

pub const SCHEMA: &str = "
    CREATE TABLE catalog_document_cache (
        storage_uuid TEXT NOT NULL, recording_uuid TEXT NOT NULL, document BLOB NOT NULL,
        PRIMARY KEY(storage_uuid, recording_uuid)
    );
    CREATE TABLE catalog_cache_heads (
        storage_uuid TEXT PRIMARY KEY NOT NULL, complete INTEGER NOT NULL, projected_epoch TEXT
    );
";

#[derive(Clone)]
pub(super) struct Cache {
    pub pool: SqlitePool,
    pub rescan: bool,
    pub expected: Option<Uuid>,
}

impl Cache {
    pub async fn load(&self, uuid: Uuid) -> io::Result<Option<BTreeMap<Uuid, Document>>> {
        let uuid_text = uuid.to_string();
        let mut tx = self.pool.begin().await.map_err(io::Error::other)?;
        let complete: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM catalog_cache_heads WHERE storage_uuid=? AND complete=1)",
        )
        .bind(&uuid_text)
        .fetch_one(&mut *tx)
        .await
        .map_err(io::Error::other)?;
        if !complete {
            return Ok(None);
        }
        let mut docs = BTreeMap::new();
        let mut rows = sqlx::query_as::<_, (String, Vec<u8>)>(
            "SELECT recording_uuid,document FROM catalog_document_cache WHERE storage_uuid=?",
        )
        .bind(&uuid_text)
        .fetch(&mut *tx);
        while let Some((id, bytes)) = rows.try_next().await.map_err(io::Error::other)? {
            let id = id.parse().map_err(io::Error::other)?;
            docs.insert(id, decode_document(&bytes, uuid, &record_path(id))?);
        }
        Ok(Some(docs))
    }

    // Commit before touching the storage. A crash or uncertain PUT can never
    // leave a stale snapshot marked usable. Keep its rows until replacement.
    pub async fn invalidate(&self, uuid: Uuid) -> io::Result<()> {
        sqlx::query("UPDATE catalog_cache_heads SET complete=0 WHERE storage_uuid=?")
            .bind(uuid.to_string())
            .execute(&self.pool)
            .await
            .map_err(io::Error::other)?;
        Ok(())
    }

    pub async fn replace(&self, uuid: Uuid, docs: &BTreeMap<Uuid, Document>) -> io::Result<()> {
        let mut tx = self.pool.begin().await.map_err(io::Error::other)?;
        sqlx::query("DELETE FROM catalog_document_cache WHERE storage_uuid=?")
            .bind(uuid.to_string())
            .execute(&mut *tx)
            .await
            .map_err(io::Error::other)?;
        for doc in docs.values() {
            save(&mut tx, doc).await?;
        }
        // A full replacement may have changed content even with identical
        // revision numbers (e.g. a restored backup). Require republication.
        sqlx::query(
            "INSERT OR REPLACE INTO catalog_cache_heads(storage_uuid,complete) VALUES (?,1)",
        )
        .bind(uuid.to_string())
        .execute(&mut *tx)
        .await
        .map_err(io::Error::other)?;
        tx.commit().await.map_err(io::Error::other)
    }

    pub async fn update(&self, doc: &Document) -> io::Result<()> {
        let mut tx = self.pool.begin().await.map_err(io::Error::other)?;
        save(&mut tx, doc).await?;
        sqlx::query("UPDATE catalog_cache_heads SET complete=1 WHERE storage_uuid=?")
            .bind(doc.storage_uuid.to_string())
            .execute(&mut *tx)
            .await
            .map_err(io::Error::other)?;
        tx.commit().await.map_err(io::Error::other)
    }
}

type Transaction<'a> = sqlx::Transaction<'a, sqlx::Sqlite>;

async fn save(tx: &mut Transaction<'_>, doc: &Document) -> io::Result<()> {
    sqlx::query("INSERT OR REPLACE INTO catalog_document_cache VALUES (?,?,?)")
        .bind(doc.storage_uuid.to_string())
        .bind(doc.entry.record.recording_id.to_string())
        .bind(serde_json::to_vec(doc)?)
        .execute(&mut **tx)
        .await
        .map_err(io::Error::other)?;
    Ok(())
}
