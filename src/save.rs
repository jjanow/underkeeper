//! Typed access to an Underkeep save file (`save_1`, `save_auto`, `save_quick`, ...).
//!
//! Edits go straight into the DfT tree, so fields the editor doesn't know
//! about are carried through untouched.

use crate::dft::{Doc, Entry, Key, Value};
use std::path::{Path, PathBuf};

pub struct SaveFile {
    pub path: PathBuf,
    pub doc: Doc,
    pub original: Vec<u8>,
    pub modified: std::time::SystemTime,
}

/// Summary shown in the save list without keeping every file open.
#[derive(Clone, Debug)]
pub struct SaveSummary {
    pub path: PathBuf,
    pub file_name: String,
    pub modified: std::time::SystemTime,
    pub play_time_min: i32,
    pub area: String,
    pub heroes: Vec<(String, String, i32)>,
    pub error: Option<String>,
}

pub fn is_save_name(name: &str) -> bool {
    name.starts_with("save_") && !name.contains('.') // skip temp/backup files
}

pub fn list_saves(dir: &Path) -> Vec<SaveSummary> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !is_save_name(&name) || !e.path().is_file() {
            continue;
        }
        let modified = e
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        let mut s = SaveSummary {
            path: e.path(),
            file_name: name,
            modified,
            play_time_min: 0,
            area: String::new(),
            heroes: Vec::new(),
            error: None,
        };
        match SaveFile::open(&e.path()) {
            Ok(f) => {
                s.play_time_min = f.root_int("play_time").unwrap_or(0);
                s.area = f
                    .root_str("current_level")
                    .unwrap_or_default()
                    .replace('_', " ");
                for h in 0..f.hero_count() {
                    s.heroes.push((
                        f.hero_str(h, "name").unwrap_or_default(),
                        f.hero_str(h, "class").unwrap_or_default(),
                        f.hero_int(h, "level").unwrap_or(0),
                    ));
                }
            }
            Err(err) => s.error = Some(err),
        }
        out.push(s);
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    out
}

fn pos_of(doc: &Doc, t: &[Entry], name: &str) -> Option<usize> {
    let idx = doc.find_str(name)?;
    t.iter().position(|e| e.key == Key::Name(idx))
}

fn table(e: &Entry) -> Option<&Vec<Entry>> {
    match &e.value {
        Value::Table(t) => Some(t),
        _ => None,
    }
}

fn table_mut(e: &mut Entry) -> Option<&mut Vec<Entry>> {
    match &mut e.value {
        Value::Table(t) => Some(t),
        _ => None,
    }
}

/// Follow a list of entry positions down from `root`.
fn walk_mut<'a>(root: &'a mut Vec<Entry>, pos: &[usize]) -> Option<&'a mut Vec<Entry>> {
    let mut cur = root;
    for &p in pos {
        cur = table_mut(cur.get_mut(p)?)?;
    }
    Some(cur)
}

fn walk<'a>(root: &'a [Entry], pos: &[usize]) -> Option<&'a Vec<Entry>> {
    let mut cur: Option<&Vec<Entry>> = None;
    let mut slice = root;
    for &p in pos {
        let t = table(slice.get(p)?)?;
        cur = Some(t);
        slice = t;
    }
    cur
}

impl SaveFile {
    pub fn open(path: &Path) -> Result<SaveFile, String> {
        let original =
            std::fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
        let doc = Doc::parse(&original).map_err(|e| format!("{}: {e}", path.display()))?;
        let modified = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        Ok(SaveFile {
            path: path.to_path_buf(),
            doc,
            original,
            modified,
        })
    }

    pub fn is_dirty(&self) -> bool {
        self.doc.to_bytes() != self.original
    }

    // ---- root ----

