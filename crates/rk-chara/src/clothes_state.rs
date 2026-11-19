// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The states of the nine pieces of clothing, as CharaStudio's State panel
//! sets them: on, half off, half off further, and off.
//!
//! Three parts of `ChaControl` are followed here. `AddClothesStateKind`
//! says which states a worn piece has (`dictStateType`), from the
//! `StateType` column of its list entry and from the pieces around it.
//! `SetClothesState` moves a piece to a state, skipping the ones it does
//! not have, and pulls the paired piece along: top and bottom of a one
//! piece top, bra and shorts of a one piece bra, and the two kinds of
//! shoes. `UpdateVisible` then shows the objects each state names
//! (`ChaReference` finds them by name in the piece's prefab), and
//! `ChangeAlphaMask` lets the top's state decide how much of the body, the
//! bra and the top's inner layer the top's mask cuts away.
//!
//! One part of the body follows the states as well: `S_Son` (`n_dankon`),
//! which `UpdateVisible` shows while neither the bottom nor the shorts is
//! fully on, and only while the character's own switch for it is on, as
//! CharaStudio's State panel sets that switch.
//!
//! Not followed: the rest of `UpdateVisible` that is not clothing, the
//! accessory options of a piece (`DrawOption`) and its sleeves
//! (`DrawSode`), which follow the outfit and not the state, and the
//! switching of the top's skirt bones (`DB_SKIRT_TOP`).

use crate::scene::{Instance, Scene};

/// How many kinds of clothing an outfit has.
pub const KINDS: usize = 9;

/// The kinds in the game's order (`ChaFileDefine.ClothesKind`).
pub const TOP: usize = 0;
pub const BOTTOM: usize = 1;
pub const BRA: usize = 2;
pub const SHORTS: usize = 3;
pub const GLOVES: usize = 4;
pub const PANTYHOSE: usize = 5;
pub const SOCKS: usize = 6;
pub const SHOES_INDOOR: usize = 7;
pub const SHOES_OUTDOOR: usize = 8;

/// The states: fully on, the two half off ones, and off.
pub const ON: u8 = 0;
pub const HALF: u8 = 1;
pub const HALF_2: u8 = 2;
pub const OFF: u8 = 3;

/// The state of every kind, as `ChaFileStatus.clothesState`.
pub type States = [u8; KINDS];

/// What the worn pieces say about their states, as it stands once every
/// piece of an outfit is loaded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Worn {
    /// The `StateType` list column of each loaded piece; `None` for a kind
    /// with no object (`infoClothes` is null).
    pub state_type: [Option<i32>; KINDS],
    /// `ChaControl.notBot`: the top is a one piece and there is no bottom.
    pub not_bot: bool,
    /// `ChaControl.notBra`: the top rules the bra out.
    pub not_bra: bool,
    /// `ChaControl.notShorts`: the bra is a one piece and there are no
    /// shorts.
    pub not_shorts: bool,
    /// The top's prefab has a bottom part (`S_CTOP_B_DEF`, `n_bot_a`).
    pub top_has_bottom: bool,
    /// The bottom's prefab has a top part (`S_CBOT_T_DEF`, `n_top_a`).
    pub bottom_has_top: bool,
    /// The bra's prefab has a bottom part (`S_UWT_B_DEF`, `n_bot_a`).
    pub bra_has_bottom: bool,
    /// The shorts' prefab has a top part (`S_UWB_T_DEF`, `n_top_a`).
    pub shorts_have_top: bool,
    /// `ChaControl.hideShortsBot`: the bottom hides the shorts while it is
    /// fully on.
    pub hide_shorts_bot: bool,
}

/// `byte.Parse` of the column, with 3 (no states) for a missing piece.
fn type_of(column: Option<i32>) -> u8 {
    column.and_then(|t| u8::try_from(t).ok()).unwrap_or(3)
}

/// The kinds that have states and which (`dictStateType`): for each kind
/// its state type, 0 for on, half and off, 1 for on and off, 2 for all
/// four.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Kinds(pub [Option<u8>; KINDS]);

impl Kinds {
    /// `IsClothesStateKind`.
    pub fn is_kind(&self, kind: usize) -> bool {
        self.0[kind].is_some()
    }

    /// `IsClothesStateType`: whether a kind has a state.
    pub fn has(&self, kind: usize, state: u8) -> bool {
        match (self.0[kind], state) {
            (None, _) => false,
            (Some(_), ON | OFF) => true,
            (Some(0), HALF) => true,
            (Some(2), HALF | HALF_2) => true,
            _ => false,
        }
    }

    /// The states of a kind in order; empty for a kind without states.
    pub fn states(&self, kind: usize) -> Vec<u8> {
        (ON..=OFF).filter(|&s| self.has(kind, s)).collect()
    }
}

