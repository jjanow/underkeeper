//! Changing a hero's class, and everything that goes with it: skills,
//! skill points, the class's main attribute, class-locked gear, class-only
//! state (zeal, auras) and the new class's starting items.
//!
//! `plan` works out what will happen (shown to the user first), `apply`
//! does it.

use crate::gamedata::GameData;
use crate::save::SaveFile;

#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub new_class: String,
    pub starter: String,
    /// New skill list: (skill id, level).
    pub skills: Vec<(String, i32)>,
    pub skill_points_before: i32,
    pub skill_points_after: i32,
    /// (attribute field, old value, new value) if it gets raised.
    pub attr_raise: Option<(String, i32, i32)>,
    /// Equipped items the new class can't use: (slot, item id).
    pub unequip: Vec<(u32, String)>,
    /// Starting items to add, (item id, count). Coins are separate.
    pub kit: Vec<(String, i32)>,
    pub kit_coins: i32,
    /// No skill data (game install not found): only the class field changes.
    pub class_only: bool,
}

pub fn plan(
    s: &SaveFile,
    gd: &GameData,
    h: usize,
    new_class: &str,
    starter: &str,
    give_kit: bool,
) -> Plan {
    let old_class = s.hero_str(h, "class").unwrap_or_default();
    let mut p = Plan {
        new_class: new_class.to_string(),
        starter: starter.to_string(),
        skill_points_before: s.hero_int(h, "skill_points").unwrap_or(0),
        ..Plan::default()
    };
    p.skill_points_after = p.skill_points_before;
    if gd.class_skills(new_class).is_empty() {
        p.class_only = true;
        return p;
    }

    // Skills: refund everything earned beyond what a new hero starts with,
    // then set up the new class like the game's character creator does.
    let spent: i32 = s.skills(h).iter().map(|(_, l)| (*l).max(0)).sum();
    let refund = (spent - gd.class_baseline(&old_class)).max(0);
    p.skill_points_after = p.skill_points_before + refund;
    let free = gd.free_skills(new_class);
    p.skills = gd
        .class_skills(new_class)
        .iter()
        .map(|k| {
            let lvl = i32::from(free.contains(&k.id) || k.id == starter);
            (k.id.clone(), lvl)
        })
        .collect();

    // Main attribute up to the new class's starting minimum.
    if let Some((field, min)) = gd.class_main.get(new_class) {
        let v = s.hero_int(h, field).unwrap_or(0);
        if v < *min {
            p.attr_raise = Some((field.clone(), v, *min));
        }
    }

    // Gear locked to another class.
    p.unequip = s
        .equipped(h)
        .into_iter()
        .filter(|(_, id)| {
            gd.items
                .iter()
                .find(|i| &i.id == id)
                .is_some_and(|i| !i.req_class.is_empty() && i.req_class != new_class)
        })
        .collect();

    if give_kit {
        for (id, n) in gd.starter_kit(new_class, starter) {
            if id == "item_coins" {
                p.kit_coins += n;
            } else {
                p.kit.push((id, n));
            }
        }
    }
    p
}

