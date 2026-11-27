# Recording JSON catalog

Each storage backend owns its recordings' current metadata. The application
can rebuild its SQLite projection from that metadata and uses it for
aggregation, sorting, and pagination. New catalogs require only ordinary object
GET, PUT, and LIST operations. They do not require conditional PUT, distributed
leases, a database server, compaction, or metadata GC.

## Layout and ownership

```text
.bsr/storage.json                         storage UUID, format_version = 2
.bsr/records/<uuid>/recording.json        one current document per recording
.bsr/import.json                          legacy SQLite import progress, if used
.bsr/imports/<import uuid>.json           completed receipts retained before a later batch
.bsr/writer.lock                          local filesystem lock only
```

Media paths stay unchanged. Keeping JSON outside the media namespace prevents
file transfers from accidentally publishing a source's metadata in the target
before verification and the catalog commit.

**Only one BSR instance may write to a given remote store.** There is no remote
writer detection, automatic takeover, or cross-machine CAS. While that instance
is writing, do not record, edit, delete, transfer, or modify files with another
instance or tool. Within a process, UUID-scoped guards prevent duplicate
writers; local roots also use an OS file lock. Readers may coexist. If a scan
fails, keep the last complete SQLite projection rather than publishing an
empty index.

## Publication and failure behavior

- Each recording has one JSON document, overwritten on meaningful updates.
  No timestamped history is written. Fields include identity, revision, current
  record, status, and small import/transfer retry receipts.
- A writer serializes its operations. Local writes use a same-directory tempfile,
  file fsync, atomic replacement and (on Unix) parent-directory fsync.
- Remote writes use ordinary PUT. A bounded readback verifies publication and can
  resolve a lost acknowledgement. An unresolved write outcome fails the writer
  closed until reopened. Cancelled requests retain their operation gate until the
  spawned publication finishes.
- Updates check the caller's observed revision within this single writer. This is
  optimistic application concurrency, **not distributed compare-and-swap**.
- Deletion first writes `status: deleted`, then removes assets. An unfinished
  cleanup is retried on writable remount. The small current tombstone remains
  after cleanup so delayed imports/events cannot resurrect a deleted namespace.
- Transfer receives the target document only after file verification, then marks
  the source transferred, commits the local binding and cleans up source files.
  Receipts are saved in the same JSON as the status, so retries cannot overwrite
  an unrelated target or remove a recreated source. No cross-storage transaction
  or atomic visibility to other machines is promised.
- Tombstones and retry receipts are retained; there is no automatic metadata GC.
  Growth is per recording and transfer operation, not per title/progress update.
- First mount loads current documents and builds an in-memory view used for
  publishing the SQLite index. This is O(record count), not a bounded-memory
  streaming database. A scan copies that view for stable pagination. Read-only
  remote scans compare listings before/after reads and reject concurrent changes.
- The application keeps a local SQLite document cache per storage UUID. Warm
  opens still verify storage identity and import state, but require no listing
  or individual recording GETs. Pending destructive cleanup rechecks its marker.
  Publishing invalidates the cache before the storage write, then commits the
  verified document and completeness marker together; interrupted writes are
  recovered by scanning on reconnect. After another instance has written to a
  storage, use **Sync** for that storage before starting to record again.

## Format and SQLite import

Only JSON format 2 is supported. An unsupported format is rejected by both readers
and writers, including explicit initialization; it is never rewritten as an empty
catalog. Catalog revisions come solely from current recording documents, including
deletion/transfer markers. No separate revision floor or engine backup is read.

Manual SQLite import preflights the entire frozen input against the storage
and local identity bindings. It adopts existing recordings, skips tombstones,
and journals only missing records. An adoption-only sync needs no remote import
journal. For old in-flight plans a locally persisted adoption can acknowledge
the original input without rewriting the storage document or changing the plan
digest. A supplemental batch archives the previous completed journal under
`.bsr/imports/<import UUID>.json`; incomplete foreign batches are never replaced.
Incomplete imports still cannot publish the aggregate index.

## Validation

```sh
cargo test --manifest-path src-tauri/Cargo.toml -p recording_catalog --locked
cargo clippy --manifest-path src-tauri/Cargo.toml -p recording_catalog --all-targets --locked -- -D warnings
python3 scripts/test-json-catalog-rustfs.py
```

The RustFS runner uses an already installed `rustfs/rustfs` image, a disposable
container and random process-only credentials. It never touches an existing
container or application credentials. Credentials used by the application still
come from settings, not environment variables. The local-only mock also rejects
all conditional writes, covering providers with a smaller API than RustFS.

The runner covers JSON creation and reopening, media access, cross-storage
transfers and remounting. It does not exercise unsupported development formats.
