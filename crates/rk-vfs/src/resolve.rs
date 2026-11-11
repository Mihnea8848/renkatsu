// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Walks every item reference in a card and resolves it against the item
//! database: face and body parts, hair, and each outfit's clothes and
//! accessories.

use crate::cat;
use crate::items::{resolve_infos, ItemDb, Lookup, ResolveInfo};
use rk_card::{Card, Coordinate};
use std::collections::HashMap;

pub struct Slot<'a> {
    /// Human-readable slot name, e.g. `outfit0.top`.
    pub label: String,
    pub category: i32,
    pub id: i32,
    pub lookup: Lookup<'a>,
}

pub struct CardItems<'a> {
    pub infos: HashMap<String, ResolveInfo>,
    pub slots: Vec<Slot<'a>>,
}

impl<'a> CardItems<'a> {
    pub fn missing(&self) -> impl Iterator<Item = &Slot<'a>> {
        self.slots.iter().filter(|s| !matches!(s.lookup, Lookup::Found(_)))
    }
}

/// Resolves one outfit. `prefix` is `outfitN.` for cards, empty for
/// standalone coordinate files.
pub fn coordinate_slots<'a>(
    db: &'a ItemDb,
    infos: &HashMap<String, ResolveInfo>,
    co: &Coordinate,
    prefix: &str,
    label: &str,
    out: &mut Vec<Slot<'a>>,
) {
    for (i, part) in co.clothes.parts.iter().enumerate().take(9) {
        let prop = format!("{prefix}ChaFileClothes.{}", cat::CLOTHES_PROPS[i]);
        out.push(Slot {
            label: format!("{label}{}", rk_card::CLOTHES_KIND_NAMES[i]),
            category: cat::CLOTHES[i],
            id: part.id,
            lookup: db.lookup(infos, &prop, cat::CLOTHES[i], part.id),
        });
    }
    for (i, acc) in co.accessory.parts.iter().enumerate() {
        if acc.is_empty() {
            continue;
        }
        let prop = format!("{prefix}accessory{i}.ChaFileAccessory.PartsInfo.id");
        out.push(Slot {
            label: format!("{label}accessory{i}"),
            category: acc.kind,
            id: acc.id,
            lookup: db.lookup(infos, &prop, acc.kind, acc.id),
        });
    }
}

pub fn card_items<'a>(db: &'a ItemDb, card: &Card) -> CardItems<'a> {
    let infos = resolve_infos(&card.ext);
    let mut slots = Vec::new();
    let mut add = |label: &str, prop: &str, category: i32, id: i32| {
        slots.push(Slot {
            label: label.to_string(),
            category,
            id,
            lookup: db.lookup(&infos, prop, category, id),
        });
    };
    let f = &card.face;
    add("head", "ChaFileFace.headId", cat::BO_HEAD, f.head_id);
    add("face detail", "ChaFileFace.detailId", cat::MT_FACE_DETAIL, f.detail_id);
    add("eyebrow", "ChaFileFace.eyebrowId", cat::MT_EYEBROW, f.eyebrow_id);
    add("nose", "ChaFileFace.noseId", cat::MT_NOSE, f.nose_id);
    add("eye highlight up", "ChaFileFace.hlUpId", cat::MT_EYE_HI_UP, f.hl_up_id);
    add(
        "eye highlight down",
        "ChaFileFace.hlDownId",
        cat::MT_EYE_HI_DOWN,
        f.hl_down_id,
    );
    add("eye white", "ChaFileFace.whiteId", cat::MT_EYE_WHITE, f.white_id);
    add(
        "eyeline up",
        "ChaFileFace.eyelineUpId",
        cat::MT_EYELINE_UP,
        f.eyeline_up_id,
    );
    add(
        "eyeline down",
        "ChaFileFace.eyelineDownId",
        cat::MT_EYELINE_DOWN,
        f.eyeline_down_id,
    );
    add("mole", "ChaFileFace.moleId", cat::MT_MOLE, f.mole_id);
    add("lip line", "ChaFileFace.lipLineId", cat::MT_LIPLINE, f.lip_line_id);
    for (i, p) in f.pupil.iter().enumerate().take(2) {
        add(
            &format!("pupil{i}"),
            &format!("ChaFileFace.Pupil{}", i + 1),
            cat::MT_EYE,
            p.id,
        );
        add(
            &format!("pupil{i} gradient"),
            &format!("ChaFileFace.PupilGradient{}", i + 1),
            cat::MT_EYE_GRADATION,
            p.grad_mask_id,
        );
    }
    let m = &f.base_makeup;
    add(
        "eyeshadow",
        "ChaFileMakeup.eyeshadowId",
        cat::MT_EYESHADOW,
        m.eyeshadow_id,
    );
    add("cheek", "ChaFileMakeup.cheekId", cat::MT_CHEEK, m.cheek_id);
    add("lip", "ChaFileMakeup.lipId", cat::MT_LIP, m.lip_id);
    for (i, id) in m.paint_id.iter().enumerate().take(2) {
        add(
            &format!("face paint{i}"),
            &format!("ChaFileMakeup.PaintID{}", i + 1),
            cat::MT_FACE_PAINT,
            *id,
        );
    }
    let b = &card.body;
    add("body detail", "ChaFileBody.detailId", cat::MT_BODY_DETAIL, b.detail_id);
    add("sunburn", "ChaFileBody.sunburnId", cat::MT_SUNBURN, b.sunburn_id);
    add("nipple", "ChaFileBody.nipId", cat::MT_NIP, b.nip_id);
    add(
        "underhair",
        "ChaFileBody.underhairId",
        cat::MT_UNDERHAIR,
        b.underhair_id,
    );
    for (i, id) in b.paint_id.iter().enumerate().take(2) {
        add(
            &format!("body paint{i}"),
            &format!("ChaFileBody.PaintID{}", i + 1),
            cat::MT_BODY_PAINT,
            *id,
        );
    }
    add(
        "hair gloss",
        "ChaFileHair.glossId",
        cat::MT_HAIRGLOSS,
        card.hair.gloss_id,
    );
    for (i, p) in card.hair.parts.iter().enumerate().take(4) {
        add(
            &format!("hair {}", ["back", "front", "side", "option"][i]),
            &format!("ChaFileHair.{}", cat::HAIR_PROPS[i]),
            cat::HAIR[i],
            p.id,
        );
    }
    for (i, co) in card.coordinates.iter().enumerate() {
        coordinate_slots(
            db,
            &infos,
            co,
            &format!("outfit{i}."),
            &format!("outfit{i}."),
            &mut slots,
        );
    }
    CardItems { infos, slots }
}