impl Worn {
    /// `AddClothesStateKind` for every piece.
    pub fn kinds(&self) -> Kinds {
        // `AddClothesStateKindSub` takes only the types 0 to 2.
        let sub = |t: u8| (t <= 2).then_some(t);
        let mut out = [None; KINDS];
        for (kind, slot) in out.iter_mut().enumerate().skip(GLOVES) {
            *slot = self.state_type[kind].and_then(|t| u8::try_from(t).ok()).and_then(sub);
        }

        // Top and bottom lend each other their type through the part of
        // the other one that their prefab carries.
        let mut top = type_of(self.state_type[TOP]);
        let mut bottom = type_of(self.state_type[BOTTOM]);
        if bottom != 0 && self.top_has_bottom {
            match top {
                0 => bottom = 0,
                1 if bottom == 2 => bottom = 1,
                _ => {}
            }
        }
        if top != 0 && self.bottom_has_top {
            match bottom {
                0 => top = 0,
                1 if top == 2 => top = 1,
                _ => {}
            }
        }
        out[TOP] = sub(top);
        out[BOTTOM] = sub(bottom);

        // Bra and shorts the same way, unless the top or the bra rules
        // them out.
        let mut bra = if self.not_bra { 3 } else { type_of(self.state_type[BRA]) };
        let mut shorts = if !self.not_bra || !self.not_shorts {
            type_of(self.state_type[SHORTS])
        } else {
            3
        };
        if shorts != 0 && shorts != 2 && self.bra_has_bottom {
            match bra {
                0 => shorts = 0,
                1 if shorts == 3 => shorts = 1,
                _ => {}
            }
        }
        if bra != 0 && bra != 2 && self.shorts_have_top {
            match shorts {
                0 => bra = 0,
                1 if bra == 3 => bra = 1,
                _ => {}
            }
        }
        out[BRA] = sub(bra);
        out[SHORTS] = sub(shorts);
        Kinds(out)
    }
}

/// `ChaControl.SetClothesState`: asks for `state` on `kind`. A state the
/// kind does not have moves on to the next one (`next`) or the one before
/// until one fits, and the paired piece follows.
pub fn set_state(states: &mut States, worn: &Worn, kind: usize, state: u8, next: bool) {
    let kinds = worn.kinds();
    let mut state = state % 4;
    let was = states[kind];
    loop {
        if !kinds.is_kind(kind) || kinds.has(kind, state) {
            states[kind] = state;
            break;
        }
        state = (state + if next { 1 } else { 3 }) % 4;
        if state == was {
            break;
        }
    }
    let mut pair = |this: usize, other: usize, joined: bool| {
        if joined {
            if states[this] == OFF {
                states[other] = OFF;
            } else if states[other] == OFF {
                states[other] = state;
            }
        }
    };
    match kind {
        TOP => pair(TOP, BOTTOM, worn.not_bot),
        BOTTOM => pair(BOTTOM, TOP, worn.not_bot),
        BRA => pair(BRA, SHORTS, worn.not_shorts),
        SHORTS => pair(SHORTS, BRA, worn.not_shorts),
        SHOES_INDOOR | SHOES_OUTDOOR => {
            let other = if kind == SHOES_INDOOR {
                SHOES_OUTDOOR
            } else {
                SHOES_INDOOR
            };
            if state != ON || kinds.is_kind(other) {
                states[other] = state;
            }
        }
        _ => {}
    }
}

/// `SetClothesStateNext`: the button that cycles a piece.
pub fn next_state(states: &mut States, worn: &Worn, kind: usize) {
    set_state(states, worn, kind, (states[kind] + 1) % 4, true);
}

/// `SetClothesStatePrev`.
pub fn prev_state(states: &mut States, worn: &Worn, kind: usize) {
    set_state(states, worn, kind, (states[kind] + 3) % 4, false);
}

/// `SetClothesStateAll`: every kind in turn, as CharaStudio's buttons for
/// all on, all half off and all off do.
pub fn set_all(states: &mut States, worn: &Worn, state: u8) {
    for kind in 0..KINDS {
        set_state(states, worn, kind, state, true);
    }
}

/// States read from a file, made to fit what is worn: every kind is set
/// again in turn (`UpdateClothesStateAll`).
pub fn fitted(states: &States, worn: &Worn) -> States {
    let mut out = *states;
    for (kind, state) in states.iter().enumerate() {
        set_state(&mut out, worn, kind, (*state).min(OFF), true);
    }
    out
}

/// What `UpdateVisible` shows for a set of states. Each field is the
/// active flag of one object; the names are those of `ChaReference`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    /// `objClothes` of each kind.
    pub objects: [bool; KINDS],
    /// `objParts`: the three parts of a sailor or jacket top.
    pub parts: bool,
    /// `S_CTOP_T_DEF`, `S_CTOP_T_NUGE`, `S_CTOP_B_DEF`, `S_CTOP_B_NUGE`;
    /// the first two also stand for `S_TPARTS_*_DEF` and `_NUGE`.
    pub top: [bool; 4],
    /// `S_CBOT_B_DEF`, `S_CBOT_B_NUGE`, `S_CBOT_T_DEF`, `S_CBOT_T_NUGE`.
    pub bottom: [bool; 4],
    /// `S_UWT_T_DEF`, `S_UWT_T_NUGE`, `S_UWT_B_DEF`, `S_UWT_B_NUGE`.
    pub bra: [bool; 4],
    /// `S_UWB_B_DEF`, `S_UWB_B_NUGE`, `S_UWB_B_NUGE2`.
    pub shorts: [bool; 3],
    /// `S_PANST_DEF`, `S_PANST_NUGE`.
    pub pantyhose: [bool; 2],
    /// `_alpha_a` and `_alpha_b` of the body, the bra and the inner layer.
    pub alpha: [f32; 2],
    /// `S_Son` (`n_dankon`), the part of a male body the bottom and the
    /// shorts cover.
    pub son: bool,
}