    pub fn root_int(&self, name: &str) -> Option<i32> {
        match &self
            .doc
            .root
            .get(pos_of(&self.doc, &self.doc.root, name)?)?
            .value
        {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn root_str(&self, name: &str) -> Option<String> {
        match &self
            .doc
            .root
            .get(pos_of(&self.doc, &self.doc.root, name)?)?
            .value
        {
            Value::Str(s) => Some(self.doc.str(*s)),
            _ => None,
        }
    }

    // ---- party ----

    fn party_pos(&self) -> Option<Vec<usize>> {
        Some(vec![pos_of(&self.doc, &self.doc.root, "party")?])
    }

    pub fn party_int(&self, name: &str) -> Option<i32> {
        let pp = self.party_pos()?;
        get_int(&self.doc, walk(&self.doc.root, &pp)?, name)
    }

    pub fn set_party_int(&mut self, name: &str, v: i32) {
        if let Some(pp) = self.party_pos() {
            set_int(&mut self.doc, &pp, name, v);
        }
    }

    // ---- heroes ----

    fn heroes_pos(&self) -> Option<Vec<usize>> {
        let mut p = self.party_pos()?;
        let party = walk(&self.doc.root, &p)?;
        p.push(pos_of(&self.doc, party, "heroes")?);
        Some(p)
    }

    pub fn hero_pos(&self, h: usize) -> Option<Vec<usize>> {
        let mut p = self.heroes_pos()?;
        if h >= walk(&self.doc.root, &p)?.len() {
            return None;
        }
        p.push(h);
        Some(p)
    }

    pub fn hero_count(&self) -> usize {
        self.heroes_pos()
            .and_then(|p| walk(&self.doc.root, &p).map(|t| t.len()))
            .unwrap_or(0)
    }

    pub fn hero_table(&self, h: usize) -> Option<&Vec<Entry>> {
        walk(&self.doc.root, &self.hero_pos(h)?)
    }

    pub fn hero_int(&self, h: usize, name: &str) -> Option<i32> {
        get_int(&self.doc, self.hero_table(h)?, name)
    }

    pub fn hero_str(&self, h: usize, name: &str) -> Option<String> {
        get_str(&self.doc, self.hero_table(h)?, name)
    }

    pub fn set_hero_int(&mut self, h: usize, name: &str, v: i32) {
        if let Some(p) = self.hero_pos(h) {
            set_int(&mut self.doc, &p, name, v);
        }
    }

    pub fn set_hero_str(&mut self, h: usize, name: &str, v: &str) {
        if let Some(p) = self.hero_pos(h) {
            set_str(&mut self.doc, &p, name, v);
        }
    }

    /// Position path of a list field (e.g. "skill_states") inside a hero.
    fn hero_list_pos(&self, h: usize, list: &str) -> Option<Vec<usize>> {
        let mut p = self.hero_pos(h)?;
        p.push(pos_of(&self.doc, walk(&self.doc.root, &p)?, list)?);
        Some(p)
    }

    /// (skill id, level) for every skill the hero has an entry for.
    pub fn skills(&self, h: usize) -> Vec<(String, i32)> {
        let Some(p) = self.hero_list_pos(h, "skill_states") else {
            return Vec::new();
        };
        let Some(list) = walk(&self.doc.root, &p) else {
            return Vec::new();
        };
        list.iter()
            .filter_map(|e| {
                let t = table(e)?;
                Some((
                    get_str(&self.doc, t, "skill_id")?,
                    get_int(&self.doc, t, "skill_level").unwrap_or(0),
                ))
            })
            .collect()
    }

    pub fn set_skill_level(&mut self, h: usize, skill: &str, level: i32) {
        let Some(mut p) = self.hero_list_pos(h, "skill_states") else {
            return;
        };
        let Some(list) = walk(&self.doc.root, &p) else {
            return;
        };
        let found = list.iter().position(|e| {
            table(e)
                .and_then(|t| get_str(&self.doc, t, "skill_id"))
                .as_deref()
                == Some(skill)
        });
        match found {
            Some(i) => {
                p.push(i);
                set_int(&mut self.doc, &p, "skill_level", level);
            }
            None => {
                let entry = vec![
                    new_str_entry(&mut self.doc, "skill_id", skill),
                    new_int_entry(&mut self.doc, "skill_level", level),
                    new_int_entry(&mut self.doc, "skill_cooldown", 0),
                ];
                push_list(&mut self.doc, &p, entry);
            }
        }
    }

    /// Replace the hero's skill list with `skills` (keeping levels of skills
    /// that are kept). Used when changing class.
    pub fn reset_skills(&mut self, h: usize, skills: &[String]) {
        let current: Vec<(String, i32)> = self.skills(h);
        let Some(p) = self.hero_list_pos(h, "skill_states") else {
            return;
        };
        if let Some(list) = walk_mut(&mut self.doc.root, &p) {
            list.clear();
        }
        for s in skills {
            let lvl = current
                .iter()
                .find(|(id, _)| id == s)
                .map(|(_, l)| *l)
                .unwrap_or(0);
            self.set_skill_level(h, s, lvl);
        }
    }

    /// All status effects of the hero whose type starts with "trait_".
    pub fn traits(&self, h: usize) -> Vec<String> {
        let Some(p) = self.hero_list_pos(h, "status_effects") else {
            return Vec::new();
        };
        let Some(list) = walk(&self.doc.root, &p) else {
            return Vec::new();
        };
        list.iter()
            .filter_map(|e| get_str(&self.doc, table(e)?, "type"))
            .filter(|t| t.starts_with("trait_"))
            .collect()
    }

    pub fn remove_trait(&mut self, h: usize, name: &str) {
        let Some(p) = self.hero_list_pos(h, "status_effects") else {
            return;
        };
        let Some(list) = walk(&self.doc.root, &p) else {
            return;
        };
        let keep: Vec<bool> = list
            .iter()
            .map(|e| {
                table(e)
                    .and_then(|t| get_str(&self.doc, t, "type"))
                    .as_deref()
                    != Some(name)
            })
            .collect();
        if let Some(list) = walk_mut(&mut self.doc.root, &p) {
            let mut it = keep.iter();
            list.retain(|_| *it.next().unwrap());
            renumber(list);
        }
    }

    pub fn add_trait(&mut self, h: usize, name: &str) {
        if self.traits(h).iter().any(|t| t == name) {
            return;
        }
        let Some(p) = self.hero_list_pos(h, "status_effects").or_else(|| {
            let hp = self.hero_pos(h)?;
            let k = self.doc.intern("status_effects");
            walk_mut(&mut self.doc.root, &hp)?
                .push(Entry::new(Key::Name(k), Value::Table(Vec::new())));
            self.hero_list_pos(h, "status_effects")
        }) else {
            return;
        };
        let entry = vec![
            new_str_entry(&mut self.doc, "type", name),
            new_int_entry(&mut self.doc, "duration", -1),
            new_int_entry(&mut self.doc, "power", 0),
            new_int_entry(&mut self.doc, "level", 1),
        ];
        push_list(&mut self.doc, &p, entry);
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.doc.to_bytes()
    }
}

// ---- generic table helpers ----

pub fn get_int(doc: &Doc, t: &[Entry], name: &str) -> Option<i32> {
    match t.get(pos_of(doc, t, name)?)?.value {
        Value::Int(i) => Some(i),
        Value::Float(b) => Some(f32::from_bits(b) as i32),
        _ => None,
    }
}

pub fn get_str(doc: &Doc, t: &[Entry], name: &str) -> Option<String> {
    match t.get(pos_of(doc, t, name)?)?.value {
        Value::Str(s) => Some(doc.str(s)),
        _ => None,
    }
}

fn set_int(doc: &mut Doc, table_pos: &[usize], name: &str, v: i32) {
    let k = doc.intern(name);
    let Some(t) = walk_mut(&mut doc.root, table_pos) else {
        return;
    };
    match t.iter_mut().find(|e| e.key == Key::Name(k)) {
        Some(e) => e.value = Value::Int(v),
        None => t.push(Entry::new(Key::Name(k), Value::Int(v))),
    }
}

fn set_str(doc: &mut Doc, table_pos: &[usize], name: &str, v: &str) {
    let k = doc.intern(name);
    let s = doc.intern(v);
    let Some(t) = walk_mut(&mut doc.root, table_pos) else {
        return;
    };
    match t.iter_mut().find(|e| e.key == Key::Name(k)) {
        Some(e) => {
            if e.value != Value::Str(s) {
                e.value = Value::Str(s);
                e.val_w = 1;
            }
        }
        None => t.push(Entry::new(Key::Name(k), Value::Str(s))),
    }
}

fn new_int_entry(doc: &mut Doc, name: &str, v: i32) -> Entry {
    Entry::new(Key::Name(doc.intern(name)), Value::Int(v))
}

fn new_str_entry(doc: &mut Doc, name: &str, v: &str) -> Entry {
    let k = doc.intern(name);
    let s = doc.intern(v);
    Entry::new(Key::Name(k), Value::Str(s))
}

fn push_list(doc: &mut Doc, list_pos: &[usize], item: Vec<Entry>) {
    if let Some(list) = walk_mut(&mut doc.root, list_pos) {
        let i = list.len() as u32;
        list.push(Entry::new(Key::Index(i), Value::Table(item)));
    }
}

/// Re-number array indices after removing an element.
fn renumber(list: &mut [Entry]) {
    for (i, e) in list.iter_mut().enumerate() {
        if let Key::Index(_) = e.key
            && e.key != Key::Index(i as u32)
        {
            e.key = Key::Index(i as u32);
            e.key_w = 1;
        }
    }
}

/// Write `data` to `path` safely: back up the current file into
/// `backup_dir`, write a temp file next to the target, then rename it over.
pub fn write_with_backup(path: &Path, data: &[u8], backup_dir: &Path) -> Result<PathBuf, String> {
    // Make sure what we write is readable by our own parser first.
    Doc::parse(data).map_err(|e| format!("refusing to write, output doesn't re-parse: {e}"))?;

    std::fs::create_dir_all(backup_dir).map_err(|e| format!("can't create backup folder: {e}"))?;
    let stamp = chrono_like_stamp();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let backup = backup_dir.join(format!("{name}.{stamp}.bak"));
    std::fs::copy(path, &backup).map_err(|e| format!("backup failed, nothing written: {e}"))?;

    let tmp = path.with_file_name(format!("{name}.underkeeper-tmp"));
    std::fs::write(&tmp, data).map_err(|e| format!("write failed: {e}"))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("couldn't replace save: {e}"));
    }
    Ok(backup)
}

