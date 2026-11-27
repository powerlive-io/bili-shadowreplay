use super::*;
use crate::database::catalog::import::LegacyImportReport;
use crate::database::catalog::runtime::Handle;
use recording_catalog::{
    object_store::{
        aws::{AmazonS3, AmazonS3Builder},
        prefix::PrefixStore,
        ClientOptions,
    },
    CatalogLocation,
};
use std::time::Duration;

fn resolved_root(path: &std::path::Path) -> io::Result<std::path::PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .ok_or_else(|| io::Error::other("invalid storage root"))?;
            let name = path.file_name().ok_or_else(|| {
                io::Error::other("storage roots must not contain unresolved '..'")
            })?;
            Ok(resolved_root(parent)?.join(name))
        }
        Err(error) => Err(error),
    }
}

pub(super) fn validate_catalog_namespace(
    id: &str,
    config: &crate::config::StorageConfig,
) -> io::Result<()> {
    let selected = &config.backends[id];
    for (other_id, other) in &config.backends {
        if other_id == id {
            continue;
        }
        let overlap = match (&selected.s3, &other.s3) {
            (Some(a), Some(b)) => {
                a.service_endpoint()? == b.service_endpoint()?
                    && a.bucket == b.bucket
                    && (a.prefix.is_empty()
                        || b.prefix.is_empty()
                        || a.prefix == b.prefix
                        || a.prefix.starts_with(&format!("{}/", b.prefix))
                        || b.prefix.starts_with(&format!("{}/", a.prefix)))
            }
            (None, None) if selected.kind == "local" && other.kind == "local" => {
                let a = resolved_root(std::path::Path::new(&selected.root))?;
                let b = resolved_root(std::path::Path::new(&other.root))?;
                a.starts_with(&b) || b.starts_with(&a)
            }
            _ => false,
        };
        let overlap = overlap
            || match (&selected.webdav, &other.webdav) {
                (Some(a), Some(b)) => a.overlaps(b)?,
                _ => false,
            };
        if overlap {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("存储 {id} 与 {other_id} 的目录或前缀重叠，请使用独立命名空间"),
            ));
        }
    }
    Ok(())
}

