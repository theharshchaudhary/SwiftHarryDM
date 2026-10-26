//! SQLite persistence: WAL journal, synchronous=FULL (a committed transition survives power
//! loss), forward-only migrations with a backup first, refusal to touch a newer schema.
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::models::{Job, JobSpec, JobState, Segment};
use crate::grabber::{Item, ItemKind, ItemStatus, Project};
use crate::scheduler::Queue;

pub const SCHEMA_VERSION: i64 = 2;

const MIGRATION_1: &str = r#"
CREATE TABLE jobs (
    id TEXT PRIMARY KEY, spec TEXT NOT NULL, state TEXT NOT NULL, reason TEXT, version INTEGER NOT NULL,
    kind TEXT, title TEXT, final_url TEXT, dest_dir TEXT NOT NULL, reserved_name TEXT, temp_path TEXT,
    artifact_path TEXT, final_path TEXT, total_bytes INTEGER, committed_bytes INTEGER NOT NULL DEFAULT 0,
    etag TEXT, last_modified TEXT, content_type TEXT, accept_ranges INTEGER, size_verified INTEGER,
    media TEXT, error TEXT, attempts INTEGER NOT NULL DEFAULT 0, created_at REAL NOT NULL,
    updated_at REAL NOT NULL, completed_at REAL, last_try REAL, cookies_enc TEXT
);
CREATE TABLE segments (
    job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE, idx INTEGER NOT NULL,
    start INTEGER NOT NULL, "end" INTEGER NOT NULL, committed INTEGER NOT NULL, PRIMARY KEY (job_id, idx)
);
CREATE TABLE queues (id TEXT PRIMARY KEY, data TEXT NOT NULL, position INTEGER NOT NULL);
-- No two unfinished jobs may target the same file name in the same folder.
CREATE UNIQUE INDEX jobs_reservation ON jobs(lower(dest_dir), lower(reserved_name))
    WHERE reserved_name IS NOT NULL AND state NOT IN ('COMPLETED', 'CANCELLED');
CREATE TABLE schema_version (v INTEGER NOT NULL);
"#;

/// Site grabber projects and what exploring them found.
const MIGRATION_2: &str = r#"
CREATE TABLE grabber_projects (
    id TEXT PRIMARY KEY, data TEXT NOT NULL, created_at REAL NOT NULL, updated_at REAL NOT NULL
);
CREATE TABLE grabber_items (
    id INTEGER PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES grabber_projects(id) ON DELETE CASCADE,
    url TEXT NOT NULL, parent_url TEXT, depth INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('page','file')),
    content_type TEXT, size INTEGER, link_text TEXT,
    checked INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'found' CHECK (status IN ('found','explored','skipped','failed','queued')),
    note TEXT,
    -- the download made from this item (not a foreign key: the item is saved before the
    -- manager writes the download, and the download may later be removed from the list)
    job_id TEXT,
    UNIQUE (project_id, url)
);
CREATE INDEX grabber_items_project ON grabber_items(project_id, kind, status);
CREATE INDEX grabber_items_job ON grabber_items(job_id);
"#;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("state database schema v{0} is newer than this app (v{1}); refusing to modify it")]
    TooNew(i64, i64),
    #[error("name reservation conflict")]
    ReservationConflict,
}

pub struct Store {
    conn: Mutex<Connection>,
    path: PathBuf,
}

