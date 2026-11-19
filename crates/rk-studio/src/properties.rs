// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The Properties panel: everything about the selection in sections of
//! rows, with names and values aligned in two columns. The values are the
//! selected object's own, from the scene model; what waits for a later
//! feature is shown greyed, and says what it waits for.

use crate::design::Props;
use crate::kinds::Kind;
use crate::profile::{self, Profile};
use crate::scene::{Content, Place};
use crate::state::{Look, State};
use crate::widgets::{self, glyph, hair_h, hair_v, icon_button, label, Marker};
use egui::{Align, Align2, Color32, Id, Layout, Pos2, Rect, Sense, Ui, UiBuilder, Vec2};
use egui_phosphor::regular as icon;

const AXES: [&str; 3] = ["X", "Y", "Z"];

/// How far a number moves per point of drag: positions and scales, then
/// angles in degrees.
const FINE: f32 = 0.005;
const ANGLE: f32 = 0.5;

pub fn show(ui: &mut Ui, k: &Look, st: &mut State) {
    let t = &k.t;
    let Some(before) = st.doc.selection().cloned() else {
        let rect = ui.available_rect_before_wrap();
        ui.painter().text(
            rect.center_top() + Vec2::new(0.0, 40.0),
            Align2::CENTER_CENTER,
            "Nothing selected",
            k.font(),
            t.text_dim,
        );
        return;
    };
    // The rows edit a copy; a copy that changed goes back to the scene as
    // one step of the history.
    let mut object = before.clone();
    let kind = object.kind();
    header(ui, k, st, kind, &object.name);
    // Who a character is: the card's profile, or the one the character
    // has in this scene. Its pages sit beside the scene's settings.
    let card_profile = match &object.content {
        Content::Character { card, .. } => st.profile(card),
        _ => None,
    };
    let mut shown = card_profile.as_ref().map(|card| {
        let mut p = match &object.content {
            Content::Character { profile: Some(own), .. } => (**own).clone(),
            _ => card.clone(),
        };
        p.male = card.male;
        p
    });
    let shown_before = shown.clone();
    let pages: &[&str] = match &shown {
        Some(p) if p.male => &["Scene", "Profile"],
        Some(_) => &["Scene", "Profile", "Likes", "H Traits"],
        None => &[],
    };
    if pages.is_empty() {
        st.property_tab = 0;
    } else {
        st.property_tab = st.property_tab.min(pages.len() - 1);
        page_tabs(ui, k, &mut st.property_tab, pages);
    }
    let page = st.property_tab;
    let built = st.live.as_ref().and_then(|live| live.built(object.id));
    let mut show_animations = false;
    let mut show_cards = false;
    let failure = st
        .live
        .as_ref()
        .and_then(|live| live.failure(object.id))
        .map(str::to_owned);
    let lights = st.doc.scene().key_light().map(|o| o.id) == Some(object.id);
    let holds = st.doc.scene().subtree(object.id).len() - 1;
    // What the object is attached to: a folder only holds it.
    let target = before
        .parent
        .and_then(|parent| st.doc.scene().get(parent))
        .filter(|parent| parent.kind() != Kind::Folder)
        .map(|parent| (parent.id, parent.name.clone(), parent.kind()));
    let mut detach = false;
    let mut bone: Option<Option<String>> = None;

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        if let (Some(p), 1..) = (shown.as_mut(), page) {
            if let Some(card) = &card_profile {
                profile_page(ui, k, st, p, card, page);
            }
            return;
        }
        let id = ui.id();
        if kind != Kind::Folder && section(ui, k, st, 0, "Transform") {
            // What each of position, rotation and scale waits for, when it
            // does nothing yet for this kind of object.
            let off: [Option<&str>; 3] = match &object.content {
                Content::Character { .. } => [None; 3],
                Content::Light { from_camera, .. } => [
                    Some("A directional light has no position"),
                    (*from_camera).then_some("The light follows the camera"),
                    Some("A light has no size"),
                ],
                Content::Camera { .. } => [Some("Not available yet"); 3],
                _ => [Some("Not available yet"); 3],
            };
            let transform = &mut object.transform;
            let values = [
                ("Position", &mut transform.position, FINE),
                ("Rotation", &mut transform.rotation, ANGLE),
                ("Scale", &mut transform.scale, FINE),
            ];
            for (i, (name, value, speed)) in values.into_iter().enumerate() {
                row(ui, k, name, off[i], |ui, rect| {
                    vec3(ui, k, rect, id.with(("transform", i)), value, speed);
                });
            }
            section_end(ui, k);
        }
        if let Some((to, name, target_kind)) = &target {
            if section(ui, k, st, 15, "Attached to") {
                row(ui, k, "Object", None, |ui, rect| {
                    let w = widgets::text_width(ui, "Detach", k.font()).ceil() + 18.0;
                    let field = rect.with_max_x(rect.right() - w - 4.0);
                    widgets::value_box(ui, k, field, name);
                    let button = rect.with_min_x(field.right() + 4.0);
                    let resp = ui.interact(button, id.with("detach"), Sense::click());
                    let fill = if resp.hovered() { k.t.hover } else { k.t.control };
                    ui.painter().rect_filled(button, k.radius(), fill);
                    ui.painter()
                        .text(button.center(), Align2::CENTER_CENTER, "Detach", k.font(), k.t.text);
                    detach = resp.clicked();
                });
                if *target_kind == Kind::Character {
                    row(ui, k, "Bone", None, |ui, rect| {
                        let shown = before.bone.as_deref().map_or("None", crate::bones::label);
                        let resp = drop_field(ui, k, rect, id.with("bone"), shown);
                        egui::Popup::menu(&resp)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                let all = st.live.as_ref().map(|live| live.bone_names(*to)).unwrap_or_default();
                                bone = crate::bones::menu(ui, k, &mut st.bone_search, &all, before.bone.as_deref());
                            });
                    });
                }
                section_end(ui, k);
            }
        }
        match &mut object.content {
            Content::Character {
                card,
                outfit,
                outdoor_shoes,
                simulate,
                clothes,
                animation,
                ..
            } => {
                if section(ui, k, st, 1, "Character") {
                    row(ui, k, "Card", None, |ui, rect| {
                        let side = rect.height();
                        let field = Rect::from_min_max(rect.min, Pos2::new(rect.right() - side - 4.0, rect.bottom()));
                        // The card by its name in the project, or by its file.
                        let file = match st.library.characters.iter().find(|c| c.file == *card) {
                            Some(entry) => entry.name.clone(),
                            None => card.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                        };
                        // A dropdown of the project's cards.
                        let resp = ui.interact(field, id.with("card"), Sense::click());
                        let p = ui.painter();
                        let fill = if resp.hovered() { k.t.hover } else { k.t.control };
                        p.rect_filled(field, k.radius(), fill);
                        let color = if resp.hovered() { k.t.text_bright } else { k.t.text };
                        p.with_clip_rect(field.with_max_x(field.right() - 22.0)).text(
                            Pos2::new(field.left() + 8.0, field.center().y),
                            Align2::LEFT_CENTER,
                            &file,
                            k.font(),
                            color,
                        );
                        glyph(
                            p,
                            k,
                            Pos2::new(field.right() - 12.0, field.center().y + 1.0),
                            icon::CARET_DOWN,
                            10.0,
                            color,
                        );
                        egui::Popup::menu(&resp).show(|ui| {
                            ui.set_min_width(field.width() - 10.0);
                            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                                for entry in &st.library.characters {
                                    // The folder tells cards of one name apart.
                                    let rel = entry.file.to_string_lossy().replace('\\', "/");
                                    let folder = crate::content::parent(&rel)
                                        .strip_prefix(crate::project::CHARACTERS)
                                        .unwrap_or_default()
                                        .trim_start_matches('/')
                                        .replace('/', " / ");
                                    if widgets::check_item(ui, k, entry.file == *card, &entry.name, &folder).clicked() {
                                        *card = entry.file.clone();
                                        ui.close();
                                    }
                                }
                            });
                        });
                        resp.on_hover_text(card.display().to_string());
                        let button = Rect::from_min_size(Pos2::new(field.right() + 4.0, rect.top()), Vec2::splat(side));
                        let resp = ui
                            .interact(button, id.with("browse"), Sense::click())
                            .on_hover_text("Show in Content");
                        let fill = if resp.hovered() { k.t.hover } else { k.t.control };
                        ui.painter().rect_filled(button, k.radius(), fill);
                        glyph(ui.painter(), k, button.center(), icon::FOLDER_OPEN, 14.0, k.t.text);
                        if resp.clicked() {
                            show_cards = true;
                        }
                    });
                    let outfits: Vec<String> = (0..built.as_ref().map_or(1, |b| b.outfits.max(1)))
                        .map(outfit_name)
                        .collect();
                    let outfits: Vec<&str> = outfits.iter().map(String::as_str).collect();
                    let waiting = built.is_none().then_some("The character is not built");
                    row(ui, k, "Outfit", waiting, |ui, rect| {
                        widgets::combo(ui, k, rect, id.with("outfit"), outfit, &outfits);
                    });
                    row(ui, k, "Shoes", waiting, |ui, rect| {
                        let mut chosen = usize::from(*outdoor_shoes);
                        widgets::segmented(ui, k, rect, id.with("shoes"), &mut chosen, &["Indoor", "Outdoor"]);
                        *outdoor_shoes = chosen == 1;
                    });
                    row(ui, k, "Physics", None, |ui, rect| {
                        widgets::checkbox(ui, k, rect, id.with("physics"), simulate, "Simulate");
                    });
                    if let Some(why) = &failure {
                        row(ui, k, "Problem", None, |ui, rect| {
                            ui.interact(rect, id.with("problem"), Sense::hover()).on_hover_text(why);
                            let text = format!("Could not be built: {why}");
                            label(&ui.painter().with_clip_rect(rect), k, rect.left_center(), &text, t.stop);
                        });
                    } else if let Some(count) = built.as_ref().map(|b| b.warnings).filter(|n| *n > 0) {
                        row(ui, k, "Warnings", None, |ui, rect| {
                            let text = count.to_string();
                            label(
                                &ui.painter().with_clip_rect(rect),
                                k,
                                rect.left_center(),
                                &text,
                                t.accent,
                            );
                        });
                    }
                    section_end(ui, k);
                }
                if section(ui, k, st, 5, "Clothing") {
                    use rk_chara::clothes_state as cs;
                    let worn = built.as_ref().map(|b| b.worn.clone());
                    let waiting = worn.is_none().then_some("The character is not built");
                    row(ui, k, "All", waiting, |ui, rect| {
                        let options = [("All on", true), ("All half off", true), ("All off", true)];
                        let clicked = choices(ui, k, rect, id.with("clothes all"), None, &options);
                        if let (Some(i), Some(worn)) = (clicked, &worn) {
                            cs::set_all(clothes, worn, [cs::ON, cs::HALF, cs::OFF][i]);
                        }
                    });
                    // The pair of shoes that is worn stands for both.
                    let shoes = if *outdoor_shoes {
                        (cs::SHOES_OUTDOOR, "Outdoor shoes")
                    } else {
                        (cs::SHOES_INDOOR, "Indoor shoes")
                    };
                    let rows = [
                        (cs::TOP, "Top"),
                        (cs::BOTTOM, "Bottom"),
                        (cs::BRA, "Bra"),
                        (cs::SHORTS, "Underwear"),
                        (cs::GLOVES, "Gloves"),
                        (cs::PANTYHOSE, "Pantyhose"),
                        (cs::SOCKS, "Socks"),
                        shoes,
                    ];
                    let kinds = worn.as_ref().map(cs::Worn::kinds).unwrap_or_default();
                    let shown = worn.as_ref().map_or(*clothes, |w| cs::fitted(clothes, w));
                    // Only what is worn gets a row; the rest is named on
                    // one line at the end.
                    let mut missing = Vec::new();
                    let half_2 = rows.iter().any(|&(kind, _)| kinds.has(kind, cs::HALF_2));
                    for (kind, name) in rows {
                        if worn.is_some() && !kinds.is_kind(kind) {
                            missing.push(name);
                            continue;
                        }
                        row(ui, k, name, waiting, |ui, rect| {
                            // Half 2 gets a place only when something worn
                            // has it.
                            let states: &[(u8, &str)] = if half_2 {
                                &[
                                    (cs::ON, "On"),
                                    (cs::HALF, "Half"),
                                    (cs::HALF_2, "Half 2"),
                                    (cs::OFF, "Off"),
                                ]
                            } else {
                                &[(cs::ON, "On"), (cs::HALF, "Half"), (cs::OFF, "Off")]
                            };
                            let options: Vec<(&str, bool)> =
                                states.iter().map(|&(s, name)| (name, kinds.has(kind, s))).collect();
                            let chosen = states.iter().position(|&(s, _)| s == shown[kind]);
                            let clicked = choices(ui, k, rect, id.with(("clothes", kind)), chosen, &options);
                            // A state the piece lacks moves on to the next
                            // one it has, as CharaStudio's buttons do.
                            if let (Some(i), Some(worn)) = (clicked, &worn) {
                                *clothes = shown;
                                cs::set_state(clothes, worn, kind, states[i].0, true);
                            }
                        });
                    }
                    if !missing.is_empty() {
                        row(ui, k, "Not worn", None, |ui, rect| {
                            let text = missing.join(", ");
                            label(
                                &ui.painter().with_clip_rect(rect),
                                k,
                                rect.left_center(),
                                &text,
                                t.text_dim,
                            );
                        });
                    }
                    section_end(ui, k);
                }
                if section(ui, k, st, 6, "Animation") {
                    let playing = built.as_ref().and_then(|b| b.animation);
                    row(ui, k, "Animation", None, |ui, rect| {
                        let side = rect.height();
                        let field = Rect::from_min_max(rect.min, Pos2::new(rect.right() - side - 4.0, rect.bottom()));
                        // A dropdown of the project's animations, by the
                        // folders the user keeps them in, and none.
                        let resp = ui.interact(field, id.with("animation"), Sense::click());
                        let p = ui.painter();
                        let fill = if resp.hovered() { k.t.hover } else { k.t.control };
                        p.rect_filled(field, k.radius(), fill);
                        let color = if resp.hovered() { k.t.text_bright } else { k.t.text };
                        let name = animation.as_ref().map_or("T-pose", |a| a.name.as_str());
                        p.with_clip_rect(field.with_max_x(field.right() - 22.0)).text(
                            Pos2::new(field.left() + 8.0, field.center().y),
                            Align2::LEFT_CENTER,
                            name,
                            k.font(),
                            color,
                        );
                        glyph(
                            p,
                            k,
                            Pos2::new(field.right() - 12.0, field.center().y + 1.0),
                            icon::CARET_DOWN,
                            10.0,
                            color,
                        );
                        egui::Popup::menu(&resp).show(|ui| {
                            ui.set_min_width(field.width() - 10.0);
                            if let Some(chosen) = crate::panels::animation_items(ui, k, st, Some(before.id)) {
                                *animation = chosen.map(Box::new);
                            }
                        });
                        if let Some(a) = animation.as_ref() {
                            let from = match &a.file {
                                Some(file) => file.clone(),
                                None => format!("{} of {} in {}", a.state, a.controller, a.bundle),
                            };
                            resp.on_hover_text(from);
                        }
                        let at = Rect::from_min_size(Pos2::new(field.right() + 4.0, rect.top()), Vec2::splat(side));
                        let browse = ui
                            .interact(at, id.with("browse animations"), Sense::click())
                            .on_hover_text("Show in Content");
                        let fill = if browse.hovered() { k.t.hover } else { k.t.control };
                        ui.painter().rect_filled(at, k.radius(), fill);
                        glyph(ui.painter(), k, at.center(), icon::FOLDER_OPEN, 14.0, k.t.text);
                        if browse.clicked() {
                            show_animations = true;
                        }
                    });
                    if let Some(why) = built.as_ref().and_then(|b| b.animation_problem.clone()) {
                        row(ui, k, "Problem", None, |ui, rect| {
                            ui.interact(rect, id.with("animation problem"), Sense::hover())
                                .on_hover_text(&why);
                            let text = format!("Could not be read: {why}");
                            label(&ui.painter().with_clip_rect(rect), k, rect.left_center(), &text, t.stop);
                        });
                    }
                    if let Some(p) = animation.as_mut() {
                        let fixed = playing.is_some_and(|(fixed, ..)| fixed);
                        row(ui, k, "Kind", None, |ui, rect| {
                            let text = match playing {
                                None => "Not read".to_owned(),
                                Some((true, ..)) => "Static".to_owned(),
                                Some((_, true, seconds)) => format!("Moving: loops every {seconds:.1} s"),
                                Some((_, false, seconds)) => format!("Moving: plays {seconds:.1} s once"),
                            };
                            label(
                                &ui.painter().with_clip_rect(rect),
                                k,
                                rect.left_center(),
                                &text,
                                t.text_dim,
                            );
                        });
                        let still = fixed.then_some("A static animation does not move");
                        row(ui, k, "Speed", still, |ui, rect| {
                            widgets::number(
                                ui,
                                k,
                                rect,
                                id.with("animation speed"),
                                &mut p.speed,
                                Marker::None,
                                true,
                                FINE,
                            );
                            p.speed = p.speed.clamp(0.0, 4.0);
                        });
                        row(ui, k, "Start", still, |ui, rect| {
                            widgets::number(
                                ui,
                                k,
                                rect,
                                id.with("animation start"),
                                &mut p.start,
                                Marker::None,
                                true,
                                FINE,
                            );
                            p.start = p.start.clamp(0.0, 1.0);
                        });
                        let loops = playing.is_some_and(|(_, loops, _)| loops);
                        let why = still.or(loops.then_some("The animation loops by itself"));
                        row(ui, k, "At the end", why, |ui, rect| {
                            widgets::checkbox(ui, k, rect, id.with("animation loop"), &mut p.force_loop, "Start over");
                        });
                    }
                    section_end(ui, k);
                }
                if section(ui, k, st, 2, "Expression") {
                    let later = Some("Not available yet");
                    for name in ["Eyes", "Mouth"] {
                        row(ui, k, name, later, |ui, rect| {
                            widgets::combo(ui, k, rect, id.with(name), &mut 0, &["Default"]);
                        });
                    }
                    section_end(ui, k);
                }
                if section(ui, k, st, 3, "Modules") {
                    let later = Some("Not available yet");
                    for name in ["MaterialEditor", "ABMX", "Stiletto", "Clothing Unlocker"] {
                        row(ui, k, name, later, |ui, rect| {
                            widgets::checkbox(ui, k, rect, id.with(name), &mut true, "");
                        });
                    }
                    section_end(ui, k);
                }
            }
            Content::Light {
                from_camera,
                intensity,
                shadows,
            } => {
                if section(ui, k, st, 1, "Light") {
                    row(ui, k, "Type", Some("Not available yet"), |ui, rect| {
                        widgets::combo(ui, k, rect, id.with("light"), &mut 0, &["Directional"]);
                    });
                    row(ui, k, "Colour", Some("Not available yet"), |ui, rect| {
                        widgets::value_box(ui, k, rect, "       FFFFFF");
                        let swatch =
                            Rect::from_min_size(rect.min + Vec2::new(4.0, 4.0), Vec2::new(16.0, rect.height() - 8.0));
                        ui.painter().rect_filled(swatch, 1.0, Color32::WHITE);
                    });
                    row(ui, k, "Direction", None, |ui, rect| {
                        let mut chosen = usize::from(!*from_camera);
                        let options = ["From camera", "Own"];
                        widgets::segmented(ui, k, rect, id.with("direction"), &mut chosen, &options);
                        *from_camera = chosen == 0;
                    });
                    row(ui, k, "Intensity", None, |ui, rect| {
                        widgets::number(ui, k, rect, id.with("intensity"), intensity, Marker::None, true, FINE);
                        *intensity = intensity.clamp(0.0, 2.0);
                    });
                    row(ui, k, "Shadows", None, |ui, rect| {
                        widgets::checkbox(ui, k, rect, id.with("shadows"), shadows, "Cast");
                    });
                    if !lights {
                        row(ui, k, "In use", None, |ui, rect| {
                            let text = "No";
                            ui.interact(rect, id.with("in-use"), Sense::hover())
                                .on_hover_text("Only one light is used");
                            label(
                                &ui.painter().with_clip_rect(rect),
                                k,
                                rect.left_center(),
                                text,
                                t.text_dim,
                            );
                        });
                    }
                    section_end(ui, k);
                }
            }
            Content::Camera {
                focal_length,
                near,
                far,
            } => {
                if section(ui, k, st, 1, "Lens") {
                    let later = Some("Not available yet");
                    let values = [("Focal length", focal_length), ("Near", near), ("Far", far)];
                    for (i, (name, value)) in values.into_iter().enumerate() {
                        row(ui, k, name, later, |ui, rect| {
                            widgets::number(ui, k, rect, id.with(("lens", i)), value, Marker::None, true, FINE);
                        });
                    }
                    section_end(ui, k);
                }
            }
            Content::Folder | Content::Item | Content::Map => {
                if section(ui, k, st, 1, "Object") {
                    row(ui, k, "Name", None, |ui, rect| {
                        widgets::value_box(ui, k, rect, &before.name)
                    });
                    row(ui, k, "Type", None, |ui, rect| {
                        label(ui.painter(), k, rect.left_center(), kind.label(), t.text_dim);
                    });
                    if kind == Kind::Folder {
                        row(ui, k, "Holds", None, |ui, rect| {
                            let text = match holds {
                                1 => "1 object".to_owned(),
                                n => format!("{n} objects"),
                            };
                            label(ui.painter(), k, rect.left_center(), &text, t.text_dim);
                        });
                    }
                    section_end(ui, k);
                }
            }
        }
    });
    if let Content::Character { profile, .. } = &mut object.content {
        // Values that are all the card's again are the card's.
        if shown != shown_before {
            *profile = shown.filter(|p| Some(p) != card_profile.as_ref()).map(Box::new);
        }
    }
    if object != before {
        let held = ui.input(|i| i.pointer.any_down());
        st.doc.edit(object, held);
    }
    if detach {
        st.doc.place_many(&[before.id], Place::Into(None));
    }
    if let (Some(bone), Some((to, ..))) = (bone, &target) {
        st.attach(&[before.id], *to, bone.as_deref());
    }
    if show_cards {
        st.show_folder(0);
    }
    if show_animations {
        st.show_folder(1);
    }
}