/// `UpdateVisible` for the clothes of a character that is shown whole.
/// `bottom_object` says whether a bottom object exists, which the rule for
/// hidden shorts asks.
pub fn shown(s: &States, worn: &Worn, outdoor_shoes: bool, bottom_object: bool) -> Shown {
    let mut objects = [false; KINDS];
    objects[TOP] = s[TOP] != OFF;
    objects[BOTTOM] = !worn.not_bot && s[BOTTOM] != OFF;
    objects[BRA] = !worn.not_bra && s[BRA] != OFF;
    // The shorts hide under a bottom made for it, while that is fully on.
    let under_bottom = worn.hide_shorts_bot && bottom_object && objects[BOTTOM] && s[BOTTOM] == ON;
    objects[SHORTS] = !under_bottom && !worn.not_shorts && s[SHORTS] != OFF;
    objects[GLOVES] = s[GLOVES] == ON;
    objects[PANTYHOSE] = s[PANTYHOSE] != OFF && s[PANTYHOSE] != HALF_2;
    objects[SOCKS] = s[SOCKS] == ON;
    objects[SHOES_INDOOR] = !outdoor_shoes && s[SHOES_INDOOR] == ON;
    objects[SHOES_OUTDOOR] = outdoor_shoes && s[SHOES_OUTDOOR] == ON;
    const ALPHA: [[f32; 2]; 4] = [[1.0, 1.0], [0.0, 1.0], [0.0, 1.0], [0.0, 0.0]];
    Shown {
        son: false,
        objects,
        parts: s[TOP] != OFF,
        top: [
            s[TOP] == ON,
            s[TOP] == HALF,
            s[BOTTOM] == ON && s[TOP] != OFF,
            s[BOTTOM] != ON && s[TOP] != OFF,
        ],
        bottom: [
            s[BOTTOM] == ON,
            s[BOTTOM] == HALF,
            s[TOP] == ON && s[BOTTOM] != HALF_2,
            s[TOP] != ON && s[BOTTOM] != HALF_2,
        ],
        bra: [
            s[BRA] == ON,
            s[BRA] == HALF,
            s[SHORTS] == ON && s[BRA] != OFF,
            s[SHORTS] != ON && s[BRA] != OFF,
        ],
        shorts: [s[SHORTS] == ON, s[SHORTS] == HALF, s[SHORTS] == HALF_2],
        pantyhose: [s[PANTYHOSE] == ON, s[PANTYHOSE] == HALF],
        alpha: ALPHA[s[TOP].min(OFF) as usize],
    }
}

/// What `UpdateVisible` asks about `S_Son` besides the clothes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Son {
    /// `ChaFileParameter.exType`. With 1 the top rules it instead of the
    /// bottom and the shorts.
    pub ex_type: i64,
    /// `ChaFileStatus.visibleSon`: drawn whatever the clothes say.
    pub whatever_is_worn: bool,
    /// `ChaFileStatus.visibleSonAlways`, which CharaStudio's State panel
    /// sets from its Son switch. Nothing is drawn while it is off.
    pub switched_on: bool,
}

impl Son {
    /// Reads what the card says, for a character the scene has not
    /// switched itself.
    pub fn of_card(card: &rk_card::Card) -> Son {
        let flag = |key: &str| {
            rk_card::map_get(&card.status, key)
                .and_then(rk_card::Value::as_bool)
                .unwrap_or(true)
        };
        Son {
            ex_type: card.param_int("exType").unwrap_or(0),
            whatever_is_worn: rk_card::map_get(&card.status, "visibleSon")
                .and_then(rk_card::Value::as_bool)
                .unwrap_or(false),
            switched_on: flag("visibleSonAlways"),
        }
    }

    /// Whether `S_Son` is drawn, as `UpdateVisible` decides it outside the
    /// H scenes, where the body and the part itself are never hidden on
    /// the scene's own account.
    pub fn shown(&self, states: &States, kinds: &Kinds) -> bool {
        let on = |kind: usize| kinds.is_kind(kind) && states[kind] == ON;
        let covered = if self.ex_type == 1 {
            on(TOP)
        } else {
            on(BOTTOM) || on(SHORTS)
        };
        self.switched_on && (!covered || self.whatever_is_worn)
    }
}

