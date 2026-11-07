//! Read-only game knowledge (classes, races, skills, traits, item
//! requirements, display names), loaded at runtime from the player's own
//! Underkeep install. Nothing from the game is bundled with this tool.

use crate::dft::{Doc, Entry, Key, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// A simplified, owned view of a DfT tree, convenient for read-only lookups.
#[derive(Clone, Debug)]
pub enum Node {
    Int(i32),
    Float(f32),
    Str(String),
    Map(Vec<(String, Node)>),
    List(Vec<Node>),
}

impl Node {
    pub fn from_doc(doc: &Doc) -> Node {
        Self::from_table(doc, &doc.root)
    }

    fn from_table(doc: &Doc, t: &[Entry]) -> Node {
        let is_list = !t.is_empty() && t.iter().all(|e| matches!(e.key, Key::Index(_)));
        let conv = |v: &Value| match v {
            Value::Int(i) => Node::Int(*i),
            Value::Float(b) => Node::Float(f32::from_bits(*b)),
            Value::Str(s) => Node::Str(doc.str(*s)),
            Value::Table(t) => Self::from_table(doc, t),
        };
        if is_list {
            Node::List(t.iter().map(|e| conv(&e.value)).collect())
        } else {
            Node::Map(
                t.iter()
                    .map(|e| (doc.key_name(e.key), conv(&e.value)))
                    .collect(),
            )
        }
    }

    pub fn get(&self, k: &str) -> Option<&Node> {
        match self {
            Node::Map(m) => m.iter().find(|(n, _)| n == k).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn int(&self) -> Option<i32> {
        match self {
            Node::Int(i) => Some(*i),
            Node::Float(f) => Some(*f as i32),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Node::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn entries(&self) -> &[(String, Node)] {
        match self {
            Node::Map(m) => m,
            _ => &[],
        }
    }
    pub fn list(&self) -> &[Node] {
        match self {
            Node::List(l) => l,
            _ => &[],
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SkillInfo {
    pub id: String,
    pub class: String,
    pub max_level: i32,
    pub req_class_level: i32,
    pub is_passive: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ItemReq {
    pub id: String,
    pub name: String,
    pub req_str: i32,
    pub req_dex: i32,
    pub req_end: i32,
    pub req_wis: i32,
    pub req_class: String,
    pub req_class_level: i32,
}

#[derive(Clone, Debug)]
pub struct GameData {
    pub classes: Vec<String>,
    pub races: Vec<String>,
    pub skills: Vec<SkillInfo>,
    pub traits: Vec<String>,
    pub items: Vec<ItemReq>,
    /// Total experience needed to reach level index+1.
    pub exp_levels: Vec<i32>,
    pub text: HashMap<String, String>,
    /// Class -> (attribute field, starting minimum), e.g. paladin -> ("base_str", 7).
    pub class_main: HashMap<String, (String, i32)>,
    /// `starting_equipment` rows: ("class race skill" selector, item spec).
    pub starting_equipment: Vec<(String, String)>,
    /// Named item pools (`drop_lists`), used by specs like `some_bones:3`.
    pub drop_lists: HashMap<String, Vec<String>>,
    /// Items that stack in the inventory.
    pub stackable: HashSet<String>,
}

impl Default for GameData {
    /// Fallback when the game install can't be found: enough to edit a save,
    /// without names, skill caps or item requirements.
    fn default() -> Self {
        GameData {
            classes: [
                "warrior",
                "paladin",
                "thief",
                "archer",
                "wizard",
                "voidcaller",
                "bloodmage",
            ]
            .map(String::from)
            .to_vec(),
            races: ["human", "dwarf", "elk", "horemount", "raven", "slugman"]
                .map(String::from)
                .to_vec(),
            skills: Vec::new(),
            traits: Vec::new(),
            items: Vec::new(),
            exp_levels: Vec::new(),
            text: HashMap::new(),
            class_main: HashMap::new(),
            starting_equipment: Vec::new(),
            drop_lists: HashMap::new(),
            stackable: HashSet::new(),
        }
    }
}

fn load(path: &Path) -> Option<Node> {
    let b = std::fs::read(path).ok()?;
    Doc::parse(&b).ok().map(|d| Node::from_doc(&d))
}

/// "item_hat_of_wisdom" -> "Hat Of Wisdom"
pub fn prettify(id: &str) -> String {
    let trimmed = ["item_", "skill_", "trait_"]
        .iter()
        .find_map(|p| id.strip_prefix(p))
        .unwrap_or(id);
    trimmed
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl GameData {
    pub fn load(game_dir: &Path) -> Option<GameData> {
        let base = game_dir.join("base");
        let main = load(&base.join("def_def.txt.rfile"))?;
        let mut gd = GameData::default();

        // English display text.
        for f in ["common", "game", "items", "skills"] {
            if let Some(Node::Map(m)) = load(&base.join(format!("text_text_{f}_en.txt.rfile"))) {
                for (_, table) in m {
                    for (k, v) in table.entries() {
                        if let Some(s) = v.str() {
                            gd.text.insert(k.clone(), s.to_string());
                        }
                    }
                }
            }
        }

        if let Some(gp) = main.get("global_params") {
            if let Some(levels) = gp.get("hero_experience_levels") {
                gd.exp_levels = levels.list().iter().filter_map(Node::int).collect();
            }
            let mut classes = Vec::new();
            let mut races = Vec::new();
            for (k, v) in gp.entries() {
                if let Some(name) = k.strip_prefix("base_stats_") {
                    if v.get("hp_class").is_some() {
                        classes.push(name.to_string());
                        for a in ["str", "dex", "end", "wis"] {
                            if let Some(min) = v.get(a).and_then(|r| r.list().first()?.int()) {
                                gd.class_main
                                    .insert(name.to_string(), (format!("base_{a}"), min));
                            }
                        }
                    } else if v.get("stats_range").is_some() || v.get("str_mod").is_some() {
                        races.push(name.to_string());
                    }
                }
            }
            if !classes.is_empty() {
                gd.classes = classes;
            }
            for row in gp.get("starting_equipment").map(Node::list).unwrap_or(&[]) {
                let (Some(sel), Some(items)) = (row.list().first(), row.list().get(1)) else {
                    continue;
                };
                let Some(sel) = sel.str() else { continue };
                for it in items.list().iter().filter_map(Node::str) {
                    gd.starting_equipment
                        .push((sel.to_string(), it.to_string()));
                }
            }
            for (k, v) in gp.get("drop_lists").map(Node::entries).unwrap_or(&[]) {
                let l: Vec<String> = v
                    .list()
                    .iter()
                    .filter_map(|n| n.str().map(String::from))
                    .collect();
                if !l.is_empty() {
                    gd.drop_lists.insert(k.clone(), l);
                }
            }
            if !races.is_empty() {
                gd.races = races;
            }
        }

        if let Some(se) = main.get("status_effects") {
            gd.traits = se
                .entries()
                .iter()
                .map(|(k, _)| k.clone())
                .filter(|k| k.starts_with("trait_"))
                .collect();
        }

        if let Some(sk) = load(&base.join("def_def_skill.txt.rfile")) {
            for s in sk.get("skills").map(Node::list).unwrap_or(&[]) {
                let Some(id) = s.get("name").and_then(Node::str) else {
                    continue;
                };
                gd.skills.push(SkillInfo {
                    id: id.to_string(),
                    class: s.get("class").and_then(Node::str).unwrap_or("").to_string(),
                    max_level: s.get("max_level").and_then(Node::int).unwrap_or(5),
                    req_class_level: s.get("req_class_level").and_then(Node::int).unwrap_or(1),
                    is_passive: s.get("is_passive").and_then(Node::int).unwrap_or(0) != 0,
                });
            }
        }

        // Items: merge all item defs, then resolve requirements through the
        // `parent` chain (templates carry some of them).
        let mut defs: BTreeMap<String, Node> = BTreeMap::new();
        for f in [
            "def_def_item.txt.rfile",
            "def_def_item_weapon.txt.rfile",
            "def_def_item_armor.txt.rfile",
        ] {
            if let Some(Node::Map(m)) = load(&base.join(f)) {
                for (k, v) in m {
                    if matches!(v, Node::Map(_)) {
                        defs.insert(k, v);
                    }
                }
            }
        }
        let lookup = |id: &str, field: &str| -> Option<Node> {
            let mut cur = id.to_string();
            for _ in 0..16 {
                let d = defs.get(&cur)?;
                if let Some(v) = d.get(field) {
                    return Some(v.clone());
                }
                cur = d.get("parent")?.str()?.to_string();
            }
            None
        };
        for id in defs.keys() {
            if id.starts_with('_') || id.starts_with("template_") {
                continue;
            }
            let int = |f: &str| lookup(id, f).and_then(|n| n.int()).unwrap_or(0);
            if int("stackable") != 0 {
                gd.stackable.insert(id.clone());
            }
            let r = ItemReq {
                id: id.clone(),
                name: gd.display(id),
                req_str: int("req_str"),
                req_dex: int("req_dex"),
                req_end: int("req_end"),
                req_wis: int("req_wis"),
                req_class: lookup(id, "req_class")
                    .and_then(|n| n.str().map(String::from))
                    .unwrap_or_default(),
                req_class_level: int("req_class_level"),
            };
            if r.req_str + r.req_dex + r.req_end + r.req_wis + r.req_class_level > 0
                || !r.req_class.is_empty()
            {
                gd.items.push(r);
            }
        }
        gd.items.sort_by(|a, b| a.name.cmp(&b.name));
        Some(gd)
    }

    /// Display name for a game id, falling back to a prettified id.
    pub fn display(&self, id: &str) -> String {
        for k in [
            id.to_string(),
            format!("hero_class_{id}"),
            format!("hero_race_{id}"),
            format!("status_effect_{id}_active"),
            format!("status_effect_{id}"),
        ] {
            if let Some(t) = self.text.get(&k)
                && !t.trim().is_empty()
                && !t.contains('{')
            {
                return t.trim().to_string();
            }
        }
        prettify(id)
    }

    pub fn skill(&self, id: &str) -> Option<&SkillInfo> {
        self.skills.iter().find(|s| s.id == id)
    }

    /// The class's skills, in the game's order.
    pub fn class_skills(&self, class: &str) -> Vec<&SkillInfo> {
        self.skills.iter().filter(|s| s.class == class).collect()
    }

    /// Skills a new hero of `class` gets at level 1 without choosing them.
    /// Not marked in the game data: Stun Undead says "Starting skill" in its
    /// description, and fresh Voidcallers have all three auras.
    pub fn free_skills(&self, class: &str) -> Vec<String> {
        let ids: &[&str] = match class {
            "paladin" => &["skill_stun_undead"],
            "voidcaller" => &[
                "skill_aura_of_blades",
                "skill_aura_of_the_void",
                "skill_aura_of_thorns",
            ],
            _ => &[],
        };
        ids.iter()
            .filter(|id| self.skills.iter().any(|s| s.class == class && s.id == **id))
            .map(|id| id.to_string())
            .collect()
    }

    /// What the player picks on the "Choose Starting Skill" screen. For a
    /// Voidcaller, the choice is which of the free auras starts active.
    pub fn starter_choices(&self, class: &str) -> Vec<&SkillInfo> {
        let free = self.free_skills(class);
        self.class_skills(class)
            .into_iter()
            .filter(|s| {
                if class == "voidcaller" {
                    free.contains(&s.id)
                } else {
                    s.req_class_level <= 1 && !free.contains(&s.id)
                }
            })
            .collect()
    }

    /// Skill levels a brand-new hero of `class` has (free skills + starting skill).
    pub fn class_baseline(&self, class: &str) -> i32 {
        let free = self.free_skills(class).len() as i32;
        if class == "voidcaller" {
            free
        } else {
            free + 1
        }
    }

    /// Items the game hands a new hero of `class` who picked `starter`,
    /// as (item id, count). Race-only rows are left out; the race doesn't change.
    pub fn starter_kit(&self, class: &str, starter: &str) -> Vec<(String, i32)> {
        let mut out: Vec<(String, i32)> = Vec::new();
        for (sel, spec) in &self.starting_equipment {
            let t: Vec<&str> = sel.split_whitespace().collect();
            let any = |i: usize| t.get(i).is_none_or(|x| *x == "*");
            let class_row = t.first() == Some(&class) && any(1) && any(2);
            let skill_row = any(0) && any(1) && t.get(2) == Some(&starter);
            if !(class_row || skill_row) {
                continue;
            }
            for tok in spec.split_whitespace() {
                let tok = tok.split('%').next().unwrap_or(tok);
                let (id, n) = match tok.split_once(':') {
                    Some((id, n)) => {
                        let n = n
                            .rsplit('-')
                            .next()
                            .and_then(|x| x.parse().ok())
                            .unwrap_or(1);
                        (id, n)
                    }
                    None => (tok, 1),
                };
                let id = match self.drop_lists.get(id) {
                    Some(pool) => match pool
                        .iter()
                        .find(|p| p.starts_with("item_") && !p.contains([' ', ':', '%']))
                    {
                        Some(p) => p.as_str(),
                        None => continue,
                    },
                    None => id,
                };
                match out.iter_mut().find(|(i, _)| i == id) {
                    Some((_, c)) => *c += n,
                    None => out.push((id.to_string(), n)),
                }
            }
        }
        out
    }

    /// Experience needed for `level` (1-based), if known.
    pub fn exp_for_level(&self, level: i32) -> Option<i32> {
        self.exp_levels.get((level - 1).max(0) as usize).copied()
    }

    pub fn max_level(&self) -> i32 {
        if self.exp_levels.is_empty() {
            99
        } else {
            self.exp_levels.len() as i32
        }
    }
}