/// A field that opens a list: what is chosen and a caret.
fn drop_field(ui: &mut Ui, k: &Look, rect: Rect, id: Id, text: &str) -> egui::Response {
    let resp = ui.interact(rect, id, Sense::click());
    let p = ui.painter();
    let fill = if resp.hovered() { k.t.hover } else { k.t.control };
    p.rect_filled(rect, k.radius(), fill);
    let color = if resp.hovered() { k.t.text_bright } else { k.t.text };
    p.with_clip_rect(rect.with_max_x(rect.right() - 22.0)).text(
        Pos2::new(rect.left() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        k.font(),
        color,
    );
    glyph(
        p,
        k,
        Pos2::new(rect.right() - 12.0, rect.center().y + 1.0),
        icon::CARET_DOWN,
        10.0,
        color,
    );
    resp
}

/// The game's names for a card's seven outfits, in their order; outfits a
/// plugin adds after them are numbered.
fn outfit_name(index: usize) -> String {
    const NAMES: [&str; 7] = [
        "In school",
        "After school",
        "Gym",
        "Swimsuit",
        "Club",
        "Casual",
        "Sleepover",
    ];
    NAMES
        .get(index)
        .map_or_else(|| format!("Outfit {}", index + 1), |name| (*name).to_owned())
}

/// The row of pages under a character's header, each a word with a line
/// under the open one.
fn page_tabs(ui: &mut Ui, k: &Look, chosen: &mut usize, pages: &[&str]) {
    let t = &k.t;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row + 4.0), Sense::hover());
    hair_h(ui.painter(), rect.left(), rect.right(), rect.bottom() - 1.0, t.line);
    let mut x = rect.left() + 8.0;
    for (i, name) in pages.iter().enumerate() {
        let w = widgets::text_width(ui, name, k.font()).ceil() + 18.0;
        let tab = Rect::from_min_max(Pos2::new(x, rect.top()), Pos2::new(x + w, rect.bottom()));
        let resp = ui.interact(tab, ui.id().with(("property page", i)), Sense::click());
        if resp.clicked() {
            *chosen = i;
        }
        let on = *chosen == i;
        let color = match (on, resp.hovered()) {
            (true, _) => t.text_bright,
            (false, true) => t.text,
            (false, false) => t.text_dim,
        };
        let p = ui.painter();
        p.text(
            tab.center() - Vec2::new(0.0, 1.0),
            Align2::CENTER_CENTER,
            name,
            k.font(),
            color,
        );
        if on {
            let line = Rect::from_min_max(
                Pos2::new(tab.left() + 4.0, tab.bottom() - 2.0),
                Pos2::new(tab.right() - 4.0, tab.bottom()),
            );
            p.rect_filled(line, 0.0, t.select);
        }
        x += w;
    }
}