/// The objects of a built character that the states switch.
#[derive(Debug, Clone, Default)]
pub struct Rig {
    pub worn: Worn,
    /// `ChaFileStatus.shoesType`: the outdoor shoes are the ones worn.
    pub outdoor_shoes: bool,
    /// What decides `S_Son` besides the clothes.
    pub son: Son,
    /// `n_dankon` of the body, for a male character.
    son_node: Option<usize>,
    /// The top object of each loaded piece.
    objects: [Option<usize>; KINDS],
    /// The top objects of the parts of a sailor or jacket top.
    parts: Vec<usize>,
    /// Nodes that take `Shown::top`: for each of the four flags, the nodes
    /// of the top and of its parts.
    top: [Vec<usize>; 4],
    bottom: [Option<usize>; 4],
    bra: [Option<usize>; 4],
    shorts: [Option<usize>; 3],
    pantyhose: [Option<usize>; 2],
    /// Renderers whose first material takes the alpha values: the body
    /// (`customMatBody`), the bra (`rendBra`) and the inner layer of the
    /// top's first part (`rendInner`).
    alpha: Vec<usize>,
    /// Renderers whose first material takes the liquid levels, by kind:
    /// the `rendNormal01` to `03` of the piece `UpdateClothesSiru` picks
    /// for that kind.
    liquid: [Vec<usize>; KINDS],
}

impl Rig {
    /// A rig with no piece yet, for a character in indoor or outdoor shoes.
    pub fn new(outdoor_shoes: bool) -> Rig {
        Rig {
            outdoor_shoes,
            ..Default::default()
        }
    }

    /// Notes a loaded piece: its object and the reference objects
    /// `ChaReference.CreateReferenceInfo` finds in it by name.
    pub fn add_piece(&mut self, scene: &Scene, kind: usize, inst: &Instance) {
        let find = |name: &str| scene.find(inst.root, name);
        self.objects[kind] = Some(inst.root);
        match kind {
            TOP => {
                for (slot, name) in self.top.iter_mut().zip(["n_top_a", "n_top_b", "n_bot_a", "n_bot_b"]) {
                    slot.extend(find(name));
                }
                self.worn.top_has_bottom = find("n_bot_a").is_some();
            }
            BOTTOM => {
                self.bottom = ["n_bot_a", "n_bot_b", "n_top_a", "n_top_b"].map(find);
                self.worn.bottom_has_top = self.bottom[2].is_some();
            }
            BRA => {
                self.bra = ["n_top_a", "n_top_b", "n_bot_a", "n_bot_b"].map(find);
                self.worn.bra_has_bottom = self.bra[2].is_some();
                self.add_alpha(scene, inst, &["o_bra_a", "o_bra_b"]);
            }
            SHORTS => {
                self.shorts = ["n_bot_a", "n_bot_b", "n_bot_c"].map(find);
                self.worn.shorts_have_top = find("n_top_a").is_some();
            }
            PANTYHOSE => self.pantyhose = ["n_panst_a", "n_panst_b"].map(find),
            _ => {}
        }
    }

    /// Notes the renderers of a piece, or of a top's part, that take the
    /// liquid levels.
    pub fn add_liquid(&mut self, kind: usize, renderers: impl IntoIterator<Item = usize>) {
        self.liquid[kind].extend(renderers);
    }

    /// Every renderer that takes the liquid levels.
    pub fn liquid_renderers(&self) -> Vec<usize> {
        self.liquid.iter().flatten().copied().collect()
    }

    /// Notes a part of a sailor or jacket top (`objParts`).
    pub fn add_part(&mut self, scene: &Scene, part: usize, inst: &Instance) {
        self.parts.push(inst.root);
        for (slot, name) in self.top.iter_mut().zip(["n_top_a", "n_top_b"]) {
            slot.extend(scene.find(inst.root, name));
        }
        if part == 0 {
            self.add_alpha(scene, inst, &["o_inner_a", "o_inner_b"]);
        }
    }

    /// Notes the body, whose material the top's mask cuts.
    pub fn add_body(&mut self, scene: &Scene, body: &Instance) {
        self.add_alpha(scene, body, &["o_body_a"]);
        self.son_node = scene.find(body.root, "n_dankon");
    }

    /// The renderers on the objects of these names, each the first of its
    /// name in the prefab.
    fn add_alpha(&mut self, scene: &Scene, inst: &Instance, names: &[&str]) {
        for name in names {
            let Some(node) = scene.find(inst.root, name) else {
                continue;
            };
            self.alpha.extend(
                inst.renderers
                    .iter()
                    .copied()
                    .filter(|&r| scene.renderers[r].node == node),
            );
        }
    }

    /// Every object the states switch, under the name `ChaReference` or
    /// `ChaControl` has for it, with whether it is switched on.
    pub fn objects(&self, scene: &Scene) -> Vec<(String, bool)> {
        const KIND: [&str; KINDS] = [
            "top",
            "bot",
            "bra",
            "shorts",
            "gloves",
            "panst",
            "socks",
            "shoes_inner",
            "shoes_outer",
        ];
        let mut out: Vec<(String, usize)> = Vec::new();
        for (kind, node) in self.objects.iter().enumerate() {
            out.extend(node.map(|n| (format!("objClothes[{}]", KIND[kind]), n)));
        }
        for (i, &node) in self.parts.iter().enumerate() {
            out.push((format!("objParts[{i}]"), node));
        }
        for (nodes, name) in self.top.iter().zip(["T_DEF", "T_NUGE", "B_DEF", "B_NUGE"]) {
            for (i, &node) in nodes.iter().enumerate() {
                out.push((format!("S_CTOP_{name}#{i}"), node));
            }
        }
        let named = |out: &mut Vec<(String, usize)>, nodes: &[Option<usize>], prefix: &str, names: &[&str]| {
            for (node, name) in nodes.iter().zip(names) {
                out.extend(node.map(|n| (format!("{prefix}{name}"), n)));
            }
        };
        named(
            &mut out,
            &self.bottom,
            "S_CBOT_",
            &["B_DEF", "B_NUGE", "T_DEF", "T_NUGE"],
        );
        named(&mut out, &self.bra, "S_UWT_", &["T_DEF", "T_NUGE", "B_DEF", "B_NUGE"]);
        named(&mut out, &self.shorts, "S_UWB_", &["B_DEF", "B_NUGE", "B_NUGE2"]);
        named(&mut out, &self.pantyhose, "S_PANST_", &["DEF", "NUGE"]);
        out.extend(self.son_node.map(|n| ("S_Son".to_owned(), n)));
        out.into_iter()
            .map(|(name, node)| (name, scene.nodes[node].active))
            .collect()
    }

