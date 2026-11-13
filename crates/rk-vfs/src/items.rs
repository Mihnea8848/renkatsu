// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Item database built from the game's lists and every zipmod's lists, and
//! the lookup that maps the IDs stored in a card to those items.
//!
//! Zipmod items are addressed by (mod GUID, category, ID). Cards store the
//! mod's own ID plus a Sideloader record naming the GUID, so no synthetic
//! "local slot" numbers are needed here.

use crate::index::{Index, ListData, Migration, MigrationKind};
use rk_card::{ExtData, Value};
use std::collections::HashMap;
use std::sync::Arc;

pub const RESOLVER_KEY: &str = "com.bepis.sideloader.universalautoresolver";

#[derive(Debug, Clone)]
pub struct Item {
    pub category: i32,
    pub id: i32,
    pub mod_idx: Option<u32>,
    keys: Arc<[String]>,
    values: Vec<String>,
}

impl Item {
    /// Raw column value.
    pub fn raw(&self, key: &str) -> Option<&str> {
        let i = self.keys.iter().position(|k| k == key)?;
        self.values.get(i).map(String::as_str)
    }
    /// Column value, treating the game's "0" and empty placeholders as absent.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.raw(key).map(str::trim).filter(|v| !v.is_empty() && *v != "0")
    }
    pub fn int(&self, key: &str) -> Option<i32> {
        self.raw(key)?.trim().parse().ok()
    }
    pub fn name(&self) -> &str {
        self.raw("Name").unwrap_or("")
    }
    pub fn columns(&self) -> impl Iterator<Item = (&str, &str)> {
        self.keys
            .iter()
            .map(String::as_str)
            .zip(self.values.iter().map(String::as_str))
    }
}

/// Sideloader's record for one card property.
#[derive(Debug, Clone)]
pub struct ResolveInfo {
    pub guid: String,
    pub slot: i32,
    pub category: i32,
    pub property: String,
}

