//! Persistent clipboard history: SQLite for metadata/text, plain files on disk
//! for images (PNG + thumbnail) and copied files/folders ("the vault").

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Text longer than this is not recorded (keeps the DB and UI snappy).
const MAX_TEXT_BYTES: usize = 5 * 1024 * 1024;
const THUMB_MAX: u32 = 480;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Text,
    Image,
    Files,
}

impl Kind {
    fn parse(s: &str) -> Kind {
        match s {
            "image" => Kind::Image,
            "files" => Kind::Files,
            _ => Kind::Text,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Filter {
    All,
    Text,
    Images,
    Files,
    Pinned,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    /// Where the file was when it was copied.
    pub original: String,
    /// Path of our own copy inside the vault (None if it was too big to copy).
    pub stored: Option<String>,
    pub size: u64,
    pub is_dir: bool,
}

impl FileEntry {
    /// The best path to put back on the clipboard: our copy, else the original.
    pub fn usable_path(&self) -> Option<PathBuf> {
        if let Some(s) = &self.stored {
            let p = PathBuf::from(s);
            if p.exists() {
                return Some(p);
            }
        }
        let p = PathBuf::from(&self.original);
        p.exists().then_some(p)
    }
}

#[derive(Clone, Debug)]
pub struct Clip {
    pub id: i64,
    pub kind: Kind,
    /// Text clips: (possibly truncated) text. Image: blob file name. Files: unused.
    pub content: String,
    pub preview: String,
    pub size: u64,
    pub pinned: bool,
    pub created_at: i64,
    pub hash: String,
    pub files: Vec<FileEntry>,
    pub width: u32,
    pub height: u32,
    /// Name of the app that was in front when this was copied ("" if unknown).
    pub source_app: String,
}

pub struct Store {
    conn: Connection,
    dir: PathBuf,
    max_items: usize,
    max_copy_bytes: u64,
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hash_parts(parts: &[&[u8]]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
        h.update([0u8]);
    }
    hex(&h.finalize())
}

fn one_line(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut last_space = false;
    for c in s.chars() {
        if out.chars().count() >= max {
            out.push('…');
            break;
        }
        if c.is_whitespace() {
            if !last_space && !out.is_empty() {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out.trim_end().to_string()
}

impl Store {
    pub fn open(dir: &Path, max_items: usize, max_copy_mb: u64) -> Result<Store> {
        fs::create_dir_all(dir.join("images"))?;
        fs::create_dir_all(dir.join("files"))?;
        let conn = Connection::open(dir.join("history.db")).context("opening history.db")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS clips (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                kind       TEXT NOT NULL,
                hash       TEXT NOT NULL UNIQUE,
                content    TEXT NOT NULL,
                preview    TEXT NOT NULL,
                search     TEXT NOT NULL,
                size       INTEGER NOT NULL DEFAULT 0,
                width      INTEGER NOT NULL DEFAULT 0,
                height     INTEGER NOT NULL DEFAULT 0,
                pinned     INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS clips_order ON clips (pinned DESC, created_at DESC);",
        )?;
        // Added after the first release; older databases get it here.
        let has_source: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('clips') WHERE name='source_app'")?
            .exists([])?;
        if !has_source {
            conn.execute_batch("ALTER TABLE clips ADD COLUMN source_app TEXT NOT NULL DEFAULT ''")?;
        }
        Ok(Store {
            conn,
            dir: dir.to_path_buf(),
            max_items,
            max_copy_bytes: max_copy_mb * 1024 * 1024,
        })
    }

    pub fn image_path(&self, clip: &Clip) -> PathBuf {
        self.dir.join("images").join(&clip.content)
    }

    pub fn thumb_path(&self, clip: &Clip) -> PathBuf {
        self.dir
            .join("images")
            .join(format!("{}_thumb.png", clip.hash))
    }

    // ---------- inserting ----------

    /// If a clip with this hash exists, move it to the top and return its id.
    fn bump(&self, hash: &str) -> Result<Option<i64>> {
        let id: Option<i64> = self
            .conn
            .query_row("SELECT id FROM clips WHERE hash = ?1", [hash], |r| r.get(0))
            .optional()?;
        if let Some(id) = id {
            self.conn.execute(
                "UPDATE clips SET created_at = ?1 WHERE id = ?2",
                params![now_ms(), id],
            )?;
        }
        Ok(id)
    }

    /// Returns Some(id) if something was stored/bumped, None if ignored.
    pub fn add_text(&self, text: &str) -> Result<Option<i64>> {
        if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES {
            return Ok(None);
        }
        let hash = hash_parts(&[b"text", text.as_bytes()]);
        if let Some(id) = self.bump(&hash)? {
            return Ok(Some(id));
        }
        self.conn.execute(
            "INSERT INTO clips (kind, hash, content, preview, search, size, created_at)
             VALUES ('text', ?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                hash,
                text,
                one_line(text, 200),
                text.to_lowercase(),
                text.len() as i64,
                now_ms()
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.prune()?;
        Ok(Some(id))
    }

    /// `rgba` is width*height*4 bytes.
    pub fn add_image(&self, width: u32, height: u32, rgba: &[u8]) -> Result<Option<i64>> {
        if width == 0 || height == 0 || rgba.len() != (width as usize) * (height as usize) * 4 {
            return Ok(None);
        }
        let hash = hash_parts(&[b"image", &width.to_le_bytes(), &height.to_le_bytes(), rgba]);
        if let Some(id) = self.bump(&hash)? {
            return Ok(Some(id));
        }
        let img =
            image::RgbaImage::from_raw(width, height, rgba.to_vec()).context("bad image buffer")?;
        let name = format!("{hash}.png");
        let path = self.dir.join("images").join(&name);
        img.save_with_format(&path, image::ImageFormat::Png)?;
        let thumb = image::DynamicImage::ImageRgba8(img).thumbnail(THUMB_MAX, THUMB_MAX);
        thumb.save_with_format(
            self.dir.join("images").join(format!("{hash}_thumb.png")),
            image::ImageFormat::Png,
        )?;
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        self.conn.execute(
            "INSERT INTO clips (kind, hash, content, preview, search, size, width, height, created_at)
             VALUES ('image', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                hash,
                name,
                format!("Image {width}×{height}"),
                format!("image screenshot picture {width}x{height}"),
                size as i64,
                width,
                height,
                now_ms()
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.prune()?;
        Ok(Some(id))
    }

    /// Record copied files/folders. Small enough sets are copied into the vault
    /// so they survive the originals being moved or deleted.
    pub fn add_files(&self, paths: &[PathBuf]) -> Result<Option<i64>> {
        let paths: Vec<&PathBuf> = paths.iter().filter(|p| p.exists()).collect();
        if paths.is_empty() {
            return Ok(None);
        }
        let mut sig: Vec<Vec<u8>> = vec![b"files".to_vec()];
        for p in &paths {
            sig.push(p.to_string_lossy().as_bytes().to_vec());
            let meta = fs::metadata(p)?;
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            sig.push(format!("{}:{}", meta.len(), mtime).into_bytes());
        }
        let sig_refs: Vec<&[u8]> = sig.iter().map(|v| v.as_slice()).collect();
        let hash = hash_parts(&sig_refs);
        if let Some(id) = self.bump(&hash)? {
            return Ok(Some(id));
        }

        let total: u64 = paths.iter().map(|p| tree_size(p, 0)).sum();
        let copy = total <= self.max_copy_bytes;
        let vault = self.dir.join("files").join(&hash);
        let mut entries = Vec::new();
        for (i, p) in paths.iter().enumerate() {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string_lossy().to_string());
            let is_dir = p.is_dir();
            let mut stored = None;
            if copy {
                let dest_dir = vault.join(i.to_string());
                fs::create_dir_all(&dest_dir)?;
                let dest = dest_dir.join(&name);
                if copy_tree(p, &dest, 0).is_ok() {
                    stored = Some(dest.to_string_lossy().to_string());
                }
            }
            entries.push(FileEntry {
                name,
                original: p.to_string_lossy().to_string(),
                stored,
                size: tree_size(p, 0),
                is_dir,
            });
        }
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        let preview = if entries.len() == 1 {
            entries[0].name.clone()
        } else {
            format!(
                "{} files: {}",
                entries.len(),
                one_line(&names.join(", "), 120)
            )
        };
        let search = format!(
            "{} {}",
            names.join(" "),
            entries
                .iter()
                .map(|e| e.original.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        )
        .to_lowercase();
        self.conn.execute(
            "INSERT INTO clips (kind, hash, content, preview, search, size, created_at)
             VALUES ('files', ?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                hash,
                serde_json::to_string(&entries)?,
                preview,
                search,
                total as i64,
                now_ms()
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.prune()?;
        Ok(Some(id))
    }

    // ---------- reading ----------

    fn row_to_clip(r: &rusqlite::Row) -> rusqlite::Result<Clip> {
        let kind = Kind::parse(&r.get::<_, String>("kind")?);
        let content: String = r.get("content")?;
        let files = if kind == Kind::Files {
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(Clip {
            id: r.get("id")?,
            kind,
            content: if kind == Kind::Files {
                String::new()
            } else {
                content
            },
            preview: r.get("preview")?,
            size: r.get::<_, i64>("size")? as u64,
            pinned: r.get::<_, i64>("pinned")? != 0,
            created_at: r.get("created_at")?,
            hash: r.get("hash")?,
            files,
            width: r.get::<_, i64>("width")? as u32,
            height: r.get::<_, i64>("height")? as u32,
            source_app: r.get("source_app")?,
        })
    }

    /// List clips, pinned first then newest first. Text content is truncated to
    /// keep the list cheap; use `get` for the full text.
    pub fn list(&self, query: &str, filter: Filter, limit: usize) -> Result<Vec<Clip>> {
        let mut sql = String::from(
            "SELECT id, kind, hash, CASE WHEN kind='text' THEN substr(content,1,2000) ELSE content END AS content,
                    preview, size, width, height, pinned, created_at, source_app
             FROM clips WHERE 1=1",
        );
        match filter {
            Filter::All => {}
            Filter::Text => sql.push_str(" AND kind='text'"),
            Filter::Images => sql.push_str(" AND kind='image'"),
            Filter::Files => sql.push_str(" AND kind='files'"),
            Filter::Pinned => sql.push_str(" AND pinned=1"),
        }
        // Every word must match (in any order), in the text or the source app.
        let q = query.to_lowercase();
        let pats: Vec<String> = q
            .split_whitespace()
            .map(|w| {
                format!(
                    "%{}%",
                    w.replace('\\', "\\\\")
                        .replace('%', "\\%")
                        .replace('_', "\\_")
                )
            })
            .collect();
        for n in 1..=pats.len() {
            sql.push_str(&format!(
                " AND (search LIKE ?{n} ESCAPE '\\' OR lower(source_app) LIKE ?{n} ESCAPE '\\')"
            ));
        }
        sql.push_str(" ORDER BY pinned DESC, created_at DESC LIMIT ");
        sql.push_str(&limit.to_string());
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(&pats), Self::row_to_clip)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn full_text(&self, id: i64) -> Result<String> {
        Ok(self.conn.query_row(
            "SELECT content FROM clips WHERE id=?1 AND kind='text'",
            [id],
            |r| r.get(0),
        )?)
    }

    pub fn get(&self, id: i64) -> Result<Option<Clip>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, kind, hash, content, preview, size, width, height, pinned, created_at, source_app
                 FROM clips WHERE id=?1",
                [id],
                Self::row_to_clip,
            )
            .optional()?)
    }

    /// Number of clips per filter, in `Filter` order: All, Text, Images, Files, Pinned.
    pub fn counts(&self) -> [usize; 5] {
        self.conn
            .query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(kind='text'), 0),
                        COALESCE(SUM(kind='image'), 0),
                        COALESCE(SUM(kind='files'), 0),
                        COALESCE(SUM(pinned), 0)
                 FROM clips",
                [],
                |r| {
                    let mut out = [0usize; 5];
                    for (i, o) in out.iter_mut().enumerate() {
                        *o = r.get::<_, i64>(i)? as usize;
                    }
                    Ok(out)
                },
            )
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub fn count(&self) -> usize {
        self.conn
            .query_row("SELECT COUNT(*) FROM clips", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    // ---------- changing ----------

    pub fn toggle_pin(&self, id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE clips SET pinned = 1 - pinned WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn set_source(&self, id: i64, app: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE clips SET source_app=?1 WHERE id=?2",
            params![app, id],
        )?;
        Ok(())
    }

    /// Move a clip to the top (used after the user pastes it).
    pub fn touch(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE clips SET created_at=?1 WHERE id=?2",
            params![now_ms(), id],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        if let Some(clip) = self.get(id)? {
            self.remove_blobs(&clip);
        }
        self.conn.execute("DELETE FROM clips WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn clear_unpinned(&self) -> Result<()> {
        let ids: Vec<i64> = {
            let mut stmt = self.conn.prepare("SELECT id FROM clips WHERE pinned=0")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for id in ids {
            self.delete(id)?;
        }
        Ok(())
    }

    /// Drop the oldest unpinned clips beyond the configured limit.
    fn prune(&self) -> Result<()> {
        let excess: Vec<i64> = {
            let mut stmt = self.conn.prepare(
                "SELECT id FROM clips WHERE pinned=0 ORDER BY created_at DESC LIMIT -1 OFFSET ?1",
            )?;
            let rows = stmt.query_map([self.max_items as i64], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for id in excess {
            self.delete(id)?;
        }
        Ok(())
    }

    fn remove_blobs(&self, clip: &Clip) {
        match clip.kind {
            Kind::Image => {
                let _ = fs::remove_file(self.image_path(clip));
                let _ = fs::remove_file(self.thumb_path(clip));
            }
            Kind::Files => {
                let _ = fs::remove_dir_all(self.dir.join("files").join(&clip.hash));
            }
            Kind::Text => {}
        }
    }
}

// ---------- filesystem helpers ----------

const MAX_DEPTH: usize = 12;

fn tree_size(p: &Path, depth: usize) -> u64 {
    let Ok(meta) = fs::symlink_metadata(p) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if meta.is_dir() && depth < MAX_DEPTH {
        if let Ok(rd) = fs::read_dir(p) {
            return rd.flatten().map(|e| tree_size(&e.path(), depth + 1)).sum();
        }
    }
    0
}

fn copy_tree(src: &Path, dest: &Path, depth: usize) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    if meta.is_file() {
        fs::copy(src, dest)?;
    } else if meta.is_dir() && depth < MAX_DEPTH {
        fs::create_dir_all(dest)?;
        for e in fs::read_dir(src)?.flatten() {
            copy_tree(&e.path(), &dest.join(e.file_name()), depth + 1)?;
        }
    }
    // symlinks are skipped on purpose
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("clipvault-test-{name}-{}", now_ms()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn text_dedupe_search_pin_prune() {
        let dir = tmp("text");
        let s = Store::open(&dir, 3, 10).unwrap();
        let a = s.add_text("hello world").unwrap().unwrap();
        let b = s.add_text("second").unwrap().unwrap();
        assert_ne!(a, b);
        // duplicate bumps instead of inserting
        assert_eq!(s.add_text("hello world").unwrap(), Some(a));
        assert_eq!(s.count(), 2);
        assert_eq!(s.list("", Filter::All, 10).unwrap()[0].id, a);
        assert_eq!(s.list("WORLD", Filter::All, 10).unwrap().len(), 1);
        assert!(s.add_text("   \n").unwrap().is_none());
        // 100% must be searched literally, not as a wildcard
        s.add_text("save 100% now").unwrap();
        assert_eq!(s.list("100%", Filter::All, 10).unwrap().len(), 1);
        assert_eq!(s.list("%", Filter::All, 10).unwrap().len(), 1);
        // pinned items survive pruning (max 3 unpinned)
        s.toggle_pin(b).unwrap();
        for i in 0..6 {
            s.add_text(&format!("item {i}")).unwrap();
        }
        assert!(s.get(b).unwrap().unwrap().pinned);
        assert_eq!(s.list("", Filter::All, 50).unwrap().len(), 4);
        assert_eq!(s.list("", Filter::Pinned, 50).unwrap().len(), 1);
        assert_eq!(s.counts(), [4, 4, 0, 0, 1]);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn multi_word_search_and_source_app() {
        let dir = tmp("search");
        let s = Store::open(&dir, 10, 10).unwrap();
        let a = s.add_text("the quick brown fox").unwrap().unwrap();
        s.add_text("quick lunch").unwrap();
        s.set_source(a, "Safari").unwrap();
        assert_eq!(s.list("fox quick", Filter::All, 10).unwrap().len(), 1);
        assert_eq!(s.list("  quick  ", Filter::All, 10).unwrap().len(), 2);
        // the source app is searchable and shown
        let hits = s.list("safari quick", Filter::All, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source_app, "Safari");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn old_database_gets_source_column() {
        let dir = tmp("migrate");
        let conn = Connection::open(dir.join("history.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE clips (
                id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL,
                hash TEXT NOT NULL UNIQUE, content TEXT NOT NULL, preview TEXT NOT NULL,
                search TEXT NOT NULL, size INTEGER NOT NULL DEFAULT 0,
                width INTEGER NOT NULL DEFAULT 0, height INTEGER NOT NULL DEFAULT 0,
                pinned INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL);
             INSERT INTO clips (kind, hash, content, preview, search, created_at)
                VALUES ('text', 'h', 'old clip', 'old clip', 'old clip', 1);",
        )
        .unwrap();
        drop(conn);
        let s = Store::open(&dir, 10, 10).unwrap();
        let clips = s.list("old", Filter::All, 10).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].source_app, "");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn image_roundtrip_and_cleanup() {
        let dir = tmp("img");
        let s = Store::open(&dir, 10, 10).unwrap();
        let rgba = vec![200u8; 4 * 4 * 4];
        let id = s.add_image(4, 4, &rgba).unwrap().unwrap();
        let clip = s.get(id).unwrap().unwrap();
        assert!(s.image_path(&clip).exists());
        assert!(s.thumb_path(&clip).exists());
        s.delete(id).unwrap();
        assert!(!s.image_path(&clip).exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn files_are_copied_into_vault() {
        let dir = tmp("files");
        let src = tmp("src");
        fs::write(src.join("a.txt"), "data").unwrap();
        fs::create_dir_all(src.join("folder")).unwrap();
        fs::write(src.join("folder/b.txt"), "more").unwrap();
        let s = Store::open(&dir, 10, 10).unwrap();
        let id = s
            .add_files(&[src.join("a.txt"), src.join("folder")])
            .unwrap()
            .unwrap();
        let clip = s.get(id).unwrap().unwrap();
        assert_eq!(clip.files.len(), 2);
        // original goes away; the vault copy is still usable
        fs::remove_dir_all(&src).unwrap();
        let p = clip.files[0].usable_path().unwrap();
        assert_eq!(fs::read_to_string(p).unwrap(), "data");
        assert!(clip.files[1].usable_path().unwrap().join("b.txt").exists());
        s.delete(id).unwrap();
        assert!(!dir.join("files").join(&clip.hash).exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn big_files_are_linked_not_copied() {
        let dir = tmp("big");
        let src = tmp("bigsrc");
        fs::write(src.join("x.bin"), vec![0u8; 2 * 1024 * 1024]).unwrap();
        let s = Store::open(&dir, 10, 1).unwrap(); // 1 MB limit
        let id = s.add_files(&[src.join("x.bin")]).unwrap().unwrap();
        let clip = s.get(id).unwrap().unwrap();
        assert!(clip.files[0].stored.is_none());
        assert!(clip.files[0].usable_path().is_some());
        fs::remove_dir_all(dir).ok();
        fs::remove_dir_all(src).ok();
    }
}