fn job_from_row(r: &Row) -> rusqlite::Result<Job> {
    let spec: String = r.get("spec")?;
    let mut spec: JobSpec = serde_json::from_str(&spec).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let cookies: Option<String> = r.get("cookies_enc")?;
    spec.cookies = cookies.and_then(|c| crate::secrets::unprotect_str(&c));
    let state: String = r.get("state")?;
    let opt_bool = |v: Option<i64>| v.map(|x| x != 0);
    Ok(Job {
        id: r.get("id")?,
        spec,
        state: JobState::parse(&state).unwrap_or(JobState::Failed),
        reason: r.get("reason")?,
        version: r.get::<_, i64>("version")? as u64,
        kind: r.get("kind")?,
        title: r.get("title")?,
        final_url: r.get("final_url")?,
        reserved_name: r.get("reserved_name")?,
        temp_path: r.get("temp_path")?,
        artifact_path: r.get("artifact_path")?,
        final_path: r.get("final_path")?,
        total_bytes: r.get::<_, Option<i64>>("total_bytes")?.map(|v| v as u64),
        committed_bytes: r.get::<_, i64>("committed_bytes")? as u64,
        etag: r.get("etag")?,
        last_modified: r.get("last_modified")?,
        content_type: r.get("content_type")?,
        accept_ranges: opt_bool(r.get("accept_ranges")?),
        size_verified: opt_bool(r.get("size_verified")?),
        media: r.get::<_, Option<String>>("media")?.and_then(|s| serde_json::from_str(&s).ok()),
        error: r.get::<_, Option<String>>("error")?.and_then(|s| serde_json::from_str(&s).ok()),
        attempts: r.get::<_, i64>("attempts")? as u32,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        completed_at: r.get("completed_at")?,
        last_try: r.get("last_try")?,
    })
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(&path)?;
        conn.busy_timeout(std::time::Duration::from_secs(30))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let s = Store { conn: Mutex::new(conn), path };
        s.migrate()?;
        Ok(s)
    }

    pub fn schema_version(&self) -> Result<i64, StoreError> {
        let c = self.conn.lock().unwrap();
        let has: Option<String> = c
            .query_row("SELECT name FROM sqlite_master WHERE type='table' AND name='schema_version'", [], |r| r.get(0))
            .optional()?;
        if has.is_none() {
            return Ok(0);
        }
        Ok(c.query_row("SELECT v FROM schema_version", [], |r| r.get(0)).optional()?.unwrap_or(0))
    }

    fn migrate(&self) -> Result<(), StoreError> {
        let cur = self.schema_version()?;
        if cur > SCHEMA_VERSION {
            return Err(StoreError::TooNew(cur, SCHEMA_VERSION));
        }
        if cur == SCHEMA_VERSION {
            return Ok(());
        }
        if cur > 0 {
            // A backup left by an earlier attempt is kept; this one gets the next free name.
            let name = self.path.file_name().unwrap().to_string_lossy().into_owned();
            let mut dest = self.path.with_file_name(format!("{name}.bak-v{cur}"));
            let mut n = 1;
            while dest.exists() {
                n += 1;
                dest = self.path.with_file_name(format!("{name}.bak-v{cur}-{n}"));
            }
            self.backup(&dest)?;
        }
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        if cur < 1 {
            tx.execute_batch(MIGRATION_1)?;
        }
        if cur < 2 {
            tx.execute_batch(MIGRATION_2)?;
        }
        tx.execute("DELETE FROM schema_version", [])?;
        tx.execute("INSERT INTO schema_version (v) VALUES (?1)", [SCHEMA_VERSION])?;
        tx.commit()?;
        Ok(())
    }

    pub fn backup(&self, dest: &Path) -> Result<(), StoreError> {
        let c = self.conn.lock().unwrap();
        c.execute("VACUUM INTO ?1", [dest.to_string_lossy()])?;
        Ok(())
    }

    pub fn integrity_check(&self) -> Result<String, StoreError> {
        Ok(self.conn.lock().unwrap().query_row("PRAGMA integrity_check", [], |r| r.get(0))?)
    }

    // ---------------------------------------------------------------- jobs
    pub fn save_job(&self, j: &Job) -> Result<(), StoreError> {
        let c = self.conn.lock().unwrap();
        let cookies = j.spec.cookies.as_deref().and_then(crate::secrets::protect_str);
        let r = c.execute(
            r#"INSERT INTO jobs (id, spec, state, reason, version, kind, title, final_url, dest_dir, reserved_name,
                temp_path, artifact_path, final_path, total_bytes, committed_bytes, etag, last_modified, content_type,
                accept_ranges, size_verified, media, error, attempts, created_at, updated_at, completed_at, last_try,
                cookies_enc)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28)
               ON CONFLICT(id) DO UPDATE SET spec=excluded.spec, state=excluded.state, reason=excluded.reason,
                version=excluded.version, kind=excluded.kind, title=excluded.title, final_url=excluded.final_url,
                dest_dir=excluded.dest_dir, reserved_name=excluded.reserved_name, temp_path=excluded.temp_path,
                artifact_path=excluded.artifact_path, final_path=excluded.final_path, total_bytes=excluded.total_bytes,
                committed_bytes=excluded.committed_bytes, etag=excluded.etag, last_modified=excluded.last_modified,
                content_type=excluded.content_type, accept_ranges=excluded.accept_ranges,
                size_verified=excluded.size_verified, media=excluded.media, error=excluded.error,
                attempts=excluded.attempts, created_at=excluded.created_at, updated_at=excluded.updated_at,
                completed_at=excluded.completed_at, last_try=excluded.last_try, cookies_enc=excluded.cookies_enc"#,
            params![
                j.id,
                serde_json::to_string(&j.spec).unwrap(),
                j.state.as_str(),
                j.reason,
                j.version as i64,
                j.kind,
                j.title,
                j.final_url,
                j.spec.dest_dir,
                j.reserved_name,
                j.temp_path,
                j.artifact_path,
                j.final_path,
                j.total_bytes.map(|v| v as i64),
                j.committed_bytes as i64,
                j.etag,
                j.last_modified,
                j.content_type,
                j.accept_ranges.map(|b| b as i64),
                j.size_verified.map(|b| b as i64),
                j.media.as_ref().map(|m| m.to_string()),
                j.error.as_ref().map(|e| serde_json::to_string(e).unwrap()),
                j.attempts as i64,
                j.created_at,
                j.updated_at,
                j.completed_at,
                j.last_try,
                cookies,
            ],
        );
        match r {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => {
                Err(StoreError::ReservationConflict)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn load_jobs(&self) -> Result<Vec<Job>, StoreError> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT * FROM jobs ORDER BY created_at, id")?;
        let jobs = st.query_map([], job_from_row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(jobs)
    }

    pub fn load_job(&self, id: &str) -> Result<Option<Job>, StoreError> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row("SELECT * FROM jobs WHERE id=?1", [id], job_from_row).optional()?)
    }

    pub fn delete_job(&self, id: &str) -> Result<(), StoreError> {
        self.conn.lock().unwrap().execute("DELETE FROM jobs WHERE id=?1", [id])?;
        Ok(())
    }

    // ---------------------------------------------------------------- segments
    pub fn load_segments(&self, job_id: &str) -> Result<Vec<Segment>, StoreError> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(r#"SELECT idx, start, "end", committed FROM segments WHERE job_id=?1 ORDER BY start"#)?;
        let v = st
            .query_map([job_id], |r| {
                Ok(Segment {
                    idx: r.get::<_, i64>(0)? as u32,
                    start: r.get::<_, i64>(1)? as u64,
                    end: r.get(2)?,
                    committed: r.get::<_, i64>(3)? as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    }

    pub fn replace_segments(&self, job_id: &str, segs: &[Segment]) -> Result<(), StoreError> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM segments WHERE job_id=?1", [job_id])?;
        for s in segs {
            tx.execute(
                r#"INSERT INTO segments (job_id, idx, start, "end", committed) VALUES (?1,?2,?3,?4,?5)"#,
                params![job_id, s.idx as i64, s.start as i64, s.end, s.committed as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn commit_segment(&self, job_id: &str, idx: u32, committed: u64) -> Result<(), StoreError> {
        self.conn.lock().unwrap().execute(
            "UPDATE segments SET committed=?1 WHERE job_id=?2 AND idx=?3",
            params![committed as i64, job_id, idx as i64],
        )?;
        Ok(())
    }

    /// Shrink segment `idx` and add `new_seg` atomically, so no byte range is ever lost.
    pub fn split_segment(&self, job_id: &str, idx: u32, new_end: i64, new_seg: &Segment) -> Result<(), StoreError> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute(r#"UPDATE segments SET "end"=?1 WHERE job_id=?2 AND idx=?3"#, params![new_end, job_id, idx as i64])?;
        tx.execute(
            r#"INSERT INTO segments (job_id, idx, start, "end", committed) VALUES (?1,?2,?3,?4,?5)"#,
            params![job_id, new_seg.idx as i64, new_seg.start as i64, new_seg.end, new_seg.committed as i64],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_segments(&self, job_id: &str) -> Result<(), StoreError> {
        self.conn.lock().unwrap().execute("DELETE FROM segments WHERE job_id=?1", [job_id])?;
        Ok(())
    }

    // ---------------------------------------------------------------- queues
    pub fn save_queue(&self, q: &Queue, position: i64) -> Result<(), StoreError> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO queues (id, data, position) VALUES (?1,?2,?3)
             ON CONFLICT(id) DO UPDATE SET data=excluded.data, position=excluded.position",
            params![q.id, serde_json::to_string(q).unwrap(), position],
        )?;
        Ok(())
    }

    pub fn load_queues(&self) -> Result<Vec<Queue>, StoreError> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT data FROM queues ORDER BY position, id")?;
        let v = st
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(|s| s.ok().and_then(|s| serde_json::from_str(&s).ok()))
            .collect();
        Ok(v)
    }

    pub fn delete_queue(&self, id: &str) -> Result<(), StoreError> {
        self.conn.lock().unwrap().execute("DELETE FROM queues WHERE id=?1", [id])?;
        Ok(())
    }

    // ------------------------------------------------------------------ site grabber
    pub fn save_grabber_project(&self, p: &Project) -> Result<(), StoreError> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO grabber_projects (id, data, created_at, updated_at) VALUES (?1,?2,?3,?4)
             ON CONFLICT(id) DO UPDATE SET data=excluded.data, updated_at=excluded.updated_at",
            params![p.id, serde_json::to_string(p).unwrap(), p.created_at, p.updated_at],
        )?;
        Ok(())
    }

    pub fn grabber_projects(&self) -> Result<Vec<Project>, StoreError> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT data FROM grabber_projects ORDER BY updated_at DESC")?;
        let v = st
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(|s| s.ok().and_then(|s| serde_json::from_str::<Project>(&s).ok()))
            .map(Project::normalized)
            .collect();
        Ok(v)
    }

    pub fn delete_grabber_project(&self, id: &str) -> Result<(), StoreError> {
        self.conn.lock().unwrap().execute("DELETE FROM grabber_projects WHERE id=?1", [id])?;
        Ok(())
    }

    /// Insert an item found while exploring, or update the one with the same address.
    /// Returns the item's id.
    pub fn save_grabber_item(&self, it: &Item) -> Result<i64, StoreError> {
        let c = self.conn.lock().unwrap();
        let kind = match it.kind {
            ItemKind::Page => "page",
            ItemKind::File => "file",
        };
        c.execute(
            "INSERT INTO grabber_items (project_id, url, parent_url, depth, kind, content_type, size, link_text, checked, status, note, job_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(project_id, url) DO UPDATE SET parent_url=excluded.parent_url, depth=excluded.depth, kind=excluded.kind,
               content_type=excluded.content_type, size=excluded.size, link_text=excluded.link_text, checked=excluded.checked,
               status=excluded.status, note=excluded.note, job_id=excluded.job_id",
            params![
                it.project_id,
                it.url,
                it.parent_url,
                it.depth,
                kind,
                it.content_type,
                it.size.map(|v| v as i64),
                it.link_text,
                it.checked,
                it.status.as_str(),
                it.note,
                it.job_id
            ],
        )?;
        Ok(c.query_row("SELECT id FROM grabber_items WHERE project_id=?1 AND url=?2", params![it.project_id, it.url], |r| r.get(0))?)
    }

    pub fn grabber_items(&self, project_id: &str) -> Result<Vec<Item>, StoreError> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT id, project_id, url, parent_url, depth, kind, content_type, size, link_text, checked, status, note, job_id
             FROM grabber_items WHERE project_id=?1 ORDER BY id",
        )?;
        let v = st
            .query_map([project_id], |r| {
                Ok(Item {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    url: r.get(2)?,
                    parent_url: r.get(3)?,
                    depth: r.get(4)?,
                    kind: if r.get::<_, String>(5)? == "page" { ItemKind::Page } else { ItemKind::File },
                    content_type: r.get(6)?,
                    size: r.get::<_, Option<i64>>(7)?.map(|v| v as u64),
                    link_text: r.get(8)?,
                    checked: r.get(9)?,
                    status: ItemStatus::parse(&r.get::<_, String>(10)?),
                    note: r.get(11)?,
                    job_id: r.get(12)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    }

    /// Forget what a project found (before exploring it again from scratch).
    pub fn clear_grabber_items(&self, project_id: &str) -> Result<(), StoreError> {
        self.conn.lock().unwrap().execute("DELETE FROM grabber_items WHERE project_id=?1", [project_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::{ErrorCode, SdmError};
    use crate::models::MediaOptions;

    fn job(id: &str, dest: &str, name: Option<&str>, state: JobState) -> Job {
        let mut spec = JobSpec::new("https://x.example/f.bin", dest);
        spec.media = Some(MediaOptions::preset("720"));
        let mut j = Job::new(id.into(), spec);
        j.state = state;
        j.reserved_name = name.map(Into::into);
        j
    }

    #[test]
    fn roundtrip_with_encrypted_cookies() {
        let d = tempfile::tempdir().unwrap();
        let st = Store::open(d.path().join("s.db")).unwrap();
        let mut j = job("a", "C:/d", Some("f.bin"), JobState::Queued);
        j.error = Some(SdmError::new(ErrorCode::Network, "down").retryable().0);
        j.media = Some(serde_json::json!({"format_id": "137+140"}));
        j.accept_ranges = Some(true);
        j.spec.cookies = Some("SID=secret-cookie".into());
        st.save_job(&j).unwrap();
        assert_eq!(st.load_jobs().unwrap()[0], j);
        drop(st);
        let raw = std::fs::read(d.path().join("s.db")).unwrap();
        let wal = std::fs::read(d.path().join("s.db-wal")).unwrap_or_default();
        let needle = b"secret-cookie";
        assert!(!raw.windows(needle.len()).any(|w| w == needle) && !wal.windows(needle.len()).any(|w| w == needle));
    }

    #[test]
    fn reservation_unique_among_unfinished() {
        let d = tempfile::tempdir().unwrap();
        let st = Store::open(d.path().join("s.db")).unwrap();
        st.save_job(&job("a", "C:/d", Some("F.bin"), JobState::Queued)).unwrap();
        assert!(matches!(
            st.save_job(&job("b", "C:/d", Some("f.BIN"), JobState::Queued)),
            Err(StoreError::ReservationConflict)
        ));
        st.save_job(&job("c", "C:/other", Some("f.bin"), JobState::Queued)).unwrap();
        st.save_job(&job("d", "C:/d", Some("f.bin"), JobState::Completed)).unwrap();
    }

    #[test]
    fn segment_split_atomic() {
        let d = tempfile::tempdir().unwrap();
        let st = Store::open(d.path().join("s.db")).unwrap();
        st.save_job(&job("a", "C:/d", None, JobState::Queued)).unwrap();
        st.replace_segments("a", &[Segment { idx: 0, start: 0, end: 999, committed: 100 }]).unwrap();
        st.split_segment("a", 0, 499, &Segment::new(1, 500, 999)).unwrap();
        let s: Vec<_> = st.load_segments("a").unwrap().iter().map(|s| (s.start, s.end, s.committed)).collect();
        assert_eq!(s, vec![(0, 499, 100), (500, 999, 0)]);
        assert!(st.split_segment("a", 0, 249, &Segment::new(1, 250, 499)).is_err());
        let s: Vec<_> = st.load_segments("a").unwrap().iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(s, vec![(0, 499), (500, 999)]);
    }

    #[test]
    fn refuses_newer_schema_and_backs_up() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.db");
        let st = Store::open(&p).unwrap();
        st.save_job(&job("a", "C:/d", None, JobState::Queued)).unwrap();
        st.backup(&d.path().join("b.db")).unwrap();
        assert_eq!(Store::open(d.path().join("b.db")).unwrap().load_jobs().unwrap().len(), 1);
        drop(st);
        Connection::open(&p).unwrap().execute("UPDATE schema_version SET v=99", []).unwrap();
        assert!(matches!(Store::open(&p), Err(StoreError::TooNew(99, _))));
    }

    #[test]
    fn queues_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let st = Store::open(d.path().join("s.db")).unwrap();
        st.save_queue(&Queue::main(), 0).unwrap();
        assert_eq!(st.load_queues().unwrap(), vec![Queue::main()]);
    }

    #[test]
    fn upgrades_a_version_1_database() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.db");
        {
            let c = Connection::open(&p).unwrap();
            c.execute_batch(MIGRATION_1).unwrap();
            c.execute("INSERT INTO schema_version (v) VALUES (1)", []).unwrap();
        }
        {
            let st = Store::open(&p).unwrap();
            st.save_job(&job("old", "C:/d", None, JobState::Completed)).unwrap();
        }
        // Pretend that job was saved by version 1, then open with this version again.
        Connection::open(&p).unwrap().execute("UPDATE schema_version SET v=1", []).unwrap();
        Connection::open(&p).unwrap().execute_batch("DROP TABLE grabber_items; DROP TABLE grabber_projects;").unwrap();
        let st = Store::open(&p).unwrap();
        assert_eq!(st.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(st.load_jobs().unwrap().len(), 1, "the downloads survive the upgrade");
        assert!(d.path().join("s.db.bak-v1").exists(), "a backup is made first");
        assert!(d.path().join("s.db.bak-v1-2").exists(), "an earlier backup is not in the way");
    }

    #[test]
    fn grabber_projects_and_items() {
        let d = tempfile::tempdir().unwrap();
        let st = Store::open(d.path().join("s.db")).unwrap();
        let p = Project { id: "p1".into(), name: "Docs".into(), start_url: "https://a.example/".into(), ..Project::default() };
        st.save_grabber_project(&p).unwrap();
        assert_eq!(st.grabber_projects().unwrap()[0].name, "Docs");
        let mut it = Item {
            id: 0,
            project_id: "p1".into(),
            url: "https://a.example/f.pdf".into(),
            parent_url: Some("https://a.example/".into()),
            depth: 1,
            kind: ItemKind::File,
            content_type: None,
            size: None,
            link_text: Some("F".into()),
            checked: true,
            status: ItemStatus::Found,
            note: None,
            job_id: None,
        };
        let id = st.save_grabber_item(&it).unwrap();
        it.size = Some(42);
        it.status = ItemStatus::Queued;
        assert_eq!(st.save_grabber_item(&it).unwrap(), id, "the same address updates the same item");
        let items = st.grabber_items("p1").unwrap();
        assert_eq!((items.len(), items[0].size, items[0].status), (1, Some(42), ItemStatus::Queued));
        st.delete_grabber_project("p1").unwrap();
        assert!(st.grabber_items("p1").unwrap().is_empty(), "items go with their project");
    }
}
