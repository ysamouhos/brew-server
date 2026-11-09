//! Append-only binary persistence for monitor history (completed calls and SDS).
//!
//! Format: a flat file of framed records. Each record is:
//!   [u32 little-endian length N][N bytes of bincode-serialized `StoredRecord`]
//!
//! Appends are durable in order and crash-safe on read: a torn trailing record
//! (partial length or body, e.g. from a crash mid-write) is detected and the
//! replay stops cleanly at the last complete record rather than erroring.
//!
//! Retention (`[storage] keep_days`, `max_calls`, `max_sds`) and the
//! dashboard's "Clear" buttons rewrite the log with only the records kept
//! (`Store::compact`). What was dropped is added to `<path>.totals` (JSON), so
//! the call / SDS totals keep counting it. Old readers still read the log: its
//! format is unchanged.

use crate::monitor::{CallRecord, SdsRecord};
use crate::telemetry::SdsTelemetryRecord;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::collections::HashSet;
use std::sync::Mutex;

/// One persisted event, appended in the order it occurred.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StoredRecord {
    /// A completed call (recorded when it ends).
    Call(CallRecord),
    /// An SDS transfer.
    Sds(SdsRecord),
    /// A delivery report for a previously stored SDS (by uuid).
    SdsReport { uuid: uuid::Uuid },
    /// An SDS log entry observed on a Basestation telemetry channel.
    SdsTelemetry(SdsTelemetryRecord),
}

/// Append-only log writer/reader. Cheap to clone-share via `Arc`.
pub struct Store {
    path: PathBuf,
    file: Mutex<std::fs::File>,
    /// Content hashes of every record in the log, so a record replicated
    /// from the HA peer that is already here (one this node wrote and the
    /// peer copied earlier) is not appended twice. See `append_replicated`.
    known: Mutex<HashSet<u64>>,
    /// Bumped by every `compact`: byte offsets into the log from before are
    /// no longer valid (HA replication starts over from 0, see `read_frames`).
    generation: std::sync::atomic::AtomicU64,
}

/// Calls / SDS / voice frames dropped from the log by `compact`, kept in
/// `<path>.totals` so the totals survive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroppedTotals {
    pub calls: u64,
    pub sds: u64,
    pub voice_frames: u64,
}

/// FNV-1a over a record's serialized body.
fn body_hash(body: &[u8]) -> u64 {
    body.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Splits framed log bytes into record bodies, stopping at a torn or
/// corrupt frame. Returns the bodies and how many bytes they covered.
pub fn split_frames(buf: &[u8]) -> (Vec<&[u8]>, usize) {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 4 <= buf.len() {
        let len = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]) as usize;
        let end = pos + 4 + len;
        if end > buf.len() {
            break;
        }
        out.push(&buf[pos + 4..end]);
        pos = end;
    }
    (out, pos)
}

impl Store {
    /// Opens (creating if needed) the log at `path` for appending. Parent
    /// directories are created if missing.
    pub fn open(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;
        let mut known = HashSet::new();
        if let Ok(buf) = std::fs::read(&path) {
            for body in split_frames(&buf).0 {
                known.insert(body_hash(body));
            }
        }
        Ok(Self { path, file: Mutex::new(file), known: Mutex::new(known), generation: Default::default() })
    }

    fn totals_path(&self) -> PathBuf {
        let mut p = self.path.clone().into_os_string();
        p.push(".totals");
        PathBuf::from(p)
    }

    /// What earlier compactions dropped (zero without a totals file).
    pub fn dropped_totals(&self) -> DroppedTotals {
        std::fs::read(self.totals_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Whether byte `offset` is the start of a record (or the end) of the
    /// log as it is now: an offset handed out before a `compact` may not be.
    pub fn is_boundary(&self, offset: u64) -> bool {
        let Ok(buf) = std::fs::read(&self.path) else { return offset == 0 };
        let mut pos = 0u64;
        while pos < offset {
            let p = pos as usize;
            if p + 4 > buf.len() { return false; }
            pos += 4 + u32::from_le_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]) as u64;
        }
        pos == offset
    }