impl ArchiveStorage {
    pub async fn start_catalogs(self: &Arc<Self>, readonly: bool) -> io::Result<()> {
        self.catalog_readonly
            .store(readonly, std::sync::atomic::Ordering::Release);
        let first = !self
            .catalog_started
            .swap(true, std::sync::atomic::Ordering::AcqRel);
        self.refresh_catalogs(false).await?;
        if first && self.db.catalogs.get().is_none() {
            // Do not switch a running legacy event stream after its producer
            // leases drain: queued final updates may still be in flight.
            self.catalog_startup_deferred
                .store(true, std::sync::atomic::Ordering::Release);
        }
        if first {
            let weak = Arc::downgrade(self);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    let Some(storage) = weak.upgrade() else { break };
                    if let Err(error) = storage.refresh_catalogs(false).await {
                        log::debug!("Catalog refresh deferred: {error}");
                    }
                }
            });
        }
        Ok(())
    }

    pub async fn refresh_catalogs(self: &Arc<Self>, force: bool) -> io::Result<()> {
        let storage = self.clone();
        tokio::spawn(async move { storage.refresh_catalogs_inner(force).await })
            .await
            .map_err(io::Error::other)?
    }

    async fn refresh_catalogs_inner(&self, force: bool) -> io::Result<()> {
        if self
            .catalog_startup_deferred
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok(());
        }
        if !self
            .catalog_started
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok(());
        }
        let gate = self
            .configuration_gate
            .try_write()
            .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "存储正在使用中"))?;
        self.reconcile_catalogs(force, None, gate).await.map(|_| ())
    }

    pub async fn sync_catalog(
        self: &Arc<Self>,
        id: String,
    ) -> io::Result<Option<LegacyImportReport>> {
        let storage = self.clone();
        // Finish attaching even if the client leaves mid-request.
        tokio::spawn(async move {
            storage
                .db
                .ensure_catalog_startup_complete()
                .map_err(database_error)?;
            if storage.db.catalogs.get().is_none() {
                return Err(io::Error::other("存储目录尚未初始化"));
            }
            let gate = storage
                .configuration_gate
                .try_write()
                .map_err(|_| io::Error::other("存储正在使用中，请稍后重试"))?;
            storage.reconcile_catalogs(true, Some(&id), gate).await
        })
        .await
        .map_err(io::Error::other)?
    }

    pub(super) async fn backend_in_use(&self, id: &str) -> io::Result<bool> {
        if self
            .db
            .catalog_recording_active(id)
            .await
            .map_err(database_error)?
        {
            return Ok(true);
        }
        if self
            .active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .is_some_and(|leases| leases.iter().any(|lease| lease.strong_count() > 0))
        {
            return Ok(true);
        }
        // On startup there are normally no mutation leases. Don't enumerate
        // every historical recording merely to intersect it with an empty set.
        if !self
            .mutations
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .any(|lock| lock.strong_count() > 0)
        {
            return Ok(false);
        }
        let records = self
            .db
            .storage_transfer_recordings(id)
            .await
            .map_err(database_error)?;
        let mutations = self.mutations.lock().unwrap_or_else(|p| p.into_inner());
        Ok(records.iter().any(|(platform, room, live)| {
            mutations
                .get(&format!("{platform}/{room}/{live}"))
                .is_some_and(|lock| lock.strong_count() > 0)
        }))
    }

    pub(super) async fn reconcile_catalogs(
        &self,
        force: bool,
        only: Option<&str>,
        gate: tokio::sync::RwLockWriteGuard<'_, ()>,
    ) -> io::Result<Option<LegacyImportReport>> {
        if self
            .catalog_startup_deferred
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok(None);
        }
        self.initialize().await?;
        let config = self.configuration().await?;
        if only.is_some_and(|id| !config.backends.contains_key(id)) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "未配置此存储"));
        }
        // An old file-transfer journal must finish under its original rules.
        // Recording/mutation leases also prevent freezing a moving legacy source.
        if self.db.catalogs.get().is_none() {
            if self
                .db
                .has_legacy_catalog_transfers()
                .await
                .map_err(database_error)?
            {
                return Ok(None);
            }
            for id in config.backends.keys() {
                if self.backend_in_use(id).await? {
                    return Ok(None);
                }
            }
        }
        let runtime = self.db.enable_catalogs().await.map_err(database_error)?;
        let open: Vec<_> = runtime.handles.lock().await.keys().cloned().collect();
        for id in open {
            if !config.backends.contains_key(&id) {
                runtime.detach(&id).await.map_err(database_error)?;
            }
        }
        let mut reconnect = Vec::new();
        for id in config.backends.keys() {
            if only.is_some_and(|selected| selected != id) {
                continue;
            }
            if self.backend_in_use(id).await? {
                if only.is_some() {
                    return Err(io::Error::other(
                        "此存储正在使用中，请停止录制或等待操作完成",
                    ));
                }
                continue;
            }
            let ready = self
                .db
                .catalog_status()
                .await
                .map_err(database_error)?
                .iter()
                .any(|status| status.alias == *id && status.ready);
            let healthy = ready
                && runtime.handles.lock().await.get(id).is_some_and(|handle| {
                    (handle.writer.is_some()
                        || self
                            .catalog_readonly
                            .load(std::sync::atomic::Ordering::Acquire))
                        && handle.credential_revision.as_ref()
                            == config.credential_revisions.get(id)
                });
            if healthy && !force {
                continue;
            }
            // Never refresh the compatibility projection across a pending move.
            if self
                .db
                .storage_transfer_journals()
                .await
                .map_err(database_error)?
                .iter()
                .any(|journal| journal.source_id == *id || journal.target_id == *id)
            {
                if only.is_some() {
                    return Err(io::Error::other("请先完成此存储的录播转移"));
                }
                if runtime.handles.lock().await.contains_key(id) {
                    continue;
                }
            }
            // While producers are excluded, revoke access to this backend.
            // New recording IDs also fail writable() until attach succeeds.
            self.db
                .catalog_failed(id, "正在连接存储目录")
                .await
                .map_err(database_error)?;
            runtime.detach(id).await.map_err(database_error)?;
            reconnect.push(id);
        }
        // Keep configuration/credentials stable, but do not exclude ordinary
        // readers or new producers on OTHER backends during remote I/O.
        // A second refresh/config replacement cannot race this read guard.
        let _configuration = tokio::sync::RwLockWriteGuard::downgrade(gate);
        let results = futures::future::join_all(reconnect.into_iter().map(|id| async move {
            match self.attach_catalog(id, force, only.is_some()).await {
                Ok(report) => Ok(report),
                Err(error) => {
                    self.db.startup_preview.discard(id);
                    self.db
                        .catalog_failed(id, &error.to_string())
                        .await
                        .map_err(database_error)?;
                    log::warn!("Storage catalog {id} unavailable: {error}");
                    if only.is_some() {
                        Err(error)
                    } else {
                        Ok(None)
                    }
                }
            }
        }))
        .await;
        // Join every scan before releasing configuration protection, including
        // when persisting an error for one backend fails.
        let mut report = None;
        for result in results {
            if let Some(import) = result? {
                report = Some(import);
            }
        }
        Ok(report)
    }

    async fn attach_catalog(
        &self,
        id: &str,
        rescan: bool,
        manual_sync: bool,
    ) -> io::Result<Option<LegacyImportReport>> {
        validate_catalog_namespace(id, &self.configuration().await?)?;
        let expected = self
            .db
            .catalog_status()
            .await
            .map_err(database_error)?
            .into_iter()
            .find(|status| status.alias == id)
            .and_then(|status| status.storage_uuid);
        let needs_import = self
            .db
            .catalog_needs_import(id)
            .await
            .map_err(database_error)?;
        let readonly = self
            .catalog_readonly
            .load(std::sync::atomic::Ordering::Acquire);
        if needs_import && (!manual_sync || readonly) {
            return Err(io::Error::other(
                "此存储有旧录播待迁移，请在可写实例的存储管理中点击「同步」",
            ));
        }
        let definition = self.configuration().await?.backends[id].clone();
        // Only a genuinely new local location may be initialized. A missing
        // known mount must not become an empty directory on a different disk.
        if definition.kind == "local" && expected.is_none() && !needs_import && !readonly {
            tokio::fs::create_dir_all(&definition.root).await?;
        }
        let runtime = self.db.catalogs.get().expect("catalog runtime");
        let location = self.catalog_location(id).await?;
        // Claim an existing UUID before allowing a new alias to access its
        // local cache. A copied/replaced root must not overwrite another mount.
        let expected = match expected {
            Some(uuid) => Some(uuid.parse().map_err(io::Error::other)?),
            None => match location.identity().await {
                Ok(identity) => {
                    self.db
                        .catalog_mount_for_alias(id, identity.storage_uuid)
                        .await
                        .map_err(database_error)?;
                    Some(identity.storage_uuid)
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
            },
        };
        let location = location.with_local_cache(runtime.pool.clone(), expected, rescan);
        // Preview active records from this verified mount as soon as they are
        // available (local cache or cold scan), not after all writers attach.
        let observer = (cfg!(feature = "gui")
            && self.db.ensure_catalog_startup_complete().is_err())
        .then(|| {
            let db = self.db.clone();
            let alias = id.to_owned();
            Arc::new(
                move |_: uuid::Uuid, record: &recording_catalog::CatalogRecord| {
                    db.startup_preview.observe(&alias, record);
                    db.notify_coalesced_change(crate::events::DataChange::Records);
                },
            ) as recording_catalog::ScanObserver
        });
        let (writer, reader) = if readonly {
            (None, Some(Arc::new(location.open_reader().await?)))
        } else {
            match location
                .open_writer_observed(expected.is_none(), observer.clone())
                .await
            {
                Ok(writer) => (Some(writer), None),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock && !needs_import => {
                    self.db.startup_preview.discard(id);
                    (None, Some(Arc::new(location.open_reader().await?)))
                }
                Err(error) => return Err(error),
            }
        };
        let uuid = writer
            .as_ref()
            .map(|writer| writer.identity().storage_uuid)
            .or_else(|| reader.as_ref().map(|reader| reader.identity().storage_uuid))
            .expect("opened catalog");
        let result = async {
            let mut present_before_import = std::collections::HashSet::new();
            let report = if needs_import {
                let writer = writer
                    .as_ref()
                    .ok_or_else(|| io::Error::other("旧录播迁移需要存储写入权限"))?;
                // Media conversion is a separate pass. It must not rewrite even
                // an already-published prefix while recovering a frozen plan.
                present_before_import = writer.known_live_ids().await?;
                Some(
                    self.db
                        .import_legacy_catalog(id, writer)
                        .await
                        .map_err(database_error)?,
                )
            } else {
                None
            };
            if manual_sync {
                if let Some(writer) = &writer {
                    crate::migration::migration_methods::migrate_legacy_archives(
                        &self.db,
                        id,
                        &self.store(id).await?,
                        writer,
                        &present_before_import,
                    )
                    .await?;
                }
            }
            // A persisted JSON deletion marker survives a crash between metadata
            // publication and file removal. Retry only those explicit deletions.
            if let Some(writer) = &writer {
                let pending = writer.pending_deletions().await?;
                if !pending.is_empty() {
                    let store = self.store(id).await?;
                    for record in pending {
                        store
                            .recording(&record.platform, &record.room_id, &record.live_id)?
                            .remove_all()
                            .await?;
                        writer.finish_deletion(record.recording_id).await?;
                    }
                }
            }
            let mount = self
                .db
                .catalog_mount_for_alias(id, uuid)
                .await
                .map_err(database_error)?;
            let scan = match &writer {
                Some(writer) => writer.scan().await?,
                None => {
                    reader
                        .as_ref()
                        .expect("reader")
                        .scan_observed(observer.as_ref())
                        .await?
                }
            };
            let republish = rescan
                || !self
                    .db
                    .catalog_projection_matches(&mount, &scan)
                    .await
                    .map_err(database_error)?;
            if republish {
                self.db
                    .rebuild_catalog_projection(mount, scan, rescan)
                    .await
                    .map_err(database_error)?;
            }
            let handle = Handle {
                uuid,
                writer: writer.clone(),
                _reader: reader.clone(),
                credential_revision: self
                    .configuration()
                    .await?
                    .credential_revisions
                    .get(id)
                    .cloned(),
            };
            runtime
                .attach(id.to_owned(), handle, republish)
                .await
                .map_err(database_error)?;
            Ok(report)
        }
        .await;
        if result.is_err() {
            if let Some(writer) = writer {
                let _ = writer.close().await;
            }
        }
        result
    }

    /// Catalogs and media share the exact configured root/prefix and credentials.
    /// Their object_store major versions are isolated behind their own APIs.
    /// This only resolves the location; it never initializes a missing catalog.
    pub(crate) async fn catalog_location(&self, id: &str) -> io::Result<CatalogLocation> {
        let config = self.configuration().await?;
        let definition = config
            .backends
            .get(id)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "catalog backend is not configured")
            })?
            .normalized()?;
        if let Some(webdav) = &definition.webdav {
            let credentials = self
                .db
                .storage_credentials(id)
                .await
                .map_err(database_error)?;
            return Ok(CatalogLocation::recording_backend(Arc::new(
                recording_storage::WebDavBackend::new(webdav, credentials.webdav()?)?,
            )));
        }
        let Some(s3) = definition.s3 else {
            return CatalogLocation::local(definition.root);
        };
        let credentials = self
            .db
            .storage_credentials(id)
            .await
            .map_err(database_error)?;
        // Match the media backend's literal key parsing; Into<Path> would
        // encode already-valid percent signs and other prefix characters.
        let prefix = recording_catalog::object_store::path::Path::parse(&s3.prefix)
            .map_err(io::Error::other)?;
        Ok(CatalogLocation::object_store(Arc::new(PrefixStore::new(
            catalog_s3(&s3, credentials.s3()?)?,
            prefix,
        ))))
    }
}