/// Who the character is (page 1), what it likes (page 2) or what it is
/// like in bed (page 3). The values
/// start as the card's; changing one keeps the change in the scene, for
/// this character only. Each page ends with the way back to the card's
/// values for what it shows.
fn profile_page(ui: &mut Ui, k: &Look, st: &mut State, p: &mut Profile, card: &Profile, page: usize) {
    let id = ui.id().with("profile");
    if page == 1 {
        if section(ui, k, st, 7, "Name") {
            let names = [
                ("Last name", &mut p.last_name),
                ("First name", &mut p.first_name),
                ("Nickname", &mut p.nickname),
            ];
            for (name, value) in names {
                row(ui, k, name, None, |ui, rect| {
                    if let Some(typed) = widgets::text_field(ui, k, rect, id.with(name), value, "") {
                        *value = typed.trim().to_owned();
                    }
                });
            }
            section_end(ui, k);
        }
        if section(ui, k, st, 8, "About") {
            if !p.male {
                dropdown(ui, k, id, "Personality", &profile::PERSONALITIES, &mut p.personality);
                row(ui, k, "Voice pitch", None, |ui, rect| {
                    // Dragged in fractions, kept whole.
                    let pitch_id = id.with("pitch");
                    let mut exact = ui.data(|d| d.get_temp::<f32>(pitch_id)).unwrap_or(p.pitch);
                    if exact.round() != p.pitch {
                        exact = p.pitch;
                    }
                    widgets::number_to(ui, k, rect, pitch_id, &mut exact, Marker::None, true, 0.25, 0);
                    exact = exact.clamp(0.0, 100.0);
                    ui.data_mut(|d| d.insert_temp(pitch_id, exact));
                    p.pitch = exact.round();
                });
            }
            row(ui, k, "Birthday", None, |ui, rect| birthday(ui, k, rect, id, p));
            dropdown(ui, k, id, "Blood type", &profile::BLOOD_TYPES, &mut p.blood_type);
            if p.male {
                row(
                    ui,
                    k,
                    "Club",
                    Some("Male characters are always in the Koikatsu club"),
                    |ui, rect| {
                        widgets::value_box(ui, k, rect, "Koikatsu club");
                    },
                );
                const STEPS: [&str; 11] = ["-5", "-4", "-3", "-2", "-1", "0", "+1", "+2", "+3", "+4", "+5"];
                let stats = ["Aggressiveness", "Diligence", "Kindness"];
                for (name, n) in stats.into_iter().zip(p.stats.iter_mut()) {
                    row(ui, k, name, None, |ui, rect| {
                        let mut at = (*n + 5).clamp(0, 10) as usize;
                        widgets::combo(ui, k, rect, id.with(name), &mut at, &STEPS);
                        if at as i64 - 5 != (*n).clamp(-5, 5) {
                            *n = at as i64 - 5;
                        }
                    });
                }
            } else {
                dropdown(ui, k, id, "Club", &profile::CLUBS, &mut p.club);
            }
            section_end(ui, k);
        }
        if !p.male && section(ui, k, st, 9, "Traits") {
            let grid = Grid {
                items: &profile::TRAITS,
                hint: Some("Add custom field"),
            };
            check_grid(ui, k, id.with("traits"), grid, &mut p.traits, &mut p.custom_traits);
            section_end(ui, k);
        }
    } else if page == 2 {
        if section(ui, k, st, 10, "Likes") {
            let grid = Grid {
                items: &profile::LIKES,
                hint: Some("Add custom field"),
            };
            check_grid(ui, k, id.with("likes"), grid, &mut p.likes, &mut p.custom_likes);
            section_end(ui, k);
        }
    } else {
        if section(ui, k, st, 12, "Traits") {
            let grid = Grid {
                items: &profile::H_TRAITS,
                hint: Some("Add custom field"),
            };
            check_grid(
                ui,
                k,
                id.with("h traits"),
                grid,
                &mut p.h_traits,
                &mut p.custom_h_traits,
            );
            section_end(ui, k);
        }
        if section(ui, k, st, 13, "Refuses") {
            let grid = Grid {
                items: &profile::DENIAL,
                hint: None,
            };
            check_grid(ui, k, id.with("denial"), grid, &mut p.denial, &mut Vec::new());
            section_end(ui, k);
        }
        if section(ui, k, st, 14, "Weak point") {
            row(ui, k, "Weakest at", None, |ui, rect| {
                let names: Vec<&str> = std::iter::once("Not set").chain(profile::WEAK_POINTS).collect();
                let mut at = usize::try_from(p.weak_point + 1).unwrap_or(0).min(names.len() - 1);
                widgets::combo(ui, k, rect, id.with("weak point"), &mut at, &names);
                p.weak_point = at as i64 - 1;
            });
            section_end(ui, k);
        }
    }
    // The page's values as the card has them, with the other pages' left
    // as they are now.
    let mut from_card = p.clone();
    match page {
        2 => {
            from_card.likes = card.likes.clone();
            from_card.custom_likes = card.custom_likes.clone();
        }
        3 => {
            from_card.h_traits = card.h_traits.clone();
            from_card.custom_h_traits = card.custom_h_traits.clone();
            from_card.denial = card.denial.clone();
            from_card.weak_point = card.weak_point;
        }
        _ => {
            from_card = Profile {
                likes: p.likes.clone(),
                custom_likes: p.custom_likes.clone(),
                h_traits: p.h_traits.clone(),
                custom_h_traits: p.custom_h_traits.clone(),
                denial: p.denial.clone(),
                weak_point: p.weak_point,
                ..card.clone()
            };
        }
    }
    if section(ui, k, st, 11, "Card data") {
        let changed = from_card != *p;
        let why = (!changed).then_some("No changes");
        row(ui, k, "Values", why, |ui, rect| {
            if crate::selector::button(ui, k, rect, "Reset to card values", false, changed) {
                *p = from_card.clone();
            }
        });
        section_end(ui, k);
    }
}

