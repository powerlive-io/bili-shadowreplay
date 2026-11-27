use super::*;

fn storage_record(live: &str) -> CatalogRecord {
    CatalogRecord {
        recording_id: Uuid::new_v4(),
        platform: "bilibili".into(),
        room_id: "123".into(),
        live_id: live.into(),
        parent_id: "parent".into(),
        title: "Storage title".into(),
        created_at_ms: 1,
        length: 10.0,
        size: 100,
        cover: None,
        results: serde_json::Value::Null,
    }
}

#[tokio::test]
async fn adoption_only_sync_preserves_storage_and_legacy_snapshot_without_a_journal() {
    let (root, db, storage, _) = fixture().await;
    for live in ["adopt", "deleted", "transferred"] {
        create(&db, &storage, live, "a").await;
    }
    db.start_record_summary("bilibili", "123", "adopt", "job", false)
        .await
        .unwrap();
    db.complete_record_summary(
        "bilibili",
        "123",
        "adopt",
        "local-only summary",
        "[]",
        "test",
        "test",
    )
    .await
    .unwrap();
    let loc = storage.catalog_location("a").await.unwrap();
    let writer = loc.open_writer(true).await.unwrap();
    let mut remote = storage_record("adopt");
    remote.parent_id.clear(); // Even the media migration must not normalize an adopted JSON.
    let a = writer.put(remote, None).await.unwrap();
    let deleted = writer.put(storage_record("deleted"), None).await.unwrap();
    writer
        .delete_recording(deleted.record.clone(), deleted.revision)
        .await
        .unwrap();
    writer
        .finish_deletion(deleted.record.recording_id)
        .await
        .unwrap();
    let moved = writer
        .put(storage_record("transferred"), None)
        .await
        .unwrap();
    writer
        .retire_transfer(Uuid::new_v4(), moved.record.recording_id, moved.revision)
        .await
        .unwrap();
    writer.close().await.unwrap();
    let dir = root.path().join("a/.bsr");
    let doc = dir.join(format!("records/{}/recording.json", a.record.recording_id));
    let original = std::fs::read(&doc).unwrap();
    storage.start_catalogs(false).await.unwrap();
    let report = storage.sync_catalog("a".into()).await.unwrap().unwrap();
    assert_eq!((report.inserted, report.adopted, report.skipped), (0, 1, 2));
    assert_eq!(report.differences[0].live_id, "adopt");
    assert!(
        report.differences[0].fields.contains(&"标题")
            && report.differences[0].fields.contains(&"摘要")
    );
    assert_eq!(
        db.catalogs.get().unwrap().resolve("adopt").await.unwrap().1,
        a.record.recording_id
    );
    assert_eq!(
        db.get_record("123", "adopt").await.unwrap().title,
        "Storage title"
    );
    assert!(
        db.get_record("123", "deleted").await.is_err()
            && db.get_record("123", "transferred").await.is_err()
    );
    let snapshot: String =
        sqlx::query_scalar("SELECT record FROM catalog_legacy_matches WHERE live_id='adopt'")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    let saved: CatalogRecord = from_json(&snapshot).unwrap();
    assert_eq!(saved.title, "Title");
    assert_eq!(
        saved.results["summary"]["summary_markdown"],
        "local-only summary"
    );
    assert!(storage.sync_catalog("a".into()).await.unwrap().is_none());
    assert!(!dir.join("import.json").exists());
    assert_eq!(std::fs::read(doc).unwrap(), original);
    assert_ready(&db).await;
    close(&db).await;
}

