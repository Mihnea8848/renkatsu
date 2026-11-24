// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The Animator panel: the selected character's skeleton while she is
//! posed by hand (FK), each joint by its readable name, to choose a bone
//! as a click on its ball does.
//!
//! It edits the character's pose layer, plain values that keys will drive:
//! with the scene's timeline the character's FK and IK animation plays as
//! her track beside the map's objects, the effects, lights and cameras,
//! and this panel shows each bone's keys at the timeline's time.

use crate::panels::toolbar;
use crate::scene::Content;
use crate::state::{Look, State};
use crate::widgets::{self, glyph, hair_h, hair_v};
use egui::{Align2, Color32, Pos2, Rect, Sense, Ui, Vec2};
use egui_phosphor::regular as icon;
use rk_chara::keys::Ease;
use rk_chara::pose::BoneEdit;

/// What the Animator's bar asks for.
enum BarAct {
    Solo,
    Save,
    Load(String),
    Clear,
}

/// What a menu of the Animator asks for.
enum KeyAct {
    Copy(BoneEdit),
    Move(String, f32),
    Ease(String, f32, Ease),
    Delete(String, f32),
    Set(String),
    Paste(String),
    DeleteBone(String),
}

/// Draws the panel.
pub fn show(ui: &mut Ui, k: &Look, st: &mut State) {
    let t = &k.t;
    let character = st
        .doc
        .selection()
        .filter(|o| matches!(o.content, Content::Character { .. }))
        .map(|o| (o.id, o.name.clone()));
    let fk_on = matches!(
        st.doc.selection().map(|o| &o.content),
        Some(Content::Character { pose_layer: Some(l), .. }) if l.fk_on
    );
    // What the bar asks for: playing her keys alone, and its menu.
    let has_keys = character
        .as_ref()
        .and_then(|(id, _)| st.doc.scene().sequence.track(*id))
        .is_some_and(|t| !t.keys.is_empty());
    let solo_here = character.as_ref().is_some_and(|(id, _)| st.solo == Some(*id));
    let saved_keys: Vec<String> = {
        let mut files = Vec::new();
        crate::project::files_below(&st.project.folder(crate::project::ANIMATIONS), &mut files);
        let mut rels: Vec<String> = files
            .iter()
            .filter_map(|f| crate::project::relative_to(&st.project.root, f))
            .filter(|r| crate::sequence::is_keys(r))
            .collect();
        rels.sort_by_key(|r| r.to_lowercase());
        rels
    };
    let mut bar_act: Option<BarAct> = None;
    toolbar(ui, k, |ui| {
        if let Some((_, name)) = &character {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(140.0, k.field()), Sense::hover());
            glyph(
                ui.painter(),
                k,
                Pos2::new(rect.left() + 8.0, rect.center().y),
                crate::kinds::Kind::Character.glyph(),
                14.0,
                t.text,
            );
            widgets::label(
                ui.painter(),
                k,
                Pos2::new(rect.left() + 22.0, rect.center().y),
                name,
                t.text_bright,
            );
        }
        if character.is_some() {
            let (g, tip) = if solo_here {
                (icon::PAUSE, "Pause")
            } else {
                (icon::PLAY, "Play this character alone")
            };
            let resp = ui
                .add_enabled_ui(has_keys || solo_here, |ui| widgets::icon_button(ui, k, g, solo_here))
                .inner
                .on_hover_text(tip);
            if resp.clicked() {
                bar_act = Some(BarAct::Solo);
            }
            let more = widgets::icon_button(ui, k, icon::DOTS_THREE, false);
            egui::Popup::menu(&more)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    if widgets::menu_item(ui, k, icon::FLOPPY_DISK, "Save as animation", "", has_keys) {
                        bar_act = Some(BarAct::Save);
                        ui.close();
                    }
                    widgets::menu_sub(ui, k, icon::FILM_REEL, "Load animation", |ui| {
                        if saved_keys.is_empty() {
                            widgets::menu_item(ui, k, icon::FILM_REEL, "No saved animations", "", false);
                        }
                        for rel in &saved_keys {
                            if widgets::menu_item(ui, k, icon::FILM_REEL, crate::content::stem(rel), "", true) {
                                bar_act = Some(BarAct::Load(rel.clone()));
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    if widgets::menu_item(ui, k, icon::TRASH, "Clear keys", "", has_keys) {
                        bar_act = Some(BarAct::Clear);
                        ui.close();
                    }
                });
            crate::widgets::bar_divider(ui, k);
        }
        let w = ui.available_width();
        widgets::search(ui, k, &mut st.animator_search, "Search", w);
    });
    if let (Some(act), Some((id, _))) = (bar_act, &character) {
        let id = *id;
        match act {
            BarAct::Solo => {
                if solo_here {
                    // Paused where she is, so that frame can be edited.
                    let at = st.solo_time;
                    st.solo = None;
                    crate::timeline::go_to(st, at);
                } else {
                    st.solo = Some(id);
                    st.solo_time = st.clock.clamp(0.0, st.doc.scene().sequence.length);
                    if st.playback == crate::state::Playback::Playing {
                        st.playback = crate::state::Playback::Paused;
                    }
                }
            }
            BarAct::Save => st.run(crate::state::Command::SaveKeys),
            BarAct::Load(rel) => st.run(crate::state::Command::LoadKeys(rel)),
            BarAct::Clear => crate::keying::clear_keys(st, id),
        }
    }
    let area = ui.available_rect_before_wrap();
    let say = |ui: &Ui, text: &str| {
        ui.painter()
            .text(area.center(), Align2::CENTER_CENTER, text, k.font(), t.text_dim);
    };
    let Some((id, _)) = character else {
        say(ui, "Select a character");
        return;
    };
    // IK's targets while it is on, then the skeleton while FK is.
    let ik_groups: Option<Vec<bool>> = match st.doc.selection().map(|o| &o.content) {
        Some(Content::Character {
            pose_layer: Some(l), ..
        }) if l.ik_on && l.ik.len() == rk_chara::pose::IK_TARGETS => {
            Some((0..rk_chara::pose::IK_TARGETS).map(|i| l.ik_group_on(i)).collect())
        }
        _ => None,
    };
    if !fk_on && ik_groups.is_none() {
        say(ui, "Forward kinematics is off");
        return;
    }
    let Some(joints) = st.live.as_ref().and_then(|l| l.joints(id)) else {
        say(ui, "The character is not built");
        return;
    };
    let needle = st.animator_search.trim().to_lowercase();
    let targets = ik_groups.iter().flat_map(|on| {
        (0..rk_chara::pose::IK_TARGETS)
            .filter(|i| on[*i])
            .map(|i| (0, rk_chara::pose::ik_key(i), rk_chara::pose::ik_label(i).to_owned()))
    });
    let bones = joints
        .iter()
        .filter(|_| fk_on)
        .map(|(depth, name)| (*depth, name.clone(), crate::bones::pose_label(name)));
    let rows: Vec<(usize, String, String)> = targets
        .chain(bones)
        .filter(|(_, name, shown)| {
            needle.is_empty() || shown.to_lowercase().contains(&needle) || name.to_lowercase().contains(&needle)
        })
        .collect();

    // The time the keys lie in: the scene's sequence; the playhead at the
    // scene's time.
    let pass = st.doc.scene().sequence.length.max(0.1);
    let now = if st.solo == Some(id) { st.solo_time } else { st.clock }.clamp(0.0, pass);
    let keys = st
        .doc
        .scene()
        .sequence
        .track(id)
        .map(|t| t.keys.clone())
        .unwrap_or_default();

    let area = ui.available_rect_before_wrap();
    let tree_w = (area.width() * 0.3).clamp(200.0, 320.0);
    let (ruler, scrub) = ui.allocate_exact_size(Vec2::new(area.width(), 22.0), Sense::click_and_drag());
    let sheet_x = |r: Rect| (r.left() + tree_w + 8.0, r.right() - 12.0);
    let (x0, x1) = sheet_x(ruler);
    let at = |time: f32| x0 + (x1 - x0) * (time / pass);
    let time_at = |x: f32| (x - x0) / (x1 - x0).max(1.0) * pass;
    // A click or a drag on the ruler moves the playhead, on the frames.
    let mut go_to: Option<f32> = None;
    if scrub.clicked() || scrub.dragged() {
        if let Some(pos) = scrub.interact_pointer_pos().filter(|p| p.x >= x0 - 6.0) {
            go_to = Some(time_at(pos.x));
        }
    }
    if scrub.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    // A tick each second, a finer one each quarter while there is room.
    let per_second = (x1 - x0) / pass;
    let step = if per_second > 160.0 {
        0.25
    } else if per_second > 60.0 {
        0.5
    } else {
        1.0
    };
    {
        let p = ui.painter();
        p.rect_filled(ruler, 0.0, t.header);
        hair_h(p, ruler.left(), ruler.right(), ruler.bottom() - 0.5, t.divider);
        hair_v(p, ruler.left() + tree_w, ruler.top(), ruler.bottom(), t.divider);
        let ticks = (pass / step).floor() as usize;
        for i in 0..=ticks {
            let time = i as f32 * step;
            let x = at(time).round() + 0.5;
            let whole = (time - time.round()).abs() < 1e-4;
            let top = if whole { ruler.top() + 6.0 } else { ruler.bottom() - 6.0 };
            hair_v(p, x, top, ruler.bottom(), if whole { t.text_dim } else { t.border });
            if whole {
                p.text(
                    Pos2::new(x + 3.0, ruler.top() + 2.0),
                    Align2::LEFT_TOP,
                    format!("{time:.0}"),
                    k.small(),
                    t.text_dim,
                );
            }
        }
    }

    let mut chosen: Option<String> = None;
    let mut acts: Vec<KeyAct> = Vec::new();
    let listed = egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (i, (depth, name, shown)) in rows.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row), Sense::click());
            let target = rk_chara::pose::ik_key_target(name);
            let on = match target {
                Some(i) => st.pose_ik == Some(i),
                None => st.pose_ik.is_none() && st.pose_bone.as_deref() == Some(name.as_str()),
            };
            let p = ui.painter();
            let tree = Rect::from_min_max(rect.min, Pos2::new(rect.left() + tree_w, rect.bottom()));
            // The bone's row of the sheet, every other one a shade darker.
            let (sx0, sx1) = sheet_x(rect);
            let sheet = Rect::from_min_max(Pos2::new(sx0, rect.top()), Pos2::new(sx1, rect.bottom()));
            if i % 2 == 1 {
                p.rect_filled(sheet, 0.0, Color32::from_black_alpha(28));
            }
            if on {
                p.rect_filled(tree, 0.0, t.select);
                p.rect_filled(sheet, 0.0, Color32::from_white_alpha(10));
            } else if resp.hovered() {
                p.rect_filled(tree, 0.0, t.row_hover);
            }
            hair_v(p, rect.left() + tree_w, rect.top(), rect.bottom(), t.divider);
            let indent = if needle.is_empty() { *depth as f32 * 14.0 } else { 0.0 };
            let x = rect.left() + 10.0 + indent;
            let y = rect.center().y;
            let (text, faint) = if on {
                (Color32::WHITE, Color32::from_white_alpha(170))
            } else {
                (t.text, t.text_dim)
            };
            let mark = if target.is_some() { icon::TARGET } else { icon::BONE };
            glyph(p, k, Pos2::new(x + 6.0, y), mark, 13.0, faint);
            widgets::label(p, k, Pos2::new(x + 18.0, y), shown, text);
            // The bone's keys, a diamond each; a click goes to the key.
            for key in keys.get(name.as_str()).into_iter().flatten() {
                let centre = Pos2::new(at(key.time).round() + 0.5, y);
                let hit = Rect::from_center_size(centre, Vec2::splat(12.0));
                let resp = ui.interact(
                    hit,
                    ui.id().with(("key", name.as_str(), key.time.to_bits())),
                    Sense::click(),
                );
                let r = if resp.hovered() { 5.5 } else { 4.5 };
                let fill = if on {
                    t.accent
                } else if resp.hovered() {
                    t.text_bright
                } else {
                    t.text
                };
                crate::timeline::diamond(ui.painter(), centre, r, fill);
                if resp.clicked() {
                    chosen = Some((*name).clone());
                    go_to = Some(key.time);
                }
                let (bone, time) = ((*name).clone(), key.time);
                resp.context_menu(|ui| {
                    if widgets::menu_item(ui, k, icon::COPY, "Copy key", "", true) {
                        acts.push(KeyAct::Copy(key.edit));
                        ui.close();
                    }
                    if widgets::menu_item(ui, k, icon::ARROWS_HORIZONTAL, "Move to playhead", "", true) {
                        acts.push(KeyAct::Move(bone.clone(), time));
                        ui.close();
                    }
                    widgets::menu_sub(ui, k, icon::WAVE_SINE, "Ease", |ui| {
                        for (ease, text) in [(Ease::Smooth, "Smooth"), (Ease::Linear, "Linear"), (Ease::Step, "Hold")] {
                            if widgets::check_item(ui, k, key.ease == ease, text, "").clicked() {
                                acts.push(KeyAct::Ease(bone.clone(), time, ease));
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    if widgets::menu_item(ui, k, icon::TRASH, "Delete key", "", true) {
                        acts.push(KeyAct::Delete(bone.clone(), time));
                        ui.close();
                    }
                });
            }
            let resp = if shown != name {
                resp.on_hover_text(name.as_str())
            } else {
                resp
            };
            if resp.clicked() {
                chosen = Some((*name).clone());
            }
            let keyed = keys.contains_key(name.as_str());
            let can_paste = st.key_clipboard.is_some();
            resp.context_menu(|ui| {
                let bone = (*name).clone();
                if widgets::menu_item(ui, k, icon::DIAMOND, "Set key", "", true) {
                    acts.push(KeyAct::Set(bone.clone()));
                    ui.close();
                }
                if widgets::menu_item(ui, k, icon::CLIPBOARD, "Paste key", "", can_paste) {
                    acts.push(KeyAct::Paste(bone.clone()));
                    ui.close();
                }
                ui.separator();
                if widgets::menu_item(ui, k, icon::TRASH, "Delete keys of this bone", "", keyed) {
                    acts.push(KeyAct::DeleteBone(bone));
                    ui.close();
                }
            });
        }
    });
    // The playhead, over the ruler and the rows.
    let x = at(now).round() + 0.5;
    let p = ui.painter();
    p.vline(
        x,
        ruler.top()..=listed.inner_rect.bottom(),
        egui::Stroke::new(1.5, t.accent),
    );
    if let Some(name) = chosen {
        match rk_chara::pose::ik_key_target(&name) {
            Some(i) => {
                st.pose_ik = Some(i);
                st.pose_bone = None;
            }
            None => {
                st.pose_bone = Some(name);
                st.pose_ik = None;
                st.property_tab = 1;
            }
        }
    }
    // Moving the playhead holds the scene there, and ends playing alone.
    if let Some(time) = go_to {
        st.solo = None;
        crate::timeline::go_to(st, time);
    }
    // What the menus asked for, the keys at the playhead where they act
    // at a time.
    for act in acts {
        match act {
            KeyAct::Copy(edit) => st.key_clipboard = Some(edit),
            KeyAct::Move(bone, from) => {
                let to = st.clock;
                crate::keying::move_key(st, id, &bone, from, to);
            }
            KeyAct::Ease(bone, time, ease) => crate::keying::set_ease(st, id, &bone, time, ease),
            KeyAct::Delete(bone, time) => crate::keying::delete_key_at(st, id, &bone, time),
            KeyAct::Set(bone) => crate::keying::set_keys(st, id, &[bone]),
            KeyAct::Paste(bone) => {
                if let Some(edit) = st.key_clipboard {
                    crate::keying::paste_key(st, id, &bone, edit);
                }
            }
            KeyAct::DeleteBone(bone) => crate::keying::delete_bone_keys(st, id, &bone),
        }
    }
}