/// A property chosen from one of the game's lists, kept as its number.
fn dropdown(ui: &mut Ui, k: &Look, id: Id, name: &str, list: &[&str], n: &mut i64) {
    row(ui, k, name, None, |ui, rect| {
        let (names, mut at) = profile::options(list, *n);
        let before = at;
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        widgets::combo(ui, k, rect, id.with(name), &mut at, &names);
        if at != before && at < list.len() {
            *n = at as i64;
        }
    });
}

/// The month and the day of a birthday, side by side.
fn birthday(ui: &mut Ui, k: &Look, rect: Rect, id: Id, p: &mut Profile) {
    let split = (rect.left() + rect.width() * 0.6).round();
    let month_box = rect.with_max_x(split - 2.0);
    let day_box = rect.with_min_x(split + 2.0);
    let months: Vec<&str> = std::iter::once("Not set").chain(profile::MONTHS).collect();
    let mut month = usize::try_from(p.birth_month).ok().filter(|&m| m <= 12).unwrap_or(0);
    widgets::combo(ui, k, month_box, id.with("month"), &mut month, &months);
    if month as i64 != p.birth_month {
        p.birth_month = month as i64;
    }
    if p.birth_month == 0 {
        p.birth_day = 0;
        widgets::value_box(ui, k, day_box, "");
        return;
    }
    p.birth_day = p.birth_day.clamp(1, p.days());
    let days: Vec<String> = (1..=p.days()).map(|d| d.to_string()).collect();
    let days: Vec<&str> = days.iter().map(String::as_str).collect();
    let mut day = (p.birth_day - 1) as usize;
    widgets::combo(ui, k, day_box, id.with("day"), &mut day, &days);
    p.birth_day = day as i64 + 1;
}