/// YYYYMMDD-HHMMSS in UTC without pulling in a date crate.
pub fn chrono_like_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Civil-from-days (Howard Hinnant).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs against real saves in $UNDERKEEP_SAVES (skipped if unset):
    /// unchanged round-trip, then a set of edits that must survive re-parsing.
    #[test]
    fn edit_real_saves() {
        let Ok(dir) = std::env::var("UNDERKEEP_SAVES") else {
            return;
        };
        for s in list_saves(Path::new(&dir)) {
            let mut f = SaveFile::open(&s.path).unwrap();
            assert_eq!(f.to_bytes(), f.original, "{} round-trip", s.file_name);
            assert!(!f.is_dirty());
            for h in 0..f.hero_count() {
                f.set_hero_int(h, "base_wis", 25);
                f.set_hero_int(h, "level", 5);
                f.set_hero_str(h, "name", "Edited Name");
                let skills = f.skills(h);
                if let Some((id, _)) = skills.first() {
                    f.set_skill_level(h, id, 3);
                }
                f.add_trait(h, "trait_more_exp");
                if let Some(t) = f.traits(h).first().cloned() {
                    f.remove_trait(h, &t);
                }
            }
            f.set_party_int("coins", 12345);
            let bytes = f.to_bytes();
            let g = SaveFile {
                doc: Doc::parse(&bytes).unwrap(),
                ..SaveFile::open(&s.path).unwrap()
            };
            for h in 0..g.hero_count() {
                assert_eq!(g.hero_int(h, "base_wis"), Some(25));
                assert_eq!(g.hero_int(h, "level"), Some(5));
                assert_eq!(g.hero_str(h, "name").as_deref(), Some("Edited Name"));
                assert!(
                    g.traits(h).contains(&"trait_more_exp".to_string()) || g.traits(h).is_empty()
                );
                if let Some((_, l)) = g.skills(h).first() {
                    assert_eq!(*l, 3);
                }
                // Array indices stay contiguous.
                let p = g.hero_list_pos(h, "status_effects").unwrap();
                let list = walk(&g.doc.root, &p).unwrap();
                for (i, e) in list.iter().enumerate() {
                    assert_eq!(e.key, Key::Index(i as u32));
                }
            }
            assert_eq!(g.party_int("coins"), Some(12345));
            eprintln!("{}: ok ({} heroes)", s.file_name, g.hero_count());
        }
    }
}