fn catalog_s3(
    s3: &recording_storage::S3Config,
    credentials: &recording_storage::S3Credentials,
) -> io::Result<AmazonS3> {
    let endpoint = s3.client_endpoint()?;
    credentials.validate()?;
    // Never from_env: AWS environment variables cannot redirect this mount.
    let mut builder = AmazonS3Builder::new()
        .with_bucket_name(&s3.bucket)
        .with_region(&s3.region)
        .with_virtual_hosted_style_request(!s3.path_style)
        .with_access_key_id(&credentials.access_key_id)
        .with_secret_access_key(&credentials.secret_access_key)
        .with_client_options(
            ClientOptions::new()
                .with_allow_http(s3.endpoint.starts_with("http://"))
                .with_connect_timeout(Duration::from_secs(10))
                .with_timeout(Duration::from_secs(120)),
        );
    if let Some(endpoint) = endpoint {
        builder = builder.with_endpoint(endpoint);
    }
    if !credentials.session_token.is_empty() {
        builder = builder.with_token(&credentials.session_token);
    }
    builder.build().map_err(io::Error::other)
}

#[cfg(test)]
mod endpoint_tests {
    use super::*;
    use recording_catalog::object_store::{path::Path, signer::Signer};

    #[test]
    fn webdav_overlap_uses_the_full_collection_and_does_not_treat_remote_roots_as_local() {
        let mut config = crate::config::StorageConfig::local(
            std::env::temp_dir().to_string_lossy().into_owned(),
        );
        let backend = |endpoint: &str, prefix: &str| crate::config::RecordingBackendConfig {
            kind: "webdav".into(),
            root: String::new(),
            s3: None,
            webdav: Some(recording_storage::WebDavConfig {
                endpoint: endpoint.into(),
                prefix: prefix.into(),
            }),
        };
        config
            .backends
            .insert("dav".into(), backend("https://host/dav/", "app"));
        validate_catalog_namespace("dav", &config).unwrap();
        config
            .backends
            .insert("other".into(), backend("https://host/dav/app/", "nested"));
        assert!(validate_catalog_namespace("dav", &config).is_err());
        config
            .backends
            .insert("other".into(), backend("https://host/dav/", "app2"));
        validate_catalog_namespace("dav", &config).unwrap();
    }