/// A list of check boxes: the game's, by key and name, and what the field
/// at its end says while empty.
struct Grid<'a> {
    items: &'a [(&'a str, &'a str)],
    /// What the field that adds one says while empty, or `None` for a
    /// list that takes no additions.
    hint: Option<&'a str>,
}

/// Check boxes in two columns: the game's items, ticked for the keys in
/// `keys`, then the ones added for this character, marked with a star and
/// with a button that takes them away, then a field that adds another.
fn check_grid(ui: &mut Ui, k: &Look, id: Id, grid: Grid, keys: &mut Vec<String>, custom: &mut Vec<profile::Custom>) {
    let t = &k.t;
    let (pad, h) = (14.0, k.d.row + 4.0);
    let width = ui.available_width();
    let cells = grid.items.len() + custom.len() + usize::from(grid.hint.is_some());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, cells.div_ceil(2) as f32 * h + 16.0), Sense::hover());
    let column = ((width - 2.0 * pad) / 2.0).floor();
    let cell = |i: usize| {
        let top = rect.top() + 4.0 + (i / 2) as f32 * h + (h - k.field()) / 2.0;
        let left = rect.left() + pad + (i % 2) as f32 * column;
        Rect::from_min_size(Pos2::new(left, top), Vec2::new(column - 8.0, k.field()))
    };
    for (i, (key, name)) in grid.items.iter().enumerate() {
        let mut on = keys.iter().any(|k| k == key);
        let was = on;
        widgets::checkbox(ui, k, cell(i), id.with(key), &mut on, name);
        if on != was {
            Profile::set(keys, key, on);
        }
    }
    // A check box's mark and the gap before its name.
    let mark = 14.0 + 7.0;
    let mut gone = None;
    for (n, item) in custom.iter_mut().enumerate() {
        let place = cell(grid.items.len() + n);
        let text = format!("{} *", item.name);
        let room = place.width() - mark - 24.0;
        let text_w = widgets::text_width(ui, &text, k.font()).ceil().min(room);
        let boxed = place.with_max_x(place.left() + mark + text_w);
        let clip = ui.painter().clip_rect();
        ui.set_clip_rect(boxed.intersect(clip));
        widgets::checkbox(ui, k, boxed, id.with(("custom", n)), &mut item.on, &text);
        ui.set_clip_rect(clip);
        let x = Rect::from_center_size(Pos2::new(boxed.right() + 12.0, place.center().y), Vec2::splat(18.0));
        let resp = ui
            .interact(x, id.with(("remove", n)), Sense::click())
            .on_hover_text("Remove");
        let color = if resp.hovered() {
            ui.painter().rect_filled(x, k.radius(), t.hover);
            t.text_bright
        } else {
            t.text_dim
        };
        widgets::glyph(ui.painter(), k, x.center(), icon::X, 11.0, color);
        if resp.clicked() {
            gone = Some(n);
        }
    }
    if let Some(n) = gone {
        custom.remove(n);
    }
    // The field that adds one; what it adds starts ticked.
    let Some(hint) = grid.hint else { return };
    let field = cell(grid.items.len() + custom.len());
    let field_id = id.with("new");
    if let Some(name) = widgets::text_field(ui, k, field, field_id, "", hint) {
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if profile::add_custom(custom, grid.items, &name, true) && enter {
            // The field moves on to the next place, ready for another.
            ui.memory_mut(|m| m.request_focus(field_id));
        }
    }
}