    /// The kinds with states.
    pub fn kinds(&self) -> Kinds {
        self.worn.kinds()
    }

    fn shown(&self, states: &States) -> Shown {
        let mut out = shown(states, &self.worn, self.outdoor_shoes, self.objects[BOTTOM].is_some());
        out.son = self.son.shown(states, &self.worn.kinds());
        out
    }

    /// Shows and hides the objects as the states say.
    pub fn apply_visibility(&self, scene: &mut Scene, states: &States) {
        let shown = self.shown(states);
        let mut set = |node: Option<usize>, on: bool| {
            if let Some(node) = node {
                scene.nodes[node].active = on;
            }
        };
        for (node, on) in self.objects.iter().zip(shown.objects) {
            set(*node, on);
        }
        for &part in &self.parts {
            set(Some(part), shown.parts);
        }
        for (nodes, on) in self.top.iter().zip(shown.top) {
            for &node in nodes {
                set(Some(node), on);
            }
        }
        for (node, on) in self.bottom.iter().zip(shown.bottom) {
            set(*node, on);
        }
        for (node, on) in self.bra.iter().zip(shown.bra) {
            set(*node, on);
        }
        for (node, on) in self.shorts.iter().zip(shown.shorts) {
            set(*node, on);
        }
        for (node, on) in self.pantyhose.iter().zip(shown.pantyhose) {
            set(*node, on);
        }
        set(self.son_node, shown.son);
    }

    /// `ChangeAlphaMask` for the top's state.
    pub fn apply_alpha(&self, scene: &mut Scene, states: &States) {
        let [a, b] = self.shown(states).alpha;
        for &r in &self.alpha {
            if let Some(m) = scene.renderers[r].materials.first_mut() {
                m.floats.insert("_alpha_a".into(), a);
                m.floats.insert("_alpha_b".into(), b);
            }
        }
    }