    #[test]
    fn bucket_qualified_endpoints_cannot_bypass_catalog_overlap_checks() {
        let a = recording_storage::S3Config {
            endpoint: "https://s3.oss-cn-shanghai.aliyuncs.com".into(),
            bucket: "bsr-cache".into(),
            region: "cn-shanghai".into(),
            prefix: "app".into(),
            path_style: true,
        };
        let b = recording_storage::S3Config {
            endpoint: "https://bsr-cache.s3.oss-cn-shanghai.aliyuncs.com".into(),
            prefix: "app/nested".into(),
            path_style: false,
            ..a.clone()
        };
        let config = crate::config::StorageConfig {
            default_backend: "a".into(),
            credential_revisions: HashMap::new(),
            backends: [("a", a), ("b", b)]
                .into_iter()
                .map(|(id, s3)| {
                    (
                        id.into(),
                        crate::config::RecordingBackendConfig {
                            kind: "s3".into(),
                            root: String::new(),
                            s3: Some(s3),
                            webdav: None,
                        },
                    )
                })
                .collect(),
        };
        assert!(validate_catalog_namespace("a", &config).is_err());
    }

    #[tokio::test]
    async fn catalog_requests_include_the_bucket_in_the_selected_address_style() {
        let credentials = recording_storage::S3Credentials {
            access_key_id: "test-access".into(),
            secret_access_key: "test-only-secret".into(),
            session_token: String::new(),
        };
        for (endpoint, path_style, host, port, path) in [
            (
                "https://s3.oss-cn-shanghai.aliyuncs.com",
                false,
                "bsr-cache.s3.oss-cn-shanghai.aliyuncs.com",
                None,
                "/.bsr/storage.json",
            ),
            (
                "https://bsr-cache.s3.oss-cn-shanghai.aliyuncs.com",
                false,
                "bsr-cache.s3.oss-cn-shanghai.aliyuncs.com",
                None,
                "/.bsr/storage.json",
            ),
            (
                "https://s3.oss-cn-shanghai.aliyuncs.com",
                true,
                "s3.oss-cn-shanghai.aliyuncs.com",
                None,
                "/bsr-cache/.bsr/storage.json",
            ),
            (
                "http://objects.example:9000/",
                false,
                "bsr-cache.objects.example",
                Some(9000),
                "/.bsr/storage.json",
            ),
            (
                "",
                false,
                "bsr-cache.s3.us-east-1.amazonaws.com",
                None,
                "/.bsr/storage.json",
            ),
            (
                "",
                true,
                "s3.us-east-1.amazonaws.com",
                None,
                "/bsr-cache/.bsr/storage.json",
            ),
        ] {
            let config = recording_storage::S3Config {
                endpoint: endpoint.into(),
                bucket: "bsr-cache".into(),
                region: "us-east-1".into(),
                prefix: String::new(),
                path_style,
            };
            let store = catalog_s3(&config, &credentials).unwrap();
            for method in [reqwest::Method::GET, reqwest::Method::PUT] {
                let url = store
                    .signed_url(
                        method,
                        &Path::from(".bsr/storage.json"),
                        Duration::from_secs(60),
                    )
                    .await
                    .unwrap();
                assert_eq!(url.host_str(), Some(host));
                assert_eq!(url.port(), port);
                assert_eq!(url.path(), path);
            }
        }
    }
}