/// Buttons side by side in one box, each with whether it is one of the
/// things on offer (the others are drawn dim but still take a click). The
/// one at `chosen` is marked. Returns the button clicked.
fn choices(
    ui: &mut Ui,
    k: &Look,
    rect: Rect,
    id: Id,
    chosen: Option<usize>,
    options: &[(&str, bool)],
) -> Option<usize> {
    widgets::boxed(ui.painter(), rect, k.radius(), k.t.input, k.t.border);
    let w = (rect.width() - 2.0) / options.len() as f32;
    let mut clicked = None;
    for (i, (name, offered)) in options.iter().enumerate() {
        let cell = Rect::from_min_size(
            Pos2::new((rect.left() + 1.0 + i as f32 * w).round(), rect.top() + 1.0),
            Vec2::new(w.round(), rect.height() - 2.0),
        );
        let resp = ui.interact(cell, id.with(i), Sense::click());
        if resp.clicked() {
            clicked = Some(i);
        }
        let p = ui.painter();
        let on = chosen == Some(i);
        if on {
            p.rect_filled(cell.shrink(1.0), (k.radius() - 1.0).max(0.0), k.t.control);
        } else if resp.hovered() && *offered {
            p.rect_filled(cell.shrink(1.0), (k.radius() - 1.0).max(0.0), k.t.row_hover);
        }
        let color = if on {
            k.t.text_bright
        } else if !*offered {
            k.t.border
        } else if resp.hovered() {
            k.t.text_bright
        } else {
            k.t.text
        };
        p.with_clip_rect(cell)
            .text(cell.center(), Align2::CENTER_CENTER, *name, k.font(), color);
    }
    clicked
}