#[tokio::test]
async fn late_binding_conflict_leaves_catalog_mountable_and_new_plan_only_inserts_gaps() {
    let (root, db, storage, _) = fixture().await;
    for live in ["1-missing", "2-existing"] {
        create(&db, &storage, live, "a").await;
    }
    let loc = storage.catalog_location("a").await.unwrap();
    let writer = loc.open_writer(true).await.unwrap();
    let a = writer
        .put(storage_record("2-existing"), None)
        .await
        .unwrap();
    writer.close().await.unwrap();
    let wrong = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO catalog_record_keys VALUES ('2-existing',?)")
        .bind(&wrong)
        .execute(&db.pool)
        .await
        .unwrap();
    storage.start_catalogs(false).await.unwrap();
    let failure = storage
        .sync_catalog("a".into())
        .await
        .unwrap_err()
        .to_string();
    assert!(
        failure.contains("bilibili/123/2-existing") && failure.contains(&wrong),
        "{failure}"
    );
    assert!(!root.path().join("a/.bsr/import.json").exists());
    assert_eq!(
        loc.open_reader()
            .await
            .unwrap()
            .scan()
            .await
            .unwrap()
            .entries(),
        std::slice::from_ref(&a)
    );
    sqlx::query("DELETE FROM catalog_record_keys WHERE live_id='2-existing'")
        .execute(&db.pool)
        .await
        .unwrap();
    let report = storage.sync_catalog("a".into()).await.unwrap().unwrap();
    assert_eq!((report.inserted, report.adopted, report.skipped), (1, 1, 0));
    let plan: String = sqlx::query_scalar("SELECT plan FROM catalog_legacy_imports")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(from_json::<ImportPlan>(&plan).unwrap().count, 1);
    assert_eq!(
        db.catalogs
            .get()
            .unwrap()
            .resolve("2-existing")
            .await
            .unwrap()
            .1,
        a.record.recording_id
    );
    close(&db).await;
}

#[tokio::test]
async fn ambiguous_scope_and_foreign_unfinished_journal_are_not_adopted() {
    let (root, db, storage, _) = fixture().await;
    create(&db, &storage, "same-live", "a").await;
    let loc = storage.catalog_location("a").await.unwrap();
    let writer = loc.open_writer(true).await.unwrap();
    let mut other_room = storage_record("same-live");
    other_room.room_id = "other-room".into();
    writer.put(other_room.clone(), None).await.unwrap();
    writer.close().await.unwrap();
    storage.start_catalogs(false).await.unwrap();
    let journal = root.path().join("a/.bsr/import.json");
    let conflict = storage
        .sync_catalog("a".into())
        .await
        .unwrap_err()
        .to_string();
    assert!(
        conflict.contains("123/same-live") && conflict.contains("other-room/same-live"),
        "{conflict}"
    );
    assert!(!journal.exists());
    assert_eq!(
        loc.open_reader()
            .await
            .unwrap()
            .get(other_room.recording_id)
            .await
            .unwrap()
            .unwrap()
            .record,
        other_room
    );

    let writer = loc.open_writer(false).await.unwrap();
    let foreign = ImportPlan {
        storage_uuid: writer.identity().storage_uuid,
        import_id: Uuid::new_v4(),
        count: 0,
        digest: ImportDigest::default(),
    };
    writer.begin_import(foreign).await.unwrap();
    writer.close().await.unwrap();
    let original = std::fs::read(&journal).unwrap();
    let conflict = storage
        .sync_catalog("a".into())
        .await
        .unwrap_err()
        .to_string();
    assert!(
        conflict.contains("未完成") && conflict.contains("冻结计划"),
        "{conflict}"
    );
    assert_eq!(std::fs::read(journal).unwrap(), original);
    close(&db).await;
}