pub fn apply(s: &mut SaveFile, gd: &GameData, h: usize, p: &Plan) {
    s.set_hero_str(h, "class", &p.new_class);
    if p.class_only {
        return;
    }

    s.set_skill_list(h, &p.skills);
    s.set_hero_int(h, "skill_points", p.skill_points_after);
    let active = gd
        .class_skills(&p.new_class)
        .into_iter()
        .find(|k| k.id == p.starter)
        .is_some_and(|k| !k.is_passive);
    if active {
        s.set_hero_str(h, "selected_skill", &p.starter);
    } else {
        s.remove_hero_key(h, "selected_skill");
    }
    s.set_hero_str(h, "selected_skill_slot", "none");

    // Class-only state.
    if p.new_class != "paladin" {
        s.set_hero_int(h, "zeals", 0);
        s.set_hero_int(h, "righteous_strike_active", 0);
    }
    if p.new_class == "voidcaller" {
        s.set_hero_int(h, "can_activate_aura", 0);
        s.set_hero_str(h, "active_aura", &p.starter);
        s.set_hero_int(h, "active_aura_duration", -1);
        s.set_hero_int(h, "active_aura_power", 0);
    } else {
        s.set_hero_int(h, "can_activate_aura", 1);
        for k in ["active_aura", "active_aura_duration", "active_aura_power"] {
            s.remove_hero_key(h, k);
        }
    }

    if let Some((field, _, to)) = &p.attr_raise {
        s.set_hero_int(h, field, *to);
    }
    for (slot, id) in &p.unequip {
        s.unequip_to_inventory(h, *slot, gd.stackable.contains(id));
    }
    for (id, n) in &p.kit {
        s.add_to_inventory(id, *n, gd.stackable.contains(id));
    }
    if p.kit_coins > 0 {
        let c = s.party_int("coins").unwrap_or(0);
        s.set_party_int("coins", c + p.kit_coins);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dft::Doc;
    use std::path::Path;

    /// Every hero in every save in $UNDERKEEP_SAVES becomes every class, using
    /// the game data from $UNDERKEEP_GAME (skipped unless both are set).
    #[test]
    fn change_class_real_saves() {
        let (Ok(dir), Ok(game)) = (
            std::env::var("UNDERKEEP_SAVES"),
            std::env::var("UNDERKEEP_GAME"),
        ) else {
            return;
        };
        let gd = GameData::load(Path::new(&game)).expect("game data");
        assert!(
            gd.class_main.len() >= gd.classes.len() - 1,
            "{:?}",
            gd.class_main
        );
        for sv in crate::save::list_saves(Path::new(&dir)) {
            let orig = SaveFile::open(&sv.path).unwrap();
            for h in 0..orig.hero_count() {
                for class in &gd.classes {
                    let mut s = SaveFile::open(&sv.path).unwrap();
                    let old_class = s.hero_str(h, "class").unwrap();
                    let starter = gd.starter_choices(class).first().map(|k| k.id.clone());
                    let Some(starter) = starter else { continue };
                    let p = plan(&s, &gd, h, class, &starter, true);
                    let inv_before = s.inventory().len();
                    let eq_before: Vec<u32> = s.equipped(h).iter().map(|e| e.0).collect();
                    apply(&mut s, &gd, h, &p);

                    let g = SaveFile {
                        doc: Doc::parse(&s.to_bytes()).unwrap(),
                        ..SaveFile::open(&sv.path).unwrap()
                    };
                    let what = format!("{} hero {h} {old_class}->{class}", sv.file_name);
                    assert_eq!(g.hero_str(h, "class").as_deref(), Some(class.as_str()));
                    let ids: Vec<String> = g.skills(h).into_iter().map(|x| x.0).collect();
                    let want: Vec<String> = gd
                        .class_skills(class)
                        .iter()
                        .map(|k| k.id.clone())
                        .collect();
                    assert_eq!(ids, want, "{what}");
                    // Earned levels are conserved as skill points.
                    let lv = |f: &SaveFile| f.skills(h).iter().map(|x| x.1).sum::<i32>();
                    let pts = |f: &SaveFile| f.hero_int(h, "skill_points").unwrap_or(0);
                    if old_class != *class {
                        assert_eq!(
                            lv(&g) + pts(&g) - gd.class_baseline(class),
                            (lv(&orig) + pts(&orig) - gd.class_baseline(&old_class))
                                .max(pts(&orig)),
                            "{what}"
                        );
                    }
                    // Equipment keys are slots: survivors keep theirs.
                    let eq_after: Vec<u32> = g.equipped(h).iter().map(|e| e.0).collect();
                    assert!(eq_after.iter().all(|k| eq_before.contains(k)), "{what}");
                    assert_eq!(eq_before.len() - eq_after.len(), p.unequip.len(), "{what}");
                    for (_, id) in g.equipped(h) {
                        let ok = gd
                            .items
                            .iter()
                            .find(|i| i.id == id)
                            .is_none_or(|i| i.req_class.is_empty() || i.req_class == *class);
                        assert!(ok, "{what}: {id} still equipped");
                    }
                    assert!(g.inventory().len() >= inv_before, "{what}");
                    if let Some((f, _, to)) = &p.attr_raise {
                        assert_eq!(g.hero_int(h, f), Some(*to));
                    }
                    if class == "voidcaller" {
                        assert_eq!(g.hero_str(h, "active_aura"), Some(starter.clone()));
                    } else {
                        assert_eq!(g.hero_str(h, "active_aura"), None, "{what}");
                    }
                }
            }
            eprintln!("{}: ok", sv.file_name);
        }
    }

    /// Earned skill levels come back as skill points.
    #[test]
    fn refund_real_saves() {
        let (Ok(dir), Ok(game)) = (
            std::env::var("UNDERKEEP_SAVES"),
            std::env::var("UNDERKEEP_GAME"),
        ) else {
            return;
        };
        let gd = GameData::load(Path::new(&game)).unwrap();
        let path = crate::save::list_saves(Path::new(&dir))[0].path.clone();
        let s0 = SaveFile::open(&path).unwrap();
        let find = |class: &str| {
            (0..s0.hero_count()).find(|h| s0.hero_str(*h, "class").as_deref() == Some(class))
        };
        // Paladin: stun 1, lay on hands 3, divine guard 3 = 7 spent, baseline 2.
        if let Some(h) = find("paladin") {
            let mut s = SaveFile::open(&path).unwrap();
            s.set_skill_level(h, "skill_lay_on_hands", 3);
            s.set_skill_level(h, "skill_divine_guard", 3);
            s.set_hero_int(h, "skill_points", 2);
            let p = plan(&s, &gd, h, "wizard", "skill_fire_spark", false);
            assert_eq!(p.skill_points_after, 2 + 5);
            apply(&mut s, &gd, h, &p);
            assert_eq!(s.hero_int(h, "skill_points"), Some(7));
            assert_eq!(
                s.hero_str(h, "selected_skill").as_deref(),
                Some("skill_fire_spark")
            );
            assert_eq!(s.hero_int(h, "zeals"), Some(0));
        }
        // Voidcaller: three auras, one raised to 4 = 6 spent, baseline 3.
        if let Some(h) = find("voidcaller") {
            let mut s = SaveFile::open(&path).unwrap();
            s.set_skill_level(h, "skill_aura_of_thorns", 4);
            s.set_hero_int(h, "skill_points", 1);
            let p = plan(&s, &gd, h, "paladin", "skill_divine_guard", true);
            assert_eq!(p.skill_points_after, 1 + 3);
            apply(&mut s, &gd, h, &p);
            assert_eq!(s.hero_str(h, "active_aura"), None);
            assert_eq!(s.hero_int(h, "can_activate_aura"), Some(1));
            // Divine Guard is passive, so no selected skill.
            assert_eq!(s.hero_str(h, "selected_skill"), None);
            let mut lv: Vec<(String, i32)> = s.skills(h).into_iter().filter(|x| x.1 > 0).collect();
            lv.sort();
            let want = [("skill_divine_guard", 1), ("skill_stun_undead", 1)];
            assert_eq!(lv, want.map(|(a, b)| (a.to_string(), b)).to_vec());
        }
    }

    #[test]
    fn starter_kits() {
        let Ok(game) = std::env::var("UNDERKEEP_GAME") else {
            return;
        };
        let gd = GameData::load(Path::new(&game)).unwrap();
        for c in &gd.classes {
            for k in gd.starter_choices(c) {
                eprintln!("{c} / {}: {:?}", k.id, gd.starter_kit(c, &k.id));
            }
        }
        assert_eq!(
            gd.starter_kit("bloodmage", "skill_blood_lance"),
            vec![("item_quarterstaff".into(), 1), ("item_bones01".into(), 3)]
        );
        assert!(
            gd.starter_kit("warrior", "skill_flurry_thrust")
                .contains(&("item_rusty_sword".into(), 1))
        );
    }
}