/// Resolver records of a card or coordinate file, by property name.
pub fn resolve_infos(ext: &ExtData) -> HashMap<String, ResolveInfo> {
    let mut out = HashMap::new();
    let Some(plugin) = ext.get(RESOLVER_KEY) else {
        return out;
    };
    let Some(Value::Array(items)) = plugin.data.get("info") else {
        return out;
    };
    for item in items {
        let decoded;
        let v = match item {
            Value::Binary(b) => match rmpv::decode::read_value(&mut &b[..]) {
                Ok(v) => {
                    decoded = v;
                    &decoded
                }
                Err(_) => continue,
            },
            other => other,
        };
        let s = |k: &str| {
            rk_card::map_get(v, k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let i = |k: &str| rk_card::map_get(v, k).and_then(Value::as_i64).unwrap_or(0) as i32;
        let info = ResolveInfo {
            guid: s("ModID"),
            slot: i("Slot"),
            category: i("CategoryNo"),
            property: s("Property"),
        };
        out.entry(info.property.clone()).or_insert(info);
    }
    out
}

#[derive(Debug)]
pub enum Lookup<'a> {
    Found(&'a Item),
    /// The card names a mod that is not installed (or lacks this item).
    MissingMod {
        guid: String,
        slot: i32,
    },
    /// No item with this ID exists in the game's own lists.
    Missing {
        id: i32,
    },
}

impl<'a> Lookup<'a> {
    pub fn item(&self) -> Option<&'a Item> {
        match self {
            Lookup::Found(i) => Some(i),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct ItemDb {
    vanilla: HashMap<(i32, i32), Item>,
    modded: HashMap<(u32, i32, i32), Item>,
    guids: HashMap<String, u32>,
    migrations: HashMap<String, Vec<Migration>>,
}

impl ItemDb {
    pub fn build(index: &Index) -> Self {
        let mut db = ItemDb::default();
        for (i, m) in index.mods.iter().enumerate() {
            db.guids.insert(m.guid.clone(), i as u32);
        }
        for list in &index.lists {
            db.add_list(list);
        }
        for m in &index.migrations {
            db.migrations.entry(m.guid_old.clone()).or_default().push(m.clone());
        }
        db
    }

    fn add_list(&mut self, list: &ListData) {
        let Some(id_col) = list.keys.iter().position(|k| k == "ID") else {
            return;
        };
        let keys: Arc<[String]> = list.keys.clone().into();
        for row in &list.rows {
            let Some(id) = row.get(id_col).and_then(|v| v.trim().parse::<i32>().ok()) else {
                continue;
            };
            let item = Item {
                category: list.category,
                id,
                mod_idx: list.mod_idx,
                keys: keys.clone(),
                values: row.clone(),
            };
            match list.mod_idx {
                // First definition wins, as in the game.
                None => drop(self.vanilla.entry((list.category, id)).or_insert(item)),
                Some(m) => drop(self.modded.entry((m, list.category, id)).or_insert(item)),
            }
        }
    }

    pub fn vanilla(&self, category: i32, id: i32) -> Option<&Item> {
        self.vanilla.get(&(category, id))
    }

    pub fn modded(&self, guid: &str, category: i32, id: i32) -> Option<&Item> {
        self.modded.get(&(*self.guids.get(guid)?, category, id))
    }

    pub fn has_mod(&self, guid: &str) -> bool {
        self.guids.contains_key(guid)
    }

    pub fn counts(&self) -> (usize, usize) {
        (self.vanilla.len(), self.modded.len())
    }

    /// Applies manifest migrations: items that moved to another mod.
    /// Returns the GUID (empty when stripped) and ID to look up.
    fn migrate(&self, info: &ResolveInfo) -> (String, i32) {
        let Some(list) = self.migrations.get(&info.guid) else {
            return (info.guid.clone(), info.slot);
        };
        if list.iter().any(|m| m.kind == MigrationKind::StripAll) {
            return (String::new(), info.slot);
        }
        for m in list.iter().filter(|m| m.kind == MigrationKind::Migrate) {
            if m.id_old == info.slot && m.category == info.category && self.has_mod(&m.guid_new) {
                return (m.guid_new.clone(), m.id_new);
            }
        }
        for m in list.iter().filter(|m| m.kind == MigrationKind::MigrateAll) {
            if self.has_mod(&m.guid_new) {
                return (m.guid_new.clone(), info.slot);
            }
        }
        (info.guid.clone(), info.slot)
    }

    /// Finds the item a card property refers to.
    pub fn lookup(&self, infos: &HashMap<String, ResolveInfo>, property: &str, category: i32, id: i32) -> Lookup<'_> {
        if let Some(info) = infos.get(property).filter(|i| !i.guid.is_empty()) {
            let (guid, slot) = self.migrate(info);
            let info = &ResolveInfo {
                guid,
                slot,
                ..info.clone()
            };
            if info.guid.is_empty() {
                return match self.vanilla(category, id) {
                    Some(item) => Lookup::Found(item),
                    None => Lookup::Missing { id },
                };
            }
            if let Some(item) = self.modded(&info.guid, category, info.slot) {
                return Lookup::Found(item);
            }
            // Same fallback as Sideloader: a game item with that ID, if any.
            if let Some(item) = self.vanilla(category, id) {
                return Lookup::Found(item);
            }
            return Lookup::MissingMod {
                guid: info.guid.clone(),
                slot: info.slot,
            };
        }
        match self.vanilla(category, id) {
            Some(item) => Lookup::Found(item),
            None => Lookup::Missing { id },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::ModInfo;

    fn list(mod_idx: Option<u32>, category: i32, rows: &[(&str, &str)]) -> ListData {
        ListData {
            mod_idx,
            category,
            distribution: 0,
            keys: vec!["ID".into(), "Name".into(), "MainAB".into()],
            rows: rows
                .iter()
                .map(|(id, name)| vec![id.to_string(), name.to_string(), "0".into()])
                .collect(),
        }
    }

    fn db() -> ItemDb {
        let mut index = Index::default();
        for guid in ["mod.new", "mod.other"] {
            index.mods.push(ModInfo {
                guid: guid.into(),
                ..Default::default()
            });
        }
        index.lists.push(list(None, 105, &[("0", "none"), ("7", "game top")]));
        index.lists.push(list(None, 105, &[("7", "shadowed duplicate")]));
        index
            .lists
            .push(list(Some(0), 105, &[("7", "mod top"), ("15", "moved here")]));
        index.lists.push(list(Some(1), 122, &[("3", "hat")]));
        index.migrations.push(Migration {
            kind: MigrationKind::Migrate,
            guid_old: "mod.old".into(),
            guid_new: "mod.new".into(),
            category: 105,
            id_old: 900,
            id_new: 15,
        });
        index.migrations.push(Migration {
            kind: MigrationKind::MigrateAll,
            guid_old: "mod.renamed".into(),
            guid_new: "mod.other".into(),
            category: 0,
            id_old: 0,
            id_new: 0,
        });
        index.migrations.push(Migration {
            kind: MigrationKind::StripAll,
            guid_old: "mod.hard".into(),
            guid_new: String::new(),
            category: 0,
            id_old: 0,
            id_new: 0,
        });
        ItemDb::build(&index)
    }

    fn info(prop: &str, guid: &str, slot: i32, category: i32) -> HashMap<String, ResolveInfo> {
        let mut m = HashMap::new();
        m.insert(
            prop.to_string(),
            ResolveInfo {
                guid: guid.into(),
                slot,
                category,
                property: prop.into(),
            },
        );
        m
    }

    #[test]
    fn game_items_first_definition_wins() {
        let db = db();
        let none = HashMap::new();
        assert_eq!(db.lookup(&none, "p", 105, 7).item().unwrap().name(), "game top");
        assert!(matches!(db.lookup(&none, "p", 105, 99), Lookup::Missing { id: 99 }));
    }

    #[test]
    fn mod_items_resolve_by_guid_and_slot() {
        let db = db();
        // The ID stored in the card is irrelevant when a record names the mod.
        let infos = info("p", "mod.new", 7, 105);
        assert_eq!(db.lookup(&infos, "p", 105, 100000123).item().unwrap().name(), "mod top");
        // Other properties are unaffected by that record.
        assert_eq!(db.lookup(&infos, "q", 105, 7).item().unwrap().name(), "game top");
    }

    #[test]
    fn missing_mod_is_reported() {
        let db = db();
        let infos = info("p", "mod.absent", 4, 105);
        assert!(matches!(
            db.lookup(&infos, "p", 105, 100000001),
            Lookup::MissingMod { slot: 4, .. }
        ));
    }

    #[test]
    fn migrations_redirect_items() {
        let db = db();
        let moved = info("p", "mod.old", 900, 105);
        assert_eq!(db.lookup(&moved, "p", 105, 1).item().unwrap().name(), "moved here");
        let renamed = info("p", "mod.renamed", 3, 122);
        assert_eq!(db.lookup(&renamed, "p", 122, 1).item().unwrap().name(), "hat");
        let stripped = info("p", "mod.hard", 7, 105);
        assert_eq!(db.lookup(&stripped, "p", 105, 7).item().unwrap().name(), "game top");
    }

    #[test]
    fn placeholder_columns_read_as_absent() {
        let db = db();
        let item = db.vanilla(105, 7).unwrap();
        assert_eq!(item.raw("MainAB"), Some("0"));
        assert_eq!(item.get("MainAB"), None);
        assert_eq!(item.int("ID"), Some(7));
    }
}