#[tokio::test]
async fn poisoned_old_plan_keeps_its_digest_prefix_and_adopts_without_writing_b() {
    let (root, db, storage, _) = fixture().await;
    for live in ["1-committed", "2-overlap", "3-missing"] {
        create(&db, &storage, live, "a").await;
    }
    // This would normally trigger media metadata normalization on sync.
    sqlx::query("UPDATE records SET parent_id='' WHERE live_id='1-committed'")
        .execute(&db.pool)
        .await
        .unwrap();
    let loc = storage.catalog_location("a").await.unwrap();
    let writer = loc.open_writer(true).await.unwrap();
    // Freeze all three as B's, exactly as an older app without preflight did.
    let plan = db
        .prepare_legacy_catalog_import("a", &writer.import_inventory().await.unwrap())
        .await
        .unwrap();
    let a = writer.put(storage_record("2-overlap"), None).await.unwrap();
    let rows: Vec<String> =
        sqlx::query_scalar("SELECT record FROM catalog_legacy_entries ORDER BY ordinal")
            .fetch_all(&db.pool)
            .await
            .unwrap();
    let b: CatalogRecord = from_json(&rows[1]).unwrap();
    writer.begin_import(plan.clone()).await.unwrap();
    writer
        .import_record(plan.clone(), 0, from_json(&rows[0]).unwrap())
        .await
        .unwrap();
    assert!(writer
        .import_record(plan.clone(), 1, b.clone())
        .await
        .is_err());
    writer.close().await.unwrap();
    let dir = root.path().join("a/.bsr");
    let path = |id| dir.join(format!("records/{id}/recording.json"));
    let original = std::fs::read(path(a.record.recording_id)).unwrap();
    let first = path(from_json::<CatalogRecord>(&rows[0]).unwrap().recording_id);
    let prefix_doc = std::fs::read(&first).unwrap();
    let journal = dir.join("import.json");
    let blocked = std::fs::read(&journal).unwrap();
    storage.start_catalogs(false).await.unwrap();
    // A different local snapshot must not satisfy even one pending item.
    sqlx::query("UPDATE catalog_legacy_entries SET record=json_set(record,'$.title','changed') WHERE ordinal=2").execute(&db.pool).await.unwrap();
    assert!(storage.sync_catalog("a".into()).await.is_err());
    assert_eq!(std::fs::read(&journal).unwrap(), blocked);
    sqlx::query("UPDATE catalog_legacy_entries SET record=? WHERE ordinal=2")
        .bind(&rows[2])
        .execute(&db.pool)
        .await
        .unwrap();
    // Lose the final local acknowledgement, then retry against a completed
    // journal. The adoption and original receipt sequence must survive intact.
    sqlx::raw_sql(
        "CREATE TRIGGER lose_import_ack BEFORE UPDATE OF complete ON catalog_legacy_imports
        BEGIN SELECT RAISE(ABORT,'lost local acknowledgement'); END;",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    assert!(storage
        .sync_catalog("a".into())
        .await
        .unwrap_err()
        .to_string()
        .contains("lost local acknowledgement"));
    let finished = std::fs::read(&journal).unwrap();
    sqlx::query("DROP TRIGGER lose_import_ack")
        .execute(&db.pool)
        .await
        .unwrap();
    let report = storage.sync_catalog("a".into()).await.unwrap().unwrap();
    assert_eq!((report.inserted, report.adopted, report.skipped), (2, 1, 0));
    assert_eq!(std::fs::read(&journal).unwrap(), finished);
    assert_eq!(std::fs::read(first).unwrap(), prefix_doc);
    assert_eq!(
        std::fs::read(path(a.record.recording_id)).unwrap(),
        original
    );
    assert!(!path(b.recording_id).exists());
    assert_eq!(
        db.catalogs
            .get()
            .unwrap()
            .resolve("2-overlap")
            .await
            .unwrap()
            .1,
        a.record.recording_id
    );
    let log: serde_json::Value = serde_json::from_slice(&std::fs::read(journal).unwrap()).unwrap();
    assert_eq!(log["progress"]["plan"], serde_json::to_value(plan).unwrap());
    assert_eq!(log["progress"]["complete"], true);
    assert_eq!(log["progress"]["next_index"], 3);
    assert_ready(&db).await;
    close(&db).await;
    assert_eq!(
        loc.open_reader()
            .await
            .unwrap()
            .scan()
            .await
            .unwrap()
            .entries()
            .len(),
        3
    );
}