/// The selected object at the top of the panel, then the property search.
fn header(ui: &mut Ui, k: &Look, st: &mut State, kind: Kind, name: &str) {
    let t = &k.t;
    let h = k.d.row + 14.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
    let y = rect.center().y + 1.0;
    let p = ui.painter();
    match k.d.props {
        Props::Inspector => {
            // Unity's header: a field holding the name, the type beside it.
            let kind_w = widgets::text_width(ui, kind.label(), k.font()).ceil();
            let field = Rect::from_min_max(
                Pos2::new(rect.left() + 34.0, y - k.field() / 2.0),
                Pos2::new(rect.right() - kind_w - 18.0, y + k.field() / 2.0),
            );
            glyph(p, k, Pos2::new(rect.left() + 19.0, y), kind.glyph(), 17.0, kind.color());
            widgets::field_box(p, k, field, false, false);
            p.text(
                Pos2::new(field.left() + 7.0, y),
                Align2::LEFT_CENTER,
                name,
                k.font(),
                t.text_bright,
            );
            label(p, k, Pos2::new(field.right() + 8.0, y), kind.label(), t.text_dim);
        }
        _ => {
            glyph(p, k, Pos2::new(rect.left() + 19.0, y), kind.glyph(), 18.0, kind.color());
            let x = p
                .text(
                    Pos2::new(rect.left() + 34.0, y),
                    Align2::LEFT_CENTER,
                    name,
                    k.bold(k.d.text + 1.0),
                    t.text_bright,
                )
                .right();
            label(p, k, Pos2::new(x + 8.0, y), kind.label(), t.text_dim);
        }
    }
    if k.d.props != Props::Inspector {
        let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), k.d.row + 6.0), Sense::hover());
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    Pos2::new(bar.left() + 8.0, bar.top()),
                    Pos2::new(bar.right() - 8.0, bar.bottom() - 6.0),
                ))
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 4.0;
        let w = child.available_width() - k.field() - 4.0;
        widgets::search(&mut child, k, &mut st.property_search, "Search properties", w);
        let _ = icon_button(&mut child, k, icon::GEAR_SIX, false).on_hover_text("Options");
    }
}