    /// Brings a built character to the states: the objects and the masks.
    pub fn apply(&self, scene: &mut Scene, states: &States) {
        self.apply_visibility(scene, states);
        self.apply_alpha(scene, states);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_male_part_hides_under_the_bottom_and_the_shorts() {
        let kinds = worn([1; KINDS]).kinds();
        let son = Son {
            ex_type: 0,
            whatever_is_worn: false,
            switched_on: true,
        };
        let mut states = [ON; KINDS];
        assert!(!son.shown(&states, &kinds), "the bottom is on");
        states[BOTTOM] = OFF;
        assert!(!son.shown(&states, &kinds), "the shorts are still on");
        states[SHORTS] = OFF;
        assert!(son.shown(&states, &kinds));
        // A kind the outfit has no piece for never covers it.
        let bare = Worn::default().kinds();
        assert!(son.shown(&[ON; KINDS], &bare));
        // The card may ask for it whatever is worn, and the switch rules
        // both.
        let always = Son {
            whatever_is_worn: true,
            ..son
        };
        assert!(always.shown(&[ON; KINDS], &kinds));
        assert!(!Son {
            switched_on: false,
            ..always
        }
        .shown(&states, &kinds));
        // `exType` 1 asks the top instead.
        let ex = Son { ex_type: 1, ..son };
        assert!(!ex.shown(&[ON; KINDS], &kinds), "the top is on");
        let mut half = [ON; KINDS];
        half[TOP] = HALF;
        assert!(ex.shown(&half, &kinds));
    }
    use crate::scene::{test_renderer, MaterialInst, Trs};
    use std::collections::HashMap;

    fn worn(types: [i32; KINDS]) -> Worn {
        Worn {
            state_type: types.map(Some),
            ..Default::default()
        }
    }

    #[test]
    fn the_state_type_says_which_states_a_piece_has() {
        let k = worn([0, 1, 2, 0, 1, 2, 1, 1, 1]).kinds();
        assert_eq!(k.states(TOP), [ON, HALF, OFF]);
        assert_eq!(k.states(BOTTOM), [ON, OFF]);
        assert_eq!(k.states(BRA), [ON, HALF, HALF_2, OFF]);
        assert_eq!(k.states(PANTYHOSE), [ON, HALF, HALF_2, OFF]);
        assert_eq!(k.states(GLOVES), [ON, OFF]);
        // A piece that is not loaded, or whose type is out of range, has
        // no states.
        let mut w = worn([0; KINDS]);
        w.state_type[SOCKS] = None;
        w.state_type[GLOVES] = Some(3);
        let k = w.kinds();
        assert!(!k.is_kind(SOCKS) && !k.is_kind(GLOVES));
        assert!(k.states(SOCKS).is_empty());
    }

    #[test]
    fn top_and_bottom_lend_each_other_their_type() {
        // A one piece top of type 0 with a skirt part: the missing bottom
        // becomes a kind of type 0.
        let mut w = worn([0; KINDS]);
        w.state_type[BOTTOM] = None;
        w.top_has_bottom = true;
        w.not_bot = true;
        assert_eq!(w.kinds().0[BOTTOM], Some(0));
        // Of type 1, the bottom stays without states.
        w.state_type[TOP] = Some(1);
        assert_eq!(w.kinds().0[BOTTOM], None);
        // A top of type 1 cuts a bottom of type 2 down to on and off.
        w.state_type[BOTTOM] = Some(2);
        assert_eq!(w.kinds().0[BOTTOM], Some(1));
        // A bottom with a top part does the same to the top.
        let mut w = worn([2, 1, 0, 0, 0, 0, 0, 0, 0]);
        w.bottom_has_top = true;
        assert_eq!(w.kinds().0[TOP], Some(1));
        w.state_type[BOTTOM] = Some(0);
        assert_eq!(w.kinds().0[TOP], Some(0));
    }

    #[test]
    fn bra_and_shorts_follow_the_top_and_each_other() {
        // A top that rules the bra out takes its states away.
        let mut w = worn([0; KINDS]);
        w.not_bra = true;
        let k = w.kinds();
        assert!(!k.is_kind(BRA) && k.is_kind(SHORTS));
        // With a one piece bra on top of that, the shorts go too.
        w.not_shorts = true;
        assert!(!w.kinds().is_kind(SHORTS));
        // A one piece bra of type 0 with a bottom part: the missing shorts
        // get type 0. Shorts of type 2 keep theirs.
        let mut w = worn([0; KINDS]);
        w.state_type[SHORTS] = None;
        w.bra_has_bottom = true;
        w.not_shorts = true;
        assert_eq!(w.kinds().0[SHORTS], Some(0));
        w.state_type[SHORTS] = Some(2);
        assert_eq!(w.kinds().0[SHORTS], Some(2));
        // A bra of type 1 gives shorts without states the type 1.
        w.state_type[SHORTS] = None;
        w.state_type[BRA] = Some(1);
        assert_eq!(w.kinds().0[SHORTS], Some(1));
    }

    #[test]
    fn a_state_the_piece_lacks_moves_on() {
        let w = worn([1, 0, 2, 2, 1, 2, 1, 1, 1]);
        let mut s = [0; KINDS];
        // The top has on and off only: half becomes off going forward...
        set_state(&mut s, &w, TOP, HALF, true);
        assert_eq!(s[TOP], OFF);
        // ...and half 2 becomes on going back.
        set_state(&mut s, &w, TOP, HALF_2, false);
        assert_eq!(s[TOP], ON);
        // The bottom has no second half state.
        set_state(&mut s, &w, BOTTOM, HALF_2, true);
        assert_eq!(s[BOTTOM], OFF);
        // The cycle button walks the states a piece has.
        let mut seen = Vec::new();
        for _ in 0..4 {
            next_state(&mut s, &w, SHORTS);
            seen.push(s[SHORTS]);
        }
        assert_eq!(seen, [HALF, HALF_2, OFF, ON]);
        prev_state(&mut s, &w, TOP);
        assert_eq!(s[TOP], OFF);
        // A kind without states takes any state as it is.
        let mut none = w.clone();
        none.state_type[GLOVES] = None;
        set_state(&mut s, &none, GLOVES, HALF_2, true);
        assert_eq!(s[GLOVES], HALF_2);
    }

    #[test]
    fn paired_pieces_pull_each_other_along() {
        // A one piece top: off takes the bottom off, and back on puts it
        // on again.
        let mut w = worn([0; KINDS]);
        w.state_type[BOTTOM] = None;
        w.top_has_bottom = true;
        w.not_bot = true;
        let mut s = [0; KINDS];
        set_state(&mut s, &w, TOP, OFF, true);
        assert_eq!((s[TOP], s[BOTTOM]), (OFF, OFF));
        set_state(&mut s, &w, TOP, HALF, true);
        assert_eq!((s[TOP], s[BOTTOM]), (HALF, HALF));
        // The bottom moves alone while the top is on.
        set_state(&mut s, &w, BOTTOM, ON, true);
        assert_eq!((s[TOP], s[BOTTOM]), (HALF, ON));
        set_state(&mut s, &w, BOTTOM, OFF, true);
        assert_eq!((s[TOP], s[BOTTOM]), (OFF, OFF));
        // Without a one piece, top and bottom are apart.
        let apart = worn([0; KINDS]);
        let mut s = [0; KINDS];
        set_state(&mut s, &apart, TOP, OFF, true);
        assert_eq!((s[TOP], s[BOTTOM]), (OFF, ON));

        // A one piece bra and its shorts.
        let mut w = worn([0; KINDS]);
        w.not_shorts = true;
        let mut s = [0; KINDS];
        set_state(&mut s, &w, SHORTS, OFF, true);
        assert_eq!((s[BRA], s[SHORTS]), (OFF, OFF));
        set_state(&mut s, &w, BRA, ON, true);
        assert_eq!((s[BRA], s[SHORTS]), (ON, ON));

        // The shoes: off always takes the other pair along; on only when
        // the other pair is worn.
        let mut w = worn([0, 0, 0, 0, 1, 1, 1, 1, 1]);
        let mut s = [0; KINDS];
        set_state(&mut s, &w, SHOES_INDOOR, OFF, true);
        assert_eq!((s[SHOES_INDOOR], s[SHOES_OUTDOOR]), (OFF, OFF));
        set_state(&mut s, &w, SHOES_OUTDOOR, ON, true);
        assert_eq!((s[SHOES_INDOOR], s[SHOES_OUTDOOR]), (ON, ON));
        w.state_type[SHOES_OUTDOOR] = None;
        set_state(&mut s, &w, SHOES_INDOOR, OFF, true);
        set_state(&mut s, &w, SHOES_INDOOR, ON, true);
        assert_eq!((s[SHOES_INDOOR], s[SHOES_OUTDOOR]), (ON, OFF));
    }

    #[test]
    fn all_on_and_all_off_set_every_kind() {
        let w = worn([0, 0, 2, 2, 1, 2, 1, 1, 1]);
        let mut s = [0; KINDS];
        set_all(&mut s, &w, OFF);
        assert_eq!(s, [OFF; KINDS]);
        // All half off: pieces without a half state go on to off.
        set_all(&mut s, &w, HALF);
        assert_eq!(s, [HALF, HALF, HALF, HALF, OFF, HALF, OFF, OFF, OFF]);
        set_all(&mut s, &w, ON);
        assert_eq!(s, [ON; KINDS]);
        // States from a file are made to fit.
        assert_eq!(
            fitted(&[HALF_2, 9, 0, 0, HALF, 0, 0, 0, 0], &w)[..5],
            [OFF, OFF, ON, ON, OFF]
        );
    }

    #[test]
    fn the_states_decide_what_shows() {
        let w = worn([0; KINDS]);
        let all = |s: &States| shown(s, &w, false, true);
        let on = all(&[0; KINDS]);
        assert_eq!(on.objects, [true, true, true, true, true, true, true, true, false]);
        assert_eq!(on.top, [true, false, true, false]);
        assert_eq!(on.bottom, [true, false, true, false]);
        assert_eq!(on.bra, [true, false, true, false]);
        assert_eq!(on.shorts, [true, false, false]);
        assert_eq!(on.pantyhose, [true, false]);
        assert_eq!(on.alpha, [1.0, 1.0]);

        // Top half off: its half part shows, the bottom's top part goes to
        // its undressed one, and the mask lets the body's first part show.
        let mut s = [0; KINDS];
        s[TOP] = HALF;
        let half = all(&s);
        assert_eq!(half.top, [false, true, true, false]);
        assert_eq!(half.bottom, [true, false, false, true]);
        assert_eq!(half.alpha, [0.0, 1.0]);
        assert!(half.objects[TOP] && half.parts);

        // Top off: the object goes, with its bottom parts; no mask at all.
        s[TOP] = OFF;
        let off = all(&s);
        assert!(!off.objects[TOP] && !off.parts);
        assert_eq!(off.top, [false, false, false, false]);
        assert_eq!(off.alpha, [0.0, 0.0]);

        // Bottom half off: the top's bottom part goes to its undressed one.
        let mut s = [0; KINDS];
        s[BOTTOM] = HALF;
        let v = all(&s);
        assert_eq!(v.top, [true, false, false, true]);
        assert_eq!(v.bottom, [false, true, true, false]);
        // In the second half state the bottom's top parts both hide.
        s[BOTTOM] = HALF_2;
        assert_eq!(all(&s).bottom, [false, false, false, false]);

        // Shorts: three parts, one for each state that shows them.
        let mut s = [0; KINDS];
        s[SHORTS] = HALF_2;
        let v = all(&s);
        assert_eq!(v.shorts, [false, false, true]);
        assert_eq!(v.bra, [true, false, false, true]);
        assert!(v.objects[SHORTS]);

        // Pantyhose are gone from the second half state on; gloves, socks
        // and shoes show only when on.
        let mut s = [0; KINDS];
        s[PANTYHOSE] = HALF;
        assert!(all(&s).objects[PANTYHOSE]);
        assert_eq!(all(&s).pantyhose, [false, true]);
        s[PANTYHOSE] = HALF_2;
        assert!(!all(&s).objects[PANTYHOSE]);
        s[GLOVES] = HALF;
        s[SOCKS] = OFF;
        s[SHOES_INDOOR] = OFF;
        let v = all(&s);
        assert!(!v.objects[GLOVES] && !v.objects[SOCKS] && !v.objects[SHOES_INDOOR]);
        // The shoes type picks the pair.
        let outdoor = shown(&[0; KINDS], &w, true, true);
        assert!(!outdoor.objects[SHOES_INDOOR] && outdoor.objects[SHOES_OUTDOOR]);
    }

    #[test]
    fn pieces_ruled_out_never_show() {
        let mut w = worn([0; KINDS]);
        w.not_bot = true;
        w.not_bra = true;
        w.not_shorts = true;
        let v = shown(&[0; KINDS], &w, false, false);
        assert!(!v.objects[BOTTOM] && !v.objects[BRA] && !v.objects[SHORTS]);
        // A bottom made to hide the shorts does so only while fully on.
        let mut w = worn([0; KINDS]);
        w.hide_shorts_bot = true;
        let mut s = [0; KINDS];
        assert!(!shown(&s, &w, false, true).objects[SHORTS]);
        s[BOTTOM] = HALF;
        assert!(shown(&s, &w, false, true).objects[SHORTS]);
        s[BOTTOM] = OFF;
        assert!(shown(&s, &w, false, true).objects[SHORTS]);
        // Without a bottom object the rule does not apply.
        assert!(shown(&[0; KINDS], &w, false, false).objects[SHORTS]);
    }

    /// A prefab: a root with the named children, each with a renderer.
    fn prefab(scene: &mut Scene, top: usize, root: &str, children: &[&str]) -> Instance {
        let root = scene.add_node(root, Some(top), Trs::default());
        let mut inst = Instance {
            root,
            nodes: vec![root],
            renderers: Vec::new(),
            renderer_keys: HashMap::new(),
            object_nodes: HashMap::new(),
            transform_nodes: HashMap::new(),
            scripts: Vec::new(),
        };
        for name in children {
            let node = scene.add_node(name, Some(root), Trs::default());
            inst.nodes.push(node);
            let mut r = test_renderer(name, node);
            r.materials.push(MaterialInst::default());
            inst.renderers.push(scene.renderers.len());
            scene.renderers.push(r);
        }
        inst
    }

    #[test]
    fn a_built_character_follows_its_states() {
        let mut scene = Scene::default();
        let top_node = scene.add_node("BodyTop", None, Trs::default());
        let body = prefab(&mut scene, top_node, "p_cf_body_00", &["o_body_a"]);
        let top = prefab(
            &mut scene,
            top_node,
            "ct_top",
            &["n_top_a", "n_top_b", "n_bot_a", "n_bot_b"],
        );
        let bra = prefab(&mut scene, top_node, "ct_bra", &["n_top_a", "n_top_b", "o_bra_a"]);
        let shorts = prefab(&mut scene, top_node, "ct_shorts", &["n_bot_a", "n_bot_b", "n_bot_c"]);
        let mut rig = Rig::new(false);
        rig.worn.state_type[TOP] = Some(0);
        rig.worn.state_type[BRA] = Some(0);
        rig.worn.state_type[SHORTS] = Some(2);
        rig.worn.not_bot = true;
        rig.add_body(&scene, &body);
        rig.add_piece(&scene, TOP, &top);
        rig.add_piece(&scene, BRA, &bra);
        rig.add_piece(&scene, SHORTS, &shorts);
        assert!(rig.worn.top_has_bottom && !rig.worn.bra_has_bottom);
        // The one piece top gives the missing bottom its states.
        assert_eq!(rig.kinds().states(BOTTOM), [ON, HALF, OFF]);

        let active = |scene: &Scene, inst: &Instance| -> Vec<bool> {
            inst.nodes.iter().map(|&n| scene.nodes[n].active).collect()
        };
        let alpha = |scene: &Scene, inst: &Instance, name: &str| -> Option<[f32; 2]> {
            let r = inst.renderers.iter().find(|&&r| scene.renderers[r].name == name)?;
            let m = &scene.renderers[*r].materials[0];
            Some([*m.floats.get("_alpha_a")?, *m.floats.get("_alpha_b")?])
        };
        let mut states = [0; KINDS];
        rig.apply_visibility(&mut scene, &states);
        assert_eq!(active(&scene, &top), [true, true, false, true, false]);
        assert_eq!(active(&scene, &shorts), [true, true, false, false]);
        // Showing and hiding alone leaves the materials as they were.
        assert_eq!(alpha(&scene, &body, "o_body_a"), None);

        set_state(&mut states, &rig.worn, TOP, HALF, true);
        rig.apply(&mut scene, &states);
        assert_eq!(active(&scene, &top), [true, false, true, true, false]);
        assert_eq!(alpha(&scene, &body, "o_body_a"), Some([0.0, 1.0]));
        assert_eq!(alpha(&scene, &bra, "o_bra_a"), Some([0.0, 1.0]));
        // Only the body, the bra and the inner layer take the mask values.
        assert_eq!(alpha(&scene, &top, "n_top_a"), None);

        set_state(&mut states, &rig.worn, TOP, OFF, true);
        assert_eq!(states[BOTTOM], OFF);
        rig.apply(&mut scene, &states);
        assert!(!scene.nodes[top.root].active);
        assert_eq!(alpha(&scene, &body, "o_body_a"), Some([0.0, 0.0]));

        set_all(&mut states, &rig.worn, ON);
        rig.apply(&mut scene, &states);
        assert_eq!(active(&scene, &top), [true, true, false, true, false]);
        assert_eq!(alpha(&scene, &body, "o_body_a"), Some([1.0, 1.0]));
    }
}