    /// See `generation`.
    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Rewrites the log with only the records `keep` accepts (given all
    /// records, oldest first, it returns one flag per record), atomically
    /// (temporary file, then rename). Appends wait meanwhile. The calls / SDS
    /// dropped are added to `<path>.totals`. Returns what was dropped now.
    pub fn compact(&self, keep: impl FnOnce(&[StoredRecord]) -> Vec<bool>) -> std::io::Result<DroppedTotals> {
        let mut f = self.file.lock().unwrap();
        let buf = std::fs::read(&self.path)?;
        let bodies = split_frames(&buf).0;
        let mut recs = Vec::with_capacity(bodies.len());
        for body in &bodies {
            match bincode::deserialize::<StoredRecord>(body) {
                Ok(r) => recs.push(r),
                Err(_) => break,
            }
        }
        let flags = keep(&recs);
        let mut dropped = DroppedTotals::default();
        let mut out = Vec::with_capacity(buf.len());
        let mut known = HashSet::new();
        for (i, rec) in recs.iter().enumerate() {
            if flags.get(i).copied().unwrap_or(true) {
                let body = bodies[i];
                out.extend_from_slice(&(body.len() as u32).to_le_bytes());
                out.extend_from_slice(body);
                known.insert(body_hash(body));
                continue;
            }
            match rec {
                StoredRecord::Call(c) => { dropped.calls += 1; dropped.voice_frames += c.voice_frames; }
                StoredRecord::Sds(_) => dropped.sds += 1,
                _ => {}
            }
        }
        if out.len() == buf.len() {
            return Ok(dropped);
        }
        if dropped != DroppedTotals::default() {
            let mut t = self.dropped_totals();
            t.calls += dropped.calls;
            t.sds += dropped.sds;
            t.voice_frames += dropped.voice_frames;
            let tp = self.totals_path();
            let tmp = tp.with_extension("totals.tmp");
            std::fs::write(&tmp, serde_json::to_vec(&t).unwrap_or_default())?;
            std::fs::rename(&tmp, &tp)?;
        }
        let mut tmp = self.path.clone().into_os_string();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        std::fs::write(&tmp, &out)?;
        std::fs::rename(&tmp, &self.path)?;
        *f = std::fs::OpenOptions::new().create(true).append(true).read(true).open(&self.path)?;
        *self.known.lock().unwrap() = known;
        self.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(dropped)
    }

    /// Appends one record, framed with a u32 length prefix, and flushes.
    pub fn append(&self, rec: &StoredRecord) -> std::io::Result<()> {
        let body = bincode::serialize(rec)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.append_body(&body)
    }

    fn append_body(&self, body: &[u8]) -> std::io::Result<()> {
        let len = body.len() as u32;
        let mut f = self.file.lock().unwrap();
        f.write_all(&len.to_le_bytes())?;
        f.write_all(body)?;
        f.flush()?;
        self.known.lock().unwrap().insert(body_hash(body));
        Ok(())
    }

    /// Appends a record body received from the HA peer, unless an identical
    /// record is already in the log. Returns the decoded record when it was
    /// new, so the caller can add it to the in-memory history.
    ///
    /// Identity is by content: two genuinely separate but byte-identical
    /// records (in practice only a second `SdsReport` for the same SDS) are
    /// kept once.
    pub fn append_replicated(&self, body: &[u8]) -> std::io::Result<Option<StoredRecord>> {
        if self.known.lock().unwrap().contains(&body_hash(body)) {
            return Ok(None);
        }
        let rec: StoredRecord = bincode::deserialize(body)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.append_body(body)?;
        Ok(Some(rec))
    }

    /// Reads up to `max` bytes of complete frames starting at byte `offset`
    /// of the log, for HA replication. An offset past the end (the log was
    /// replaced) restarts from 0. Returns (start, frames, end).
    pub fn read_frames(&self, offset: u64, max: usize) -> std::io::Result<(u64, Vec<u8>, u64)> {
        use std::io::Seek;
        let mut f = std::fs::File::open(&self.path)?;
        let len = f.metadata()?.len();
        let start = if offset > len { 0 } else { offset };
        f.seek(std::io::SeekFrom::Start(start))?;
        let mut buf = Vec::new();
        (&mut f).take(max as u64).read_to_end(&mut buf)?;
        let (bodies, used) = split_frames(&buf);
        let suspect = (bodies.is_empty() && buf.len() >= 4)
            || bodies.first().is_some_and(|b| bincode::deserialize::<StoredRecord>(b).is_err());
        if start > 0 && suspect && !self.is_boundary(start) {
            // Not on a record boundary: the log was compacted since this
            // offset was handed out. Start over (the peer skips what it has).
            return self.read_frames(0, max);
        }
        if bodies.is_empty() && buf.len() >= 4 {
            // A single record larger than `max`: read exactly that one.
            let need = 4 + u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
            if (start + need as u64) <= len {
                f.seek(std::io::SeekFrom::Start(start))?;
                let mut one = vec![0u8; need];
                f.read_exact(&mut one)?;
                return Ok((start, one, start + need as u64));
            }
        }
        buf.truncate(used);
        Ok((start, buf, start + used as u64))
    }

    /// Reads the entire log from `path`, returning every complete record. A torn
    /// trailing record is ignored. A missing file yields an empty vector.
    pub fn replay(path: impl AsRef<Path>) -> std::io::Result<Vec<StoredRecord>> {
        let path = path.as_ref();
        let mut file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let mut out = Vec::new();
        for body in split_frames(&buf).0 {
            match bincode::deserialize::<StoredRecord>(body) {
                Ok(rec) => out.push(rec),
                Err(_) => break, // corrupt frame; stop at last good record
            }
        }
        Ok(out)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Which records `[storage]` retention keeps (one flag per record, `recs`
/// oldest first): calls and SDS younger than `keep_days`, the newest
/// `max_calls` calls and `max_sds` SDS (and telemetry SDS entries), delivery
/// reports of kept SDS. `clear_calls` / `clear_sds` drop every call / every
/// SDS, report and telemetry SDS entry.
pub fn retention(recs: &[StoredRecord], keep: &crate::config::StorageConfig, now_ms: u64, clear_calls: bool, clear_sds: bool) -> Vec<bool> {
    let cut = if keep.keep_days == 0 { 0 } else { now_ms.saturating_sub(keep.keep_days as u64 * 86_400_000) };
    let lim = |n: usize| if n == 0 { usize::MAX } else { n };
    let (mut calls, mut sds, mut tele) = (0usize, 0usize, 0usize);
    let mut flags = vec![false; recs.len()];
    let mut kept_sds: HashSet<uuid::Uuid> = HashSet::new();
    // Newest first, so the count limits keep the newest.
    for (i, rec) in recs.iter().enumerate().rev() {
        flags[i] = match rec {
            StoredRecord::Call(c) => !clear_calls && c.ended_at_ms.unwrap_or(c.started_at_ms) >= cut && { calls += 1; calls <= lim(keep.max_calls) },
            StoredRecord::Sds(r) => {
                let k = !clear_sds && r.at_ms >= cut && { sds += 1; sds <= lim(keep.max_sds) };
                if k { kept_sds.insert(r.uuid); }
                k
            }
            StoredRecord::SdsTelemetry(t) => !clear_sds && t.at_ms >= cut && { tele += 1; tele <= lim(keep.max_sds) },
            StoredRecord::SdsReport { .. } => false,
        };
    }
    for (i, rec) in recs.iter().enumerate() {
        if let StoredRecord::SdsReport { uuid } = rec { flags[i] = kept_sds.contains(uuid); }
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    /// After a compaction an old byte offset can fall inside a record: it is
    /// not a boundary, and reading from it starts over at 0.
    #[test]
    fn compaction_moves_replication_offsets_back_to_a_boundary() {
        let p = tmp("compact");
        let s = Store::open(&p).unwrap();
        for v in 0..10 { s.append(&StoredRecord::Call(call(v))).unwrap(); }
        let (_, _, end) = s.read_frames(0, 1 << 20).unwrap();
        assert!(s.is_boundary(end) && s.generation() == 0);
        // Drop the odd ones: the old end now points past or into a record.
        s.compact(|r| (0..r.len()).map(|i| i % 2 == 0).collect()).unwrap();
        assert_eq!(s.generation(), 1);
        assert_eq!(Store::replay(&p).unwrap().len(), 5);
        let (_, frames, _) = s.read_frames(7, 1 << 20).unwrap();
        assert_eq!(split_frames(&frames).0.len(), 5, "an offset inside a record starts over");
        assert!(!s.is_boundary(7));
        std::fs::remove_file(&p).ok();
        std::fs::remove_file(format!("{}.totals", p.display())).ok();
    }
    use crate::monitor::{CallRecord, SdsRecord};

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("brew-store-test-{}-{}.bin", name, uuid::Uuid::new_v4().simple()))
    }

    fn call(v: u64) -> CallRecord {
        CallRecord { uuid: uuid::Uuid::new_v4(), kind: "group".into(), source: 90, destination: 1001, priority: 0, started_at_ms: v, ended_at_ms: Some(v + 5), voice_frames: 3 }
    }
    fn sds(v: u64) -> SdsRecord {
        SdsRecord { uuid: uuid::Uuid::new_v4(), source: 90, destination: 100, at_ms: v, reports: 0 }
    }

    #[test]
    fn append_and_replay_roundtrip() {
        let p = tmp("roundtrip");
        let s = Store::open(&p).unwrap();
        s.append(&StoredRecord::Call(call(1))).unwrap();
        s.append(&StoredRecord::Sds(sds(2))).unwrap();
        let rep_uuid = uuid::Uuid::new_v4();
        s.append(&StoredRecord::SdsReport { uuid: rep_uuid }).unwrap();
        drop(s);

        let recs = Store::replay(&p).unwrap();
        assert_eq!(recs.len(), 3);
        assert!(matches!(recs[0], StoredRecord::Call(_)));
        assert!(matches!(recs[1], StoredRecord::Sds(_)));
        assert!(matches!(recs[2], StoredRecord::SdsReport { uuid } if uuid == rep_uuid));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn appends_persist_across_reopen() {
        let p = tmp("reopen");
        { let s = Store::open(&p).unwrap(); s.append(&StoredRecord::Call(call(1))).unwrap(); }
        { let s = Store::open(&p).unwrap(); s.append(&StoredRecord::Call(call(2))).unwrap(); }
        let recs = Store::replay(&p).unwrap();
        assert_eq!(recs.len(), 2, "append mode must not truncate");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn torn_trailing_record_is_ignored() {
        let p = tmp("torn");
        let s = Store::open(&p).unwrap();
        s.append(&StoredRecord::Call(call(1))).unwrap();
        drop(s);
        // Corrupt: append a bogus length prefix with no/short body.
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(&9999u32.to_le_bytes()).unwrap();
        f.write_all(&[1, 2, 3]).unwrap(); // far short of 9999
        drop(f);

        let recs = Store::replay(&p).unwrap();
        assert_eq!(recs.len(), 1, "should recover the one complete record and stop");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn replicated_records_are_deduplicated_and_tail_reads_resume() {
        let (pa, pb) = (tmp("repl-a"), tmp("repl-b"));
        let a = Store::open(&pa).unwrap();
        let b = Store::open(&pb).unwrap();
        a.append(&StoredRecord::Call(call(1))).unwrap();
        a.append(&StoredRecord::Sds(sds(2))).unwrap();

        // b copies a's log in two small reads.
        let (start, frames, mid) = a.read_frames(0, 8).unwrap();
        assert_eq!(start, 0);
        let (_, rest, end) = a.read_frames(mid, 1 << 20).unwrap();
        for body in split_frames(&frames).0.into_iter().chain(split_frames(&rest).0) {
            assert!(b.append_replicated(body).unwrap().is_some());
        }
        assert_eq!(end, std::fs::metadata(&pa).unwrap().len());
        assert_eq!(a.read_frames(end, 1 << 20).unwrap().1.len(), 0, "nothing new");

        // Copying a's log back from b adds nothing to a.
        let (_, back, _) = b.read_frames(0, 1 << 20).unwrap();
        for body in split_frames(&back).0 {
            assert!(a.append_replicated(body).unwrap().is_none());
        }
        assert_eq!(Store::replay(&pa).unwrap().len(), 2);
        // Dedup survives a reopen.
        let a2 = Store::open(&pa).unwrap();
        assert!(a2.append_replicated(split_frames(&back).0[0]).unwrap().is_none());
        // An offset past the end restarts from 0.
        assert_eq!(a.read_frames(end + 100, 1 << 20).unwrap().0, 0);
        std::fs::remove_file(&pa).ok();
        std::fs::remove_file(&pb).ok();
    }

    #[test]
    fn missing_file_replays_empty() {
        let p = tmp("missing");
        let recs = Store::replay(&p).unwrap();
        assert!(recs.is_empty());
    }
}