/// The header of a section; returns whether the section is open.
fn section(ui: &mut Ui, k: &Look, st: &mut State, index: usize, title: &str) -> bool {
    let t = &k.t;
    let h = match k.d.props {
        Props::Details => k.d.row + 2.0,
        Props::Inspector => k.d.row + 4.0,
        Props::Joined => k.d.row + 8.0,
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
    if resp.clicked() {
        st.sections[index] = !st.sections[index];
    }
    let open = st.sections[index];
    let p = ui.painter();
    let mut y = rect.center().y;
    match k.d.props {
        Props::Details => {
            let fill = if resp.hovered() { t.control } else { t.header };
            p.rect_filled(rect, 0.0, fill);
            hair_h(p, rect.left(), rect.right(), rect.bottom() - 1.0, t.line);
        }
        Props::Inspector => {
            hair_h(p, rect.left(), rect.right(), rect.top(), t.divider);
            if resp.hovered() {
                p.rect_filled(rect.with_min_y(rect.top() + 1.0), 0.0, t.row_hover);
            }
            y += 0.5;
            glyph(
                p,
                k,
                Pos2::new(rect.right() - 14.0, y),
                icon::DOTS_THREE_VERTICAL,
                15.0,
                t.text_dim,
            );
        }
        Props::Joined => {
            hair_h(p, rect.left() + 10.0, rect.right() - 10.0, rect.top(), t.header);
            y += 1.0;
        }
    }
    let arrow = if open { icon::CARET_DOWN } else { icon::CARET_RIGHT };
    let color = if resp.hovered() { t.text_bright } else { t.text_dim };
    glyph(p, k, Pos2::new(rect.left() + 14.0, y), arrow, 10.0, color);
    p.text(
        Pos2::new(rect.left() + 26.0, y),
        Align2::LEFT_CENTER,
        title,
        k.bold(k.d.text),
        t.text_bright,
    );
    open
}

/// Space under the rows of an open section.
fn section_end(ui: &mut Ui, k: &Look) {
    let gap = match k.d.props {
        Props::Details => 0.0,
        Props::Inspector => 5.0,
        Props::Joined => 8.0,
    };
    ui.add_space(gap);
}

/// One property: its name in the left column, and `add` drawing the value
/// into the rectangle of the right column. `off` greys the value and takes
/// it out of use, and is what the pointer is told about why.
fn row(ui: &mut Ui, k: &Look, name: &str, off: Option<&str>, add: impl FnOnce(&mut Ui, Rect)) {
    let t = &k.t;
    let name_color = if off.is_some() { t.text_dim } else { t.text };
    let (h, share, least) = match k.d.props {
        Props::Details => (k.d.row + 2.0, 0.38, 108.0),
        Props::Inspector => (k.d.row + 2.0, 0.34, 100.0),
        Props::Joined => (k.d.row + 4.0, 0.33, 100.0),
    };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
    let split = (rect.left() + (rect.width() * share).max(least)).round();
    let y = rect.center().y;
    let p = ui.painter();
    let names = p.with_clip_rect(Rect::from_min_max(rect.min, Pos2::new(split - 6.0, rect.bottom())));
    let (from, to) = match k.d.props {
        Props::Details => {
            if ui.rect_contains_pointer(rect) {
                p.rect_filled(rect, 0.0, t.row_hover);
            }
            label(&names, k, Pos2::new(rect.left() + 26.0, y), name, name_color);
            hair_v(p, split, rect.top(), rect.bottom(), t.line);
            hair_h(p, rect.left(), rect.right(), rect.bottom() - 1.0, t.line);
            (split + 9.0, rect.right() - 10.0)
        }
        Props::Inspector => {
            label(&names, k, Pos2::new(rect.left() + 26.0, y), name, name_color);
            (split, rect.right() - 8.0)
        }
        Props::Joined => {
            names.text(
                Pos2::new(split - 10.0, y),
                Align2::RIGHT_CENTER,
                name,
                k.font(),
                t.text_dim,
            );
            (split, rect.right() - 12.0)
        }
    };
    let half = k.field() / 2.0;
    let value = Rect::from_min_max(
        Pos2::new(from, (y - half).round()),
        Pos2::new(to, (y - half).round() + k.field()),
    );
    match off {
        None => add(ui, value),
        Some(why) => {
            let mut greyed = ui.new_child(UiBuilder::new().max_rect(value));
            greyed.disable();
            add(&mut greyed, value);
            ui.interact(value, ui.id().with(("off", name)), Sense::hover())
                .on_hover_text(why);
        }
    }
}

/// Three numbers for the X, Y and Z axes, marked in the axis colours.
fn vec3(ui: &mut Ui, k: &Look, rect: Rect, id: Id, values: &mut [f32; 3], speed: f32) {
    let t = &k.t;
    match k.d.props {
        Props::Details => {
            let gap = 4.0;
            let w = ((rect.width() - 2.0 * gap) / 3.0).floor();
            for (i, value) in values.iter_mut().enumerate() {
                let cell = Rect::from_min_size(
                    Pos2::new(rect.left() + i as f32 * (w + gap), rect.top()),
                    Vec2::new(w, rect.height()),
                );
                widgets::number(ui, k, cell, id.with(i), value, Marker::Bar(t.axis[i]), true, speed);
            }
        }
        Props::Inspector => {
            let gap = 6.0;
            let letter = 13.0;
            let w = ((rect.width() - 2.0 * gap) / 3.0).floor();
            for (i, value) in values.iter_mut().enumerate() {
                let left = rect.left() + i as f32 * (w + gap);
                ui.painter().text(
                    Pos2::new(left + 4.0, rect.center().y),
                    Align2::CENTER_CENTER,
                    AXES[i],
                    k.bold(k.d.small),
                    t.axis[i],
                );
                let cell = Rect::from_min_max(Pos2::new(left + letter, rect.top()), Pos2::new(left + w, rect.bottom()));
                widgets::number(ui, k, cell, id.with(i), value, Marker::None, true, speed);
            }
        }
        Props::Joined => {
            widgets::field_box(ui.painter(), k, rect, false, false);
            let w = (rect.width() / 3.0).floor();
            for (i, value) in values.iter_mut().enumerate() {
                let right = if i == 2 {
                    rect.right()
                } else {
                    rect.left() + (i + 1) as f32 * w
                };
                let cell = Rect::from_min_max(
                    Pos2::new(rect.left() + i as f32 * w, rect.top()),
                    Pos2::new(right, rect.bottom()),
                );
                if i > 0 {
                    hair_v(
                        ui.painter(),
                        cell.left(),
                        cell.top() + 1.0,
                        cell.bottom() - 1.0,
                        t.border,
                    );
                }
                widgets::number(
                    ui,
                    k,
                    cell.with_min_x(cell.left() + if i > 0 { 1.0 } else { 0.0 }),
                    id.with(i),
                    value,
                    Marker::Letter(t.axis[i], AXES[i]),
                    false,
                    speed,
                );
            }
        }
    }
}
