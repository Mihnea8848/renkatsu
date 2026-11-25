// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The Properties panel: everything about the selection in sections of
//! rows, with names and values aligned in two columns. The values are the
//! selected object's own, from the scene model; what waits for a later
//! feature is shown greyed, and says what it waits for.

use crate::design::Props;
use crate::kinds::Kind;
use crate::profile::{self, Profile};
use crate::scene::MapTime;
use crate::scene::{Content, Effect, LightKind, Place};
use crate::state::{Look, State};
use crate::widgets::{self, glyph, hair_h, hair_v, icon_button, label, Marker};
use egui::{Align, Align2, Color32, Id, Layout, Pos2, Rect, Sense, Ui, UiBuilder, Vec2};
use egui_phosphor::regular as icon;
use std::path::Path;

const AXES: [&str; 3] = ["X", "Y", "Z"];

/// The character page that holds what an H scene leaves on a character:
/// the liquid on the skin and the clothes now, and the rest of the game's
/// H-time effects later.
const H_PAGE: &str = "H Effects";
/// Renkatsu's own improvements on the game, such as Advanced Skirt
/// Physics.
const RENKATSU_PAGE: &str = "Renkatsu";

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
    let color = st.library.color_of(&object.content);
    header(
        ui,
        k,
        st,
        object.content.glyph(),
        object.content.label(),
        color,
        &object.name,
    );
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
    // A character's pages: the scene's own values, then what it plays,
    // then who it is. A card whose profile could not be read still has
    // the first two.
    let pages: &[&str] = match &shown {
        Some(_) => &["Scene", "Animation", "Profile", H_PAGE, RENKATSU_PAGE],
        None if kind == Kind::Character => &["Scene", "Animation", H_PAGE, RENKATSU_PAGE],
        None => &[],
    };
    // The page that holds what the body shows in an H scene, which is of
    // the scene and not of the card, so every character has it.
    let h_page = pages.iter().position(|&p| p == H_PAGE);
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
    let holds = st.doc.scene().subtree(object.id).len() - 1;
    // What the object is attached to: a folder only holds it.
    let target = before
        .parent
        .and_then(|parent| st.doc.scene().get(parent))
        .filter(|parent| parent.kind() != Kind::Folder)
        .map(|parent| (parent.id, parent.name.clone(), parent.kind()));
    let mut detach = false;
    let mut bone: Option<Option<String>> = None;
    // Who the character could be snapped onto: every other character of
    // the scene it may sit inside, with whether that one plays the other
    // half of the H animation this one plays. A pair playing the two
    // halves is what snapping is for, so those come first.
    let mut partners: Vec<(crate::scene::ObjectId, String, bool)> = Vec::new();
    if kind == Kind::Character {
        let scene = st.doc.scene();
        let mine = animation_meta(st, before.id);
        partners = scene
            .objects()
            .iter()
            .filter(|o| o.kind() == Kind::Character && o.id != before.id)
            .filter(|o| scene.can_place(before.id, Place::Into(Some(o.id))))
            .map(|o| {
                let pair = mine
                    .zip(animation_meta(st, o.id))
                    .is_some_and(|(a, b)| crate::animations::same_h_animation(a, b));
                (o.id, o.name.clone(), pair)
            })
            .collect();
        partners.sort_by_key(|(_, name, pair)| (!*pair, name.to_lowercase()));
    }
    // The character this one is snapped onto, or none; a character that
    // follows a bone of another is attached, not snapped.
    let snapped = before
        .parent
        .filter(|_| before.bone.is_none())
        .filter(|parent| partners.iter().any(|(id, ..)| id == parent));
    let mut snap: Option<Option<crate::scene::ObjectId>> = None;
    let mut chosen_map: Option<String> = None;
    let mut show_maps = false;

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        // What the body shows in an H scene. The levels are the game's
        // `siru`: a page of its own, since none of it is of the card.
        if Some(page) == h_page {
            let Content::Character {
                liquid,
                liquid_clothes,
                pose_layer,
                ..
            } = &mut object.content
            else {
                return;
            };
            if section(ui, k, st, 10, "Liquid") {
                use rk_chara::siru;
                let waiting = match built.as_ref() {
                    None => Some("The character is not built"),
                    // The game's male body carries no liquid mask.
                    Some(b) if !b.liquid_mask => Some("This body carries no liquid mask"),
                    Some(_) => None,
                };
                let id = ui.id();
                for (part, name) in siru::NAMES.iter().enumerate() {
                    row(ui, k, name, waiting, |ui, rect| {
                        let options = [("None", true), ("Light", true), ("Heavy", true)];
                        let chosen = usize::from(liquid[part].min(siru::MAX));
                        let clicked = choices(ui, k, rect, id.with(("liquid", part)), Some(chosen), &options);
                        if let Some(i) = clicked {
                            liquid[part] = i as u8;
                        }
                    });
                }
                // The game always lays the body's levels on the clothes
                // as well; here that can be turned off.
                row(ui, k, "Clothes", waiting, |ui, rect| {
                    widgets::checkbox(ui, k, rect, id.with("liquid clothes"), liquid_clothes, "Wet");
                });
                section_end(ui, k);
            }
            let waiting = built.is_none().then_some("The character is not built");
            skin_effects_section(ui, k, st, object.id, waiting, pose_layer);
            if let Some(card) = built.as_ref().and_then(|b| b.better_penetration) {
                penetration_section(ui, k, st, &mut object, card);
            }
            return;
        }
        let page_name = pages.get(page).copied().unwrap_or_default();
        // Renkatsu's own improvements on the game.
        if page_name == RENKATSU_PAGE {
            let Content::Character {
                outfit,
                game_skirts,
                skirt_kinds,
                ..
            } = &mut object.content
            else {
                return;
            };
            if section(ui, k, st, 18, "Physics") {
                // Per outfit: the ones listed move as in CharaStudio.
                let mut advanced = !game_skirts.contains(outfit);
                row(ui, k, "Skirts", None, |ui, rect| {
                    widgets::checkbox(ui, k, rect, ui.id().with("advanced skirts"), &mut advanced, "Advanced");
                });
                if advanced {
                    game_skirts.retain(|o| o != outfit);
                } else if !game_skirts.contains(outfit) {
                    game_skirts.push(*outfit);
                    game_skirts.sort_unstable();
                }
                // The kind of cloth, per outfit too: guessed from the
                // clothing unless one is chosen.
                let kinds = crate::scene::SkirtKind::ALL;
                let was = crate::scene::skirt_kind(skirt_kinds, *outfit);
                let mut chosen = was
                    .and_then(|k| kinds.iter().position(|o| *o == k))
                    .map_or(0, |i| i + 1);
                row(ui, k, "Kind", None, |ui, rect| {
                    let options = ["Auto", "Tight", "Pleated", "Flowing", "Fixed"];
                    widgets::combo(ui, k, rect, ui.id().with("skirt kind"), &mut chosen, &options);
                });
                let now = chosen.checked_sub(1).map(|i| kinds[i]);
                if now != was {
                    crate::scene::choose_skirt_kind(skirt_kinds, *outfit, now);
                }
                section_end(ui, k);
            }
            return;
        }
        // Who the character is, what she likes and what she is like in
        // bed, as sections of one page, and a way back to the card's.
        if let (Some(p), "Profile") = (shown.as_mut(), page_name) {
            if let Some(card) = &card_profile {
                let parts = if p.male { 1 } else { 3 };
                for n in 1..=parts {
                    profile_page(ui, k, st, p, n);
                }
                card_data(ui, k, st, p, card);
            }
            return;
        }
        let id = ui.id();
        // The Animation page: what the character plays, and the pair it
        // is snapped into.
        if page == 1 {
            let Content::Character { animation, .. } = &mut object.content else {
                return;
            };
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
                        widgets::caret_of(&resp),
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
            if section(ui, k, st, 4, "Pair") {
                let alone = partners.is_empty().then_some("The scene has one character");
                row(ui, k, "Snap to", alone, |ui, rect| {
                    let shown = snapped
                        .and_then(|id| partners.iter().find(|(other, ..)| *other == id))
                        .map_or("None", |(_, name, _)| name.as_str());
                    let resp = drop_field(ui, k, rect, id.with("snap"), shown);
                    egui::Popup::menu(&resp).show(|ui| {
                        ui.set_min_width(rect.width() - 10.0);
                        if widgets::check_item(ui, k, snapped.is_none(), "None", "").clicked() {
                            snap = Some(None);
                            ui.close();
                        }
                        for (other, name, pair) in &partners {
                            let note = if *pair { "Same H animation" } else { "" };
                            if widgets::check_item(ui, k, snapped == Some(*other), name, note).clicked() {
                                snap = Some(Some(*other));
                                ui.close();
                            }
                        }
                    });
                });
                // The H scenes' animations are blended by body, and
                // CharaStudio's two animation options
                // (`Studio.OICharInfo.animeOptionParam`) set the blend the
                // clips play at instead of the character's own. A snapped
                // pair holding different ones drifts apart.
                let h = animation
                    .as_deref()
                    .and_then(|a| st.animations.of(a))
                    .and_then(|item| item.meta.as_ref())
                    .is_some_and(|meta| meta.h);
                let own = built
                    .as_ref()
                    .map_or([0.5; 2], |b| [b.parameters.height, b.parameters.breast]);
                let flat = (!h).then_some("The animation is not one of the H scenes'");
                let mut values = animation.as_ref().and_then(|a| a.options).unwrap_or(own);
                for (i, name) in ["Height", "Breasts"].into_iter().enumerate() {
                    row(ui, k, name, flat, |ui, rect| {
                        let at = id.with(("animation option", i));
                        widgets::number(ui, k, rect, at, &mut values[i], Marker::None, true, FINE);
                        values[i] = values[i].clamp(0.0, 1.0);
                    });
                }
                if let Some(a) = animation.as_mut().filter(|_| h) {
                    if a.options.is_some() || values != own {
                        a.options = Some(values);
                    }
                }
                section_end(ui, k);
            }
            // The hands and the face over the animation.
            face_sections(ui, k, st, object.id, &mut object.content);
            // The pose made by hand over the animation, while she is posed
            // by hand.
            pose_sections(ui, k, st, object.id, &mut object.content);
            return;
        }
        // A character light is turned by its angles alone.
        let chara_light = matches!(
            object.content,
            Content::Light {
                light: LightKind::Character,
                ..
            }
        );
        if !matches!(kind, Kind::Folder | Kind::Effect) && !chara_light && section(ui, k, st, 0, "Transform") {
            // What each of position, rotation and scale waits for, when it
            // does nothing yet for this kind of object.
            let off: [Option<&str>; 3] = match &object.content {
                Content::Character { .. } => [None; 3],
                Content::Light {
                    light: LightKind::Point | LightKind::Spot,
                    ..
                } => [None, None, Some("A light has no size")],
                Content::Light {
                    light: LightKind::Character,
                    ..
                } => [
                    Some("A directional light has no position"),
                    Some("Turned by its angles"),
                    Some("A light has no size"),
                ],
                Content::Light { .. } => [
                    Some("A directional light has no position"),
                    None,
                    Some("A light has no size"),
                ],
                Content::Camera { .. } => [None, None, Some("A camera has no size")],
                Content::Item | Content::MapPiece { .. } => [None; 3],
                Content::MapDoor { .. } => [Some("A door stays in its frame"); 3],
                // CharaStudio moves and turns a map; it keeps its size.
                Content::Map { .. } => [None, None, Some("A map keeps its size")],
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
        // An effect has no place to follow; a map's own just belongs to it.
        let shown_target = target.as_ref().filter(|_| kind != Kind::Effect);
        if let Some((to, name, target_kind)) = shown_target {
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
        let mut reset_to: Option<[f32; 3]> = None;
        match &mut object.content {
            Content::Character {
                card,
                outfit,
                outdoor_shoes,
                simulate,
                clothes,
                light_group,
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
                            widgets::caret_of(&resp),
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
                    let no_names = std::collections::BTreeMap::new();
                    let names = built.as_ref().map_or(&no_names, |b| &b.outfit_names);
                    let outfits: Vec<String> = (0..built.as_ref().map_or(1, |b| b.outfits.max(1)))
                        .map(|i| rk_chara::more_outfits::outfit_name(i, names))
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
                    let mut names: Vec<String> = std::iter::once(crate::scene::EVERYONE.to_owned())
                        .chain(st.doc.scene().light_groups.iter().cloned())
                        .collect();
                    let mut new_group = false;
                    row(ui, k, "Light group", None, |ui, rect| {
                        let mut chosen = light_group
                            .as_ref()
                            .and_then(|g| names.iter().position(|n| n == g))
                            .unwrap_or(0);
                        let mut options: Vec<&str> = names.iter().map(String::as_str).collect();
                        options.push("New group");
                        widgets::combo(ui, k, rect, id.with("light group"), &mut chosen, &options);
                        new_group = chosen == names.len();
                        if !new_group {
                            *light_group = (chosen > 0).then(|| names[chosen].clone());
                        }
                    });
                    if new_group {
                        names.push(st.doc.add_light_group());
                        *light_group = names.last().cloned();
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
                light,
                from_camera,
                angles,
                intensity,
                shadows,
                color,
                range,
                spot_angle,
                on_map,
                on_characters,
                groups,
                with_map,
            } => {
                if section(ui, k, st, 1, "Light") {
                    row(ui, k, "Type", None, |ui, rect| {
                        let mut chosen = LightKind::ALL.iter().position(|l| l == light).unwrap_or(0);
                        let names = LightKind::ALL.map(|l| l.label());
                        widgets::combo(ui, k, rect, id.with("light"), &mut chosen, &names);
                        *light = LightKind::ALL[chosen];
                    });
                    row(ui, k, "Colour", None, |ui, rect| {
                        widgets::color_field(ui, k, rect, id.with("colour"), color);
                    });
                    if *light == LightKind::Character {
                        row(ui, k, "Follows", None, |ui, rect| {
                            let mut chosen = usize::from(*from_camera);
                            let options = ["Scene", "Camera"];
                            widgets::segmented(ui, k, rect, id.with("direction"), &mut chosen, &options);
                            *from_camera = chosen == 1;
                        });
                        row(ui, k, "Angles", None, |ui, rect| {
                            let half = (rect.width() - 4.0) / 2.0;
                            for (i, letter) in ["X", "Y"].into_iter().enumerate() {
                                let cell = Rect::from_min_size(
                                    rect.min + Vec2::new(i as f32 * (half + 4.0), 0.0),
                                    Vec2::new(half, rect.height()),
                                );
                                let value = &mut angles[i];
                                let marker = Marker::Letter(t.text_dim, letter);
                                widgets::number(ui, k, cell, id.with(("angle", i)), value, marker, true, ANGLE);
                                *value = value.rem_euclid(360.0);
                            }
                        });
                    } else {
                        row(ui, k, "Lights", None, |ui, rect| {
                            let mut chosen = match (*on_map, *on_characters) {
                                (true, false) => 1,
                                (false, true) => 2,
                                _ => 0,
                            };
                            let options = ["Map and characters", "Map", "Characters"];
                            widgets::combo(ui, k, rect, id.with("lights"), &mut chosen, &options);
                            (*on_map, *on_characters) = [(true, true), (true, false), (false, true)][chosen];
                        });
                    }
                    row(ui, k, "Intensity", None, |ui, rect| {
                        widgets::number(ui, k, rect, id.with("intensity"), intensity, Marker::None, true, FINE);
                        *intensity = intensity.clamp(0.0, 8.0);
                    });
                    if *light == LightKind::Character {
                        row(ui, k, "Map light", None, |ui, rect| {
                            widgets::link_toggle(ui, k, rect, id.with("with map"), with_map, "Adjust with map light");
                        });
                    }
                    if matches!(light, LightKind::Point | LightKind::Spot) {
                        row(ui, k, "Range", None, |ui, rect| {
                            widgets::number(ui, k, rect, id.with("range"), range, Marker::None, true, FINE);
                            *range = range.max(0.0);
                        });
                    }
                    if *light == LightKind::Spot {
                        row(ui, k, "Spot angle", None, |ui, rect| {
                            widgets::number(ui, k, rect, id.with("cone"), spot_angle, Marker::None, true, ANGLE);
                            *spot_angle = spot_angle.clamp(1.0, 179.0);
                        });
                    }
                    if *light == LightKind::Character || *on_characters {
                        let names: Vec<String> = std::iter::once(crate::scene::EVERYONE.to_owned())
                            .chain(st.doc.scene().light_groups.iter().cloned())
                            .collect();
                        row(ui, k, "Groups", None, |ui, rect| {
                            let text = match groups {
                                None => "All".to_owned(),
                                Some(list) if list.is_empty() => "None".to_owned(),
                                Some(list) => list.join(", "),
                            };
                            let resp = widgets::dropdown_field(ui, k, rect, id.with("groups"), &text);
                            egui::Popup::menu(&resp)
                                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                .show(|ui| {
                                    for name in &names {
                                        let on = groups.as_ref().is_none_or(|list| list.contains(name));
                                        if widgets::check_item(ui, k, on, name, "").clicked() {
                                            let mut list = groups.clone().unwrap_or_else(|| names.clone());
                                            if on {
                                                list.retain(|n| n != name);
                                            } else {
                                                list.push(name.clone());
                                            }
                                            let all = names.iter().all(|n| list.contains(n));
                                            *groups = (!all).then_some(list);
                                        }
                                    }
                                });
                        });
                    }
                    row(ui, k, "Shadows", None, |ui, rect| {
                        widgets::checkbox(ui, k, rect, id.with("shadows"), shadows, "Cast");
                    });
                    section_end(ui, k);
                }
            }
            Content::Effect { effect } => {
                let title = effect.label();
                if section(ui, k, st, 1, title) {
                    let number = |ui: &mut Ui, name: &str, value: &mut f32, lo: f32, hi: f32| {
                        row(ui, k, name, None, |ui, rect| {
                            widgets::number(ui, k, rect, id.with(name), value, Marker::None, true, FINE);
                            *value = value.clamp(lo, hi);
                        });
                    };
                    match effect {
                        Effect::ColorFilter { filter, blend } => {
                            row(ui, k, "Filter", None, |ui, rect| {
                                // First the map's own, then the game's list.
                                let names: Vec<&str> =
                                    std::iter::once("Map's own").chain(crate::scene::FILTERS).collect();
                                let mut chosen = filter.map_or(0, |f| f + 1);
                                widgets::combo(ui, k, rect, id.with("filter"), &mut chosen, &names);
                                *filter = chosen.checked_sub(1);
                            });
                            number(ui, "Blend", blend, 0.0, 1.0);
                        }
                        Effect::Bloom {
                            intensity,
                            threshold,
                            spread,
                        } => {
                            number(ui, "Intensity", intensity, 0.0, 4.0);
                            number(ui, "Threshold", threshold, 0.0, 1.0);
                            number(ui, "Spread", spread, 0.0, 4.0);
                        }
                        Effect::AmbientOcclusion { intensity, radius } => {
                            number(ui, "Intensity", intensity, 0.0, 4.0);
                            number(ui, "Radius", radius, 0.0, 2.0);
                        }
                        Effect::Vignette { intensity, blur } => {
                            number(ui, "Intensity", intensity, 0.0, 1.0);
                            number(ui, "Blur", blur, 0.0, 1.0);
                        }
                        Effect::Fog {
                            color,
                            height,
                            start_distance,
                            visibility,
                            mist,
                        } => {
                            row(ui, k, "Preset", None, |ui, rect| {
                                use crate::scene::FOG_PRESETS;
                                let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
                                let same = |x: Option<f32>, y: Option<f32>| match (x, y) {
                                    (Some(x), Some(y)) => near(x, y),
                                    (x, y) => x.is_none() && y.is_none(),
                                };
                                let current = FOG_PRESETS.iter().position(|p| {
                                    same(*visibility, p.visibility)
                                        && same(*mist, p.mist)
                                        && (p.visibility.is_none() || (*color == p.color && near(*height, p.height)))
                                });
                                let mut names: Vec<&str> = FOG_PRESETS.iter().map(|p| p.name).collect();
                                names.push("Custom");
                                let mut chosen = current.unwrap_or(FOG_PRESETS.len());
                                let was = chosen;
                                widgets::combo(ui, k, rect, id.with("fog preset"), &mut chosen, &names);
                                if chosen != was {
                                    if let Some(p) = FOG_PRESETS.get(chosen) {
                                        *color = p.color;
                                        *height = p.height;
                                        *visibility = p.visibility;
                                        *mist = p.mist;
                                        *start_distance = 0.0;
                                    }
                                }
                            });
                            row(ui, k, "Colour", None, |ui, rect| {
                                let mut chosen = usize::from(color.is_some());
                                widgets::combo(ui, k, rect, id.with("fog colour"), &mut chosen, &["Map's own", "Own"]);
                                if chosen == 0 {
                                    *color = None;
                                } else if color.is_none() {
                                    *color = Some([137.0 / 255.0, 193.0 / 255.0, 221.0 / 255.0]);
                                }
                            });
                            if let Some(own) = color {
                                row(ui, k, "", None, |ui, rect| {
                                    widgets::color_field(ui, k, rect, id.with("fog own"), own);
                                });
                            }
                            if visibility.is_none() && mist.is_none() {
                                // CharaStudio's fog.
                                number(ui, "Height", height, 0.0, 50.0);
                                number(ui, "Start", start_distance, 0.0, 50.0);
                            } else {
                                let mut see = visibility.unwrap_or(1000.0);
                                number(ui, "Visibility", &mut see, 0.5, 1000.0);
                                *visibility = Some(see);
                                row(ui, k, "Ground mist", None, |ui, rect| {
                                    let mut on = mist.is_some();
                                    widgets::checkbox(ui, k, rect, id.with("mist"), &mut on, "");
                                    *mist = match (on, *mist) {
                                        (true, None) => Some(6.0),
                                        (false, _) => None,
                                        (true, m) => m,
                                    };
                                });
                                if let Some(m) = mist {
                                    number(ui, "Mist height", height, 0.05, 10.0);
                                    number(ui, "Mist visibility", m, 0.5, 1000.0);
                                }
                            }
                        }
                        Effect::DepthOfField { focal_size, aperture } => {
                            // CharaStudio's focal size and aperture.
                            number(ui, "Sharp range", focal_size, 0.0, 2.0);
                            number(ui, "Blur", aperture, 0.0, 1.0);
                        }
                        Effect::SunShafts { threshold, color } => {
                            // CharaStudio's threshold and colour.
                            row(ui, k, "Sky brightness", None, |ui, rect| {
                                widgets::color_field(ui, k, rect, id.with("threshold"), threshold);
                            });
                            row(ui, k, "Ray colour", None, |ui, rect| {
                                let mut chosen = usize::from(color.is_some());
                                widgets::combo(ui, k, rect, id.with("ray colour"), &mut chosen, &["Map's own", "Own"]);
                                if chosen == 0 {
                                    *color = None;
                                } else if color.is_none() {
                                    *color = Some([1.0; 3]);
                                }
                            });
                            if let Some(own) = color {
                                row(ui, k, "", None, |ui, rect| {
                                    widgets::color_field(ui, k, rect, id.with("rays own"), own);
                                });
                            }
                        }
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
            Content::Map {
                map,
                time,
                hide_walls,
                light_intensity,
                filter_decay,
                sun,
                ..
            } => {
                let own_sun = crate::map_menu::own_sun(st);
                let sidecar = st.project.resolve(map);
                let file = st.map_pictures.sidecar(&sidecar);
                let times = file.as_ref().map_or_else(Vec::new, |f| f.times.clone());
                if section(ui, k, st, 1, "Map") {
                    row(ui, k, "Map", None, |ui, rect| {
                        let side = rect.height();
                        let field = Rect::from_min_max(rect.min, Pos2::new(rect.right() - side - 4.0, rect.bottom()));
                        let name = file.as_ref().map_or(before.name.as_str(), |f| f.name.as_str());
                        let resp = drop_field(ui, k, field, id.with("map"), name);
                        egui::Popup::menu(&resp).show(|ui| {
                            ui.set_min_width(field.width() - 10.0);
                            for owned in crate::maps::in_project(&st.project.root) {
                                let on = Path::new(&owned.file) == map.as_path();
                                if widgets::check_item(ui, k, on, &owned.map.name, "").clicked() {
                                    chosen_map = Some(owned.file.clone());
                                    ui.close();
                                }
                            }
                        });
                        resp.on_hover_text(map.display().to_string());
                        let button = Rect::from_min_size(Pos2::new(field.right() + 4.0, rect.top()), Vec2::splat(side));
                        let resp = ui
                            .interact(button, id.with("browse maps"), Sense::click())
                            .on_hover_text("Show in Content");
                        let fill = if resp.hovered() { k.t.hover } else { k.t.control };
                        ui.painter().rect_filled(button, k.radius(), fill);
                        glyph(ui.painter(), k, button.center(), icon::FOLDER_OPEN, 14.0, k.t.text);
                        if resp.clicked() {
                            show_maps = true;
                        }
                    });
                    let game = st.games.ready(&st.project.mods_dir());
                    let own = file
                        .as_ref()
                        .and_then(|f| Some((f.first_time(), sidecar.parent()?.join(f.thumbnail.as_ref()?))));
                    let bundle = file.as_ref().map_or("", |f| f.scene_bundle()).to_owned();
                    let pictures = MapTime::ALL.map(|t| {
                        if !times.contains(&t) {
                            return None;
                        }
                        let own = own.as_ref().filter(|(first, _)| *first == t).map(|(_, p)| p.as_path());
                        st.map_pictures
                            .get(ui.ctx(), game.as_deref(), &bundle, t, own, st.scripted)
                    });
                    time_pictures(ui, k, id.with("map time"), time, &times, &pictures);
                    let none = file
                        .as_ref()
                        .is_some_and(|f| f.walls.is_empty())
                        .then_some("This map has no walls that hide");
                    row(ui, k, "Hide walls", none, |ui, rect| {
                        widgets::checkbox(ui, k, rect, id.with("hide walls"), hide_walls, "");
                    });
                    let mut angles = sun.unwrap_or(own_sun);
                    let before = angles;
                    let mut reset = false;
                    row(ui, k, "Sun heading", None, |ui, rect| {
                        let turned = sun.is_some();
                        reset = crate::map_menu::sun_heading(ui, k, rect, id.with("sun heading"), &mut angles, turned);
                    });
                    row(ui, k, "Sun elevation", None, |ui, rect| {
                        crate::map_menu::sun_elevation(ui, k, rect, id.with("sun elevation"), &mut angles);
                    });
                    if reset {
                        *sun = None;
                    } else if angles != before {
                        *sun = Some(angles);
                    }
                    row(ui, k, "Light intensity", None, |ui, rect| {
                        widgets::number(
                            ui,
                            k,
                            rect,
                            id.with("map light"),
                            light_intensity,
                            Marker::None,
                            true,
                            FINE,
                        );
                        *light_intensity = light_intensity.clamp(0.0, 1.0);
                    });
                    row(ui, k, "Filter decay", None, |ui, rect| {
                        // How far the colour filter has faded when the map
                        // has no light.
                        const STEPS: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
                        let names = ["None", "25%", "50%", "75%", "100%"];
                        let mut chosen = STEPS
                            .iter()
                            .enumerate()
                            .min_by(|a, b| (a.1 - *filter_decay).abs().total_cmp(&(b.1 - *filter_decay).abs()))
                            .map_or(2, |(i, _)| i);
                        let was = chosen;
                        widgets::combo(ui, k, rect, id.with("filter decay"), &mut chosen, &names);
                        if chosen != was {
                            *filter_decay = STEPS[chosen];
                        }
                    });
                    section_end(ui, k);
                }
            }
            Content::MapPiece { rest, .. } => {
                if section(ui, k, st, 1, "Map piece") {
                    row(ui, k, "Place", None, |ui, rect| {
                        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                        if widgets::flat_button(
                            &mut child,
                            k,
                            Some(icon::ARROW_COUNTER_CLOCKWISE),
                            "Reset to the map's place",
                            false,
                            false,
                        )
                        .clicked()
                        {
                            reset_to = Some(*rest);
                        }
                    });
                    section_end(ui, k);
                }
            }
            Content::MapDoor { open, .. } => {
                if section(ui, k, st, 1, "Door") {
                    row(ui, k, "Open", None, |ui, rect| {
                        widgets::checkbox(ui, k, rect, id.with("door open"), open, "");
                    });
                    section_end(ui, k);
                }
            }
            Content::Folder | Content::Item => {
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
        if let Some(rest) = reset_to {
            object.transform.position = rest;
            object.transform.rotation = [0.0; 3];
            object.transform.scale = [1.0; 3];
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
    if detach || snap == Some(None) {
        st.doc.place_many(&[before.id], Place::Into(None));
    }
    if let Some(Some(to)) = snap {
        st.snap(before.id, to);
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
    if show_maps {
        st.show_folder(3);
    }
    if let Some(file) = chosen_map {
        st.add_map(std::path::PathBuf::from(file));
    }
}

/// What the project's file says about the animation a character plays;
/// none for a character playing nothing, or an animation the project
/// does not own.
fn animation_meta(st: &State, id: crate::scene::ObjectId) -> Option<&rk_chara::animation_file::Meta> {
    let Content::Character { animation, .. } = &st.doc.scene().get(id)?.content else {
        return None;
    };
    st.animations.of(animation.as_ref()?)?.meta.as_ref()
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
        widgets::caret_of(&resp),
        10.0,
        color,
    );
    resp
}

/// The row of pages under a character's header, each a word with a line
/// under the open one.
fn page_tabs(ui: &mut Ui, k: &Look, chosen: &mut usize, pages: &[&str]) {
    let t = &k.t;
    let line_height = k.d.row + 4.0;
    // Tabs that are a little too wide for one line close up first; the
    // ones that still do not fit go on the next, rather than off the edge
    // of the panel.
    let room = ui.available_width() - 16.0;
    let text: Vec<f32> = pages
        .iter()
        .map(|name| widgets::text_width(ui, name, k.font()).ceil())
        .collect();
    let gap = ((room - text.iter().sum::<f32>()) / pages.len().max(1) as f32).clamp(8.0, 18.0);
    let widths: Vec<f32> = text.iter().map(|w| w + gap).collect();
    // Where each tab goes: its line and its left edge.
    let mut place = Vec::with_capacity(widths.len());
    let (mut lines, mut used) = (1, 0.0);
    for w in &widths {
        if used + w > room && used > 0.0 {
            lines += 1;
            used = 0.0;
        }
        place.push((lines - 1, used));
        used += w;
    }
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), line_height * lines as f32),
        Sense::hover(),
    );
    hair_h(ui.painter(), rect.left(), rect.right(), rect.bottom() - 1.0, t.line);
    for (i, name) in pages.iter().enumerate() {
        let w = widths[i];
        let (line, left) = place[i];
        let (x, top) = (rect.left() + 8.0 + left, rect.top() + line_height * line as f32);
        let tab = Rect::from_min_max(Pos2::new(x, top), Pos2::new(x + w, top + line_height));
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
    }
}

/// The Animation page's sections of the pose made by hand, while the
/// character is posed by hand: the bone chosen (on its ball or in the
/// Animator) with its turn over the animation and a way back.
/// Section numbers of the hands and the face.
pub const HANDS_SECTION: usize = 19;
pub const FACE_SECTION: usize = 20;
pub const SKIN_SECTION: usize = 21;
pub const PENETRATION_SECTION: usize = 22;

/// The hands' patterns and the face, as Studio's hand and face panels set
/// them: each pattern a picture that opens a grid of the others.
fn face_sections(ui: &mut Ui, k: &Look, st: &mut State, id: crate::scene::ObjectId, content: &mut Content) {
    use crate::live::Pictures;
    use rk_chara::channels::Channel;
    let Content::Character { pose_layer, .. } = content else {
        return;
    };
    let rig = st.live.as_ref().and_then(|l| l.face_rig(id)).cloned();
    let waiting = rig.is_none().then_some("The character is not built");
    let start = rig.as_ref().map(|r| r.start).unwrap_or_default();
    let ui_id = ui.id().with("face");
    let keyed = KeyedValues::of(st, id, pose_layer.as_deref(), &start);
    let mut next = keyed.shown.clone();
    let mut toggled = Vec::new();
    if section(ui, k, st, HANDS_SECTION, "Hands") {
        for (side, name) in ["Left", "Right"].into_iter().enumerate() {
            let mark = keyed.mark(Channel::Hand(side));
            let picked = hand_row(
                ui,
                k,
                name,
                waiting,
                ui_id.with(("hand", side)),
                side,
                &mut next.hands[side],
                mark,
            );
            if picked {
                toggled.push(Channel::Hand(side));
            }
        }
        section_end(ui, k);
    }
    if section(ui, k, st, FACE_SECTION, "Expression") {
        let mut face = next.face.unwrap_or(start);
        preset_row(ui, k, st, waiting, ui_id.with("preset"), &mut face);
        let eyebrow_name = |n: i32| rk_chara::face::eyebrow_label(n).map_or_else(|| n.to_string(), str::to_owned);
        let mouth_name = |n: i32| rk_chara::face::mouth_label(n).map_or_else(|| n.to_string(), str::to_owned);
        let eye_name = |n: i32| rk_chara::face::eye_label(n).map_or_else(|| n.to_string(), str::to_owned);
        for (set, name, label_of, value, channel) in [
            (
                Pictures::Eyebrows,
                "Eyebrows",
                &eyebrow_name as &dyn Fn(i32) -> String,
                &mut face.eyebrows,
                Channel::Eyebrows,
            ),
            (Pictures::Eyes, "Eyes", &eye_name, &mut face.eyes, Channel::Eyes),
            (Pictures::Mouth, "Mouth", &mouth_name, &mut face.mouth, Channel::Mouth),
        ] {
            let mark = keyed.mark(channel);
            if pattern_row(
                ui,
                k,
                st,
                id,
                set,
                name,
                waiting,
                ui_id.with(name),
                label_of,
                value,
                mark,
            ) {
                toggled.push(channel);
            }
        }
        let amount = |ui: &mut Ui, name: &str, value: &mut f32, channel: Channel, toggled: &mut Vec<Channel>| {
            row(ui, k, name, waiting, |ui, rect| {
                let (rect, click) = key_mark(ui, k, rect, ui_id.with((name, "key")), keyed.mark(channel));
                widgets::number(ui, k, rect, ui_id.with(name), value, Marker::None, true, FINE);
                if click {
                    toggled.push(channel);
                }
            });
        };
        // Studio's eye look patterns (`EyeLookCalc.eyeTypeStates`): ahead,
        // at the camera, or away from it.
        let card_look = st.live.as_ref().and_then(|l| l.card_eyes_look(id));
        row(
            ui,
            k,
            "Eyes look",
            waiting.or(card_look.is_none().then_some("No eye look")),
            |ui, rect| {
                let patterns = [0, 1, 3, crate::live::EYES_ON_OBJECT];
                let options = [("Front", true), ("Camera", true), ("Away", true), ("Object", true)];
                let now = next.eyes_look.or(card_look).unwrap_or(0);
                let chosen = patterns.iter().position(|&p| p == now);
                if let Some(i) = choices(ui, k, rect, ui_id.with("eyes look"), chosen, &options) {
                    next.eyes_look = Some(patterns[i]);
                }
            },
        );
        // The object the eyes follow, while they follow one: any object of
        // the scene but this character and the folders.
        if next.eyes_look == Some(crate::live::EYES_ON_OBJECT) {
            let scene = st.doc.scene();
            let objects: Vec<(u64, &str)> = scene
                .objects()
                .iter()
                .filter(|o| o.id != id && !matches!(o.content, Content::Folder))
                .map(|o| (o.id.0, o.name.as_str()))
                .collect();
            let mut names: Vec<&str> = vec!["Camera"];
            names.extend(objects.iter().map(|(_, n)| *n));
            let at = next
                .eyes_target
                .and_then(|t| objects.iter().position(|(o, _)| *o == t))
                .map_or(0, |i| i + 1);
            let mut chosen = at;
            row(ui, k, "Look at", waiting, |ui, rect| {
                widgets::combo(ui, k, rect, ui_id.with("look at"), &mut chosen, &names);
            });
            if chosen != at {
                next.eyes_target = chosen.checked_sub(1).map(|i| objects[i].0);
            }
        }
        // Studio's neck buttons (`MPCharCtrl.NeckInfo`): ahead, at the
        // camera, the animation's own turn, or held where it is.
        let card_neck = st.live.as_ref().and_then(|l| l.card_neck_look(id));
        row(
            ui,
            k,
            "Neck look",
            waiting.or(card_neck.is_none().then_some("No neck look")),
            |ui, rect| {
                let patterns = [0, 1, 3, 4];
                let options = [("Front", true), ("Camera", true), ("Anim", true), ("Fixed", true)];
                let now = next.neck_look.or(card_neck).unwrap_or(0);
                let chosen = patterns.iter().position(|&p| p == now);
                if let Some(i) = choices(ui, k, rect, ui_id.with("neck look"), chosen, &options) {
                    next.neck_look = Some(patterns[i]);
                }
            },
        );
        amount(ui, "Eyes open", &mut face.eyes_open, Channel::EyesOpen, &mut toggled);
        row(ui, k, "Blink", waiting, |ui, rect| {
            let (rect, click) = key_mark(ui, k, rect, ui_id.with("blink key"), keyed.mark(Channel::Blink));
            widgets::checkbox(ui, k, rect, ui_id.with("blink"), &mut face.blink, "Blink");
            if click {
                toggled.push(Channel::Blink);
            }
        });
        amount(ui, "Mouth open", &mut face.mouth_open, Channel::MouthOpen, &mut toggled);
        row(ui, k, "Tears", waiting, |ui, rect| {
            let (rect, click) = key_mark(ui, k, rect, ui_id.with("tears key"), keyed.mark(Channel::Tears));
            let options = [("None", true), ("Small", true), ("Middle", true), ("Large", true)];
            let chosen = usize::from(face.tears.min(3));
            if let Some(i) = choices(ui, k, rect, ui_id.with("tears"), Some(chosen), &options) {
                face.tears = i as u8;
            }
            if click {
                toggled.push(Channel::Tears);
            }
        });
        amount(ui, "Blush", &mut face.blush, Channel::Blush, &mut toggled);
        row(ui, k, "Highlights", waiting, |ui, rect| {
            let (rect, click) = key_mark(
                ui,
                k,
                rect,
                ui_id.with("highlights key"),
                keyed.mark(Channel::Highlights),
            );
            let mut shown = !face.hide_highlight;
            widgets::checkbox(ui, k, rect, ui_id.with("highlights"), &mut shown, "Shown");
            face.hide_highlight = !shown;
            if click {
                toggled.push(Channel::Highlights);
            }
        });
        // KK_ExpressionControl's eye roll and smaller irises.
        amount(ui, "Eye roll", &mut face.eye_y, Channel::EyeRoll, &mut toggled);
        amount(
            ui,
            "Small irises",
            &mut face.eye_small,
            Channel::SmallIrises,
            &mut toggled,
        );
        row(ui, k, "Shaking", waiting, |ui, rect| {
            let (rect, click) = key_mark(ui, k, rect, ui_id.with("shaking key"), keyed.mark(Channel::Shaking));
            widgets::checkbox(
                ui,
                k,
                rect,
                ui_id.with("shaking"),
                &mut face.shaking,
                "Highlights shake",
            );
            if click {
                toggled.push(Channel::Shaking);
            }
        });
        face.blush = face.blush.clamp(0.0, 1.0);
        face.eye_y = face.eye_y.clamp(0.0, 0.5);
        face.eye_small = face.eye_small.clamp(0.0, 1.0);
        face.eyes_open = face.eyes_open.clamp(0.0, 1.0);
        face.mouth_open = face.mouth_open.clamp(0.0, 1.0);
        if face != next.face.unwrap_or(start) {
            next.face = Some(face);
        }
        section_end(ui, k);
    }
    let held = ui.input(|i| i.pointer.any_down());
    keyed.commit(st, id, pose_layer, &next, &toggled, held);
}

/// The values that take keys (`rk_chara::channels`) of a character as
/// the Properties show them: the pose layer with their keys at the
/// playhead, and each value's keys.
struct KeyedValues {
    start: rk_chara::face::Face,
    /// The layer as stored.
    layer: rk_chara::pose::Layer,
    /// The layer with the values' keys at the playhead.
    shown: rk_chara::pose::Layer,
    /// Each keyed value's keys: whether it has any, and one at the
    /// playhead.
    marks: std::collections::HashMap<String, (bool, bool)>,
}

impl KeyedValues {
    fn of(
        st: &State,
        id: crate::scene::ObjectId,
        layer: Option<&rk_chara::pose::Layer>,
        start: &rk_chara::face::Face,
    ) -> Self {
        use rk_chara::channels::Channel;
        let layer = layer.cloned().unwrap_or_default();
        let mut shown = layer.clone();
        let mut marks = std::collections::HashMap::new();
        if let Some(track) = st.doc.scene().sequence.track(id) {
            let mut at = rk_chara::keys::pose_at(&track.keys, st.clock);
            at.retain(|name, _| Channel::of_key(name).is_some());
            shown.with_keys_from(at, start);
            let now = crate::keying::now(st);
            let near = st.doc.scene().sequence.near();
            for (name, list) in &track.keys {
                if Channel::of_key(name).is_some() {
                    let here = list.iter().any(|key| (key.time - now).abs() <= near);
                    marks.insert(name.clone(), (true, here));
                }
            }
        }
        KeyedValues {
            start: *start,
            layer,
            shown,
            marks,
        }
    }

    fn mark(&self, channel: rk_chara::channels::Channel) -> (bool, bool) {
        self.marks.get(&channel.key()).copied().unwrap_or_default()
    }

    /// Where the edits go: a keyed value's to its key at the playhead, the
    /// others' to the pose layer. A diamond clicked sets the value's key
    /// at the playhead, or takes it away when there is one.
    fn commit(
        &self,
        st: &mut State,
        id: crate::scene::ObjectId,
        pose_layer: &mut Option<Box<rk_chara::pose::Layer>>,
        next: &rk_chara::pose::Layer,
        toggled: &[rk_chara::channels::Channel],
        held: bool,
    ) {
        use rk_chara::channels::Channel;
        let mut out = self.layer.clone();
        out.eyes_look = next.eyes_look;
        out.neck_look = next.neck_look;
        out.eyes_target = next.eyes_target;
        for channel in Channel::all() {
            let (was, now) = (channel.get(&self.shown, &self.start), channel.get(next, &self.start));
            if was == now {
                continue;
            }
            if self.mark(channel).0 {
                crate::keying::write_value(st, id, &channel.key(), now, held);
            } else {
                channel.set(&mut out, &self.start, now);
            }
        }
        for &channel in toggled {
            if self.mark(channel).1 {
                crate::keying::delete_keys(st, id, &[channel.key()]);
            } else {
                crate::keying::write_value(st, id, &channel.key(), channel.get(next, &self.start), false);
            }
        }
        if out != self.layer {
            *pose_layer = (!out.is_empty()).then(|| Box::new(out));
        }
    }
}

/// A value's key diamond at the right of its row: hollow with no keys,
/// half with keys elsewhere, full with one at the playhead. Gives the rest
/// of the row and whether the diamond was clicked.
fn key_mark(ui: &mut Ui, k: &Look, rect: Rect, id: egui::Id, (any, here): (bool, bool)) -> (Rect, bool) {
    let side = 18.0;
    let cell = Rect::from_min_max(Pos2::new(rect.right() - side, rect.top()), rect.max);
    let resp = ui
        .interact(cell, id, Sense::click())
        .on_hover_text(if here { "Delete key" } else { "Set key" });
    let c = cell.center();
    let r = 4.5;
    let points = vec![
        c + Vec2::new(0.0, -r),
        c + Vec2::new(r, 0.0),
        c + Vec2::new(0.0, r),
        c + Vec2::new(-r, 0.0),
    ];
    let line = if resp.hovered() { k.t.text_bright } else { k.t.text_dim };
    let p = ui.painter();
    if here {
        p.add(egui::Shape::convex_polygon(
            points,
            k.t.select,
            egui::Stroke::new(1.0, line),
        ));
    } else {
        p.add(egui::Shape::closed_line(points.clone(), egui::Stroke::new(1.0, line)));
        if any {
            // Half filled: keys at other times.
            p.add(egui::Shape::convex_polygon(
                vec![points[0], points[2], points[3]],
                line,
                egui::Stroke::NONE,
            ));
        }
    }
    (rect.with_max_x(cell.left() - 4.0), resp.clicked())
}

/// KK_SkinEffects: a level for each of its effects, kept in the pose so
/// that it can be keyed with it.
fn skin_effects_section(
    ui: &mut Ui,
    k: &Look,
    st: &mut State,
    id: crate::scene::ObjectId,
    waiting: Option<&str>,
    pose_layer: &mut Option<Box<rk_chara::pose::Layer>>,
) {
    use rk_chara::channels::Channel;
    if !section(ui, k, st, SKIN_SECTION, "Skin effects") {
        return;
    }
    let ui_id = ui.id().with("skin effects");
    let start = st
        .live
        .as_ref()
        .and_then(|l| l.face_rig(id))
        .map(|r| r.start)
        .unwrap_or_default();
    let keyed = KeyedValues::of(st, id, pose_layer.as_deref(), &start);
    let mut next = keyed.shown.clone();
    let mut toggled = Vec::new();
    for (i, effect) in rk_chara::skin_effects::EFFECTS.iter().enumerate() {
        row(ui, k, effect.name, waiting, |ui, rect| {
            let (rect, click) = key_mark(ui, k, rect, ui_id.with((i, "key")), keyed.mark(Channel::Skin(i)));
            let names = ["None", "1", "2", "3"];
            let options: Vec<(&str, bool)> = names[..=usize::from(effect.levels)]
                .iter()
                .map(|n| (*n, true))
                .collect();
            let chosen = usize::from(next.skin[i].min(effect.levels));
            if let Some(level) = choices(ui, k, rect, ui_id.with(i), Some(chosen), &options) {
                next.skin[i] = level as u8;
            }
            if click {
                toggled.push(Channel::Skin(i));
            }
        });
    }
    section_end(ui, k);
    keyed.commit(st, id, pose_layer, &next, &toggled, false);
}

/// BetterPenetration for a character with a shaft: the plugin's settings,
/// with its sliders' ranges, and the partner it aims at, which in Studio
/// NodesConstraints holds. Settings left as the card has them stay the
/// card's.
fn penetration_section(
    ui: &mut Ui,
    k: &Look,
    st: &mut State,
    object: &mut crate::scene::Object,
    card: rk_chara::better_penetration::Settings,
) {
    use rk_chara::better_penetration::{AutoTarget, Mode};
    let others: Vec<(crate::scene::ObjectId, String)> = st
        .doc
        .scene()
        .objects()
        .iter()
        .filter(|o| o.kind() == Kind::Character && o.id != object.id)
        .map(|o| (o.id, o.name.clone()))
        .collect();
    // The partners whose body has the bones a vaginal or anal pull moves.
    let pullable: Vec<crate::scene::ObjectId> = others
        .iter()
        .filter(|(id, _)| {
            st.live
                .as_ref()
                .and_then(|l| l.built(*id))
                .is_some_and(|b| b.pull_bones)
        })
        .map(|(id, _)| *id)
        .collect();
    // The partners whose body has belly bones a bulge can push.
    let bellies: Vec<crate::scene::ObjectId> = others
        .iter()
        .filter(|(id, _)| st.live.as_ref().and_then(|l| l.built(*id)).is_some_and(|b| b.belly))
        .map(|(id, _)| *id)
        .collect();
    if !section(ui, k, st, PENETRATION_SECTION, "Penetration") {
        return;
    }
    let Content::Character {
        better_penetration,
        penetration_target,
        ..
    } = &mut object.content
    else {
        return;
    };
    let ui_id = ui.id().with("penetration");
    let mut s = better_penetration.as_deref().copied().unwrap_or(card);
    row(ui, k, "Enabled", None, |ui, rect| {
        widgets::checkbox(ui, k, rect, ui_id.with("enabled"), &mut s.enabled, "On");
    });
    let off = (!s.enabled).then_some("BetterPenetration is off");
    row(ui, k, "Partner", off, |ui, rect| {
        let name = penetration_target
            .and_then(|t| others.iter().find(|(id, _)| *id == t.partner))
            .map_or("None", |(_, name)| name.as_str());
        let resp = widgets::dropdown_field(ui, k, rect, ui_id.with("partner"), name);
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(rect.width());
            if widgets::check_item(ui, k, penetration_target.is_none(), "None", "").clicked() {
                *penetration_target = None;
                ui.close();
            }
            for (id, name) in &others {
                let on = penetration_target.is_some_and(|t| t.partner == *id);
                if widgets::check_item(ui, k, on, name, "").clicked() {
                    let mode = penetration_target.map_or(Mode::Vaginal, |t| t.mode);
                    *penetration_target = Some(crate::scene::PenetrationTarget { partner: *id, mode });
                    ui.close();
                }
            }
        });
    });
    if let Some(target) = penetration_target.as_mut() {
        row(ui, k, "Target", off, |ui, rect| {
            let options: Vec<(&str, bool)> = Mode::ALL.iter().map(|m| (m.label(), true)).collect();
            let chosen = Mode::ALL.iter().position(|m| *m == target.mode);
            if let Some(i) = choices(ui, k, rect, ui_id.with("target"), chosen, &options) {
                target.mode = Mode::ALL[i];
            }
        });
    } else {
        // Auto is Renkatsu's own link, by the game's H positions or the
        // nearest place; the rest are the plugin's auto-target.
        row(ui, k, "Auto-target", off, |ui, rect| {
            let mut options: Vec<&str> = vec!["Auto"];
            options.extend(AutoTarget::ALL.iter().map(|a| a.label()));
            let mut chosen = if s.auto_target == AutoTarget::Off && s.auto_link {
                0
            } else {
                AutoTarget::ALL
                    .iter()
                    .position(|a| *a == s.auto_target)
                    .map_or(1, |i| i + 1)
            };
            widgets::combo(ui, k, rect, ui_id.with("auto"), &mut chosen, &options);
            s.auto_link = chosen == 0;
            s.auto_target = if chosen == 0 {
                AutoTarget::Off
            } else {
                AutoTarget::ALL[chosen - 1]
            };
        });
    }
    // Where the shaft goes, if anywhere: the partner's place, or the one
    // auto-target looks for.
    let aimed = match penetration_target {
        Some(t) => Some(t.mode),
        None if s.auto_target == AutoTarget::Off && s.auto_link => {
            st.live.as_ref().and_then(|l| l.aim(object.id)).map(|a| a.mode)
        }
        None => Mode::of_auto(s.auto_target),
    };
    // The plugin's sliders, by their ranges in Studio, each where it does
    // something: the squish while the shaft is aimed; the colliders'
    // size always, since his own balls meet two of them; the pull and its
    // return only on a mouth, the one bone the plugin pulls in Studio.
    let mut sliders: Vec<(&str, &mut f32, f32, f32)> = Vec::new();
    if aimed.is_some() {
        sliders.extend([
            ("Length squish", &mut s.length_squish, 0.0, 1.0),
            ("Girth squish", &mut s.girth_squish, 0.0, 2.0),
            ("Threshold", &mut s.squish_threshold, 0.0, 1.0),
        ]);
    }
    sliders.extend([
        ("Hit radius", &mut s.collider_radius_scale, 0.5, 1.5),
        ("Hit length", &mut s.collider_length_scale, 0.5, 1.5),
    ]);
    let pull_rows = match penetration_target {
        Some(t) => pullable.contains(&t.partner),
        None => !pullable.is_empty(),
    } && matches!(aimed, Some(Mode::Vaginal | Mode::Anal));
    if pull_rows {
        row(ui, k, "Pull", off, |ui, rect| {
            widgets::checkbox(ui, k, rect, ui_id.with("pull holes"), &mut s.pull_holes, "On");
        });
    }
    if aimed == Some(Mode::Oral) || (pull_rows && s.pull_holes) {
        sliders.extend([
            ("Max push", &mut s.max_push, 0.0, 0.3),
            ("Max pull", &mut s.max_pull, 0.0, 0.3),
            ("Pull rate", &mut s.pull_rate, 0.0, 50.0),
            // How fast the mouth returns with no stroke in it, as the
            // plugin does in the main game; 0 holds it as Studio does.
            ("Return rate", &mut s.return_rate, 0.0, 1.0),
        ]);
    }
    // Renkatsu's own: her mouth holds the shaft while it is in it.
    if aimed == Some(Mode::Oral) {
        row(ui, k, "Mouth", off, |ui, rect| {
            widgets::checkbox(ui, k, rect, ui_id.with("oral mouth"), &mut s.oral_mouth, "Hold");
        });
    }
    for (name, value, low, high) in sliders {
        row(ui, k, name, off, |ui, rect| {
            widgets::number_to(ui, k, rect, ui_id.with(name), value, Marker::None, true, FINE, 3);
            *value = value.clamp(low, high);
        });
    }
    // Renkatsu's own: the shaft follows her body inside, as the plugin does
    // in the main game, rather than ending at her waist as in Studio.
    if matches!(aimed, Some(Mode::Vaginal | Mode::Anal)) {
        row(ui, k, "Body", off, |ui, rect| {
            widgets::checkbox(ui, k, rect, ui_id.with("body"), &mut s.body_collision, "Follow");
        });
    }
    // The plugin keeps this on the card without a control in Studio: the
    // shaft's deeper points push her belly's dynamic bone.
    let belly = match penetration_target {
        Some(t) => bellies.contains(&t.partner),
        None => !bellies.is_empty(),
    };
    if aimed == Some(Mode::Vaginal) && belly {
        row(ui, k, "Belly", off, |ui, rect| {
            widgets::checkbox(ui, k, rect, ui_id.with("belly"), &mut s.belly_bulge, "Bulge");
        });
        // The plugin has no range for it; Renkatsu offers up to three times.
        if s.belly_bulge {
            row(ui, k, "Bulge size", off, |ui, rect| {
                widgets::number_to(
                    ui,
                    k,
                    rect,
                    ui_id.with("bulge size"),
                    &mut s.belly_bulge_scale,
                    Marker::None,
                    true,
                    FINE,
                    3,
                );
                s.belly_bulge_scale = s.belly_bulge_scale.clamp(0.5, 3.0);
            });
        }
    }
    section_end(ui, k);
    *better_penetration = (s != card).then(|| Box::new(s));
}

/// The file of Renkatsu's own expression presets, beside the settings.
fn expressions_path() -> Option<std::path::PathBuf> {
    Some(crate::settings::Settings::path()?.with_file_name("expressions.toml"))
}

/// Expression presets, as ExpressionControl keeps them: a pick sets the
/// whole face. Renkatsu's own come first, then the plugin's from the game
/// folder; the current face can be saved under a name, and an own preset
/// deleted.
fn preset_row(
    ui: &mut Ui,
    k: &Look,
    st: &mut State,
    off: Option<&str>,
    ui_id: egui::Id,
    face: &mut rk_chara::face::Face,
) {
    use rk_chara::expression_control as ec;
    if st.expressions.is_none() {
        let own = expressions_path()
            .and_then(|p| ec::read_saved(&p).ok())
            .unwrap_or_default();
        let plugin = rk_core::Config::load()
            .ok()
            .map(|c| {
                [
                    ("ExpressionControl", ec::of_game(&c.game_dir)),
                    ("Ahegao", rk_chara::ahegao::of_game(&c.game_dir)),
                ]
                .into_iter()
                .filter(|(_, presets)| !presets.is_empty())
                .collect()
            })
            .unwrap_or_default();
        st.expressions = Some((own, plugin));
    }
    let (own, plugin) = st.expressions.clone().unwrap_or_default();
    // Presets do not shake the eyes: a pick keeps the shaking as it is.
    let same = |a: &rk_chara::face::Face, b: &rk_chara::face::Face| {
        rk_chara::face::Face {
            shaking: b.shaking,
            ..*a
        } == *b
    };
    let shown = own
        .iter()
        .chain(plugin.iter().flat_map(|(_, presets)| presets))
        .find(|p| same(&p.face, face))
        .map_or("Custom", |p| p.name.as_str())
        .to_owned();
    let mut save = false;
    let mut delete = None;
    let mut name = st.expression_name.clone();
    if name.is_empty() {
        name = format!("Expression {}", own.len() + 1);
    }
    row(ui, k, "Preset", off, |ui, rect| {
        let resp = widgets::dropdown_field(ui, k, rect, ui_id, &shown);
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(rect.width());
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (i, preset) in own.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if widgets::check_item(ui, k, same(&preset.face, face), &preset.name, "").clicked() {
                            *face = rk_chara::face::Face {
                                shaking: face.shaking,
                                ..preset.face
                            };
                            ui.close();
                        }
                        if icon_button(ui, k, icon::TRASH, false).on_hover_text("Delete").clicked() {
                            delete = Some(i);
                        }
                    });
                }
                for (heading, presets) in &plugin {
                    ui.label(egui::RichText::new(*heading).color(k.t.text_dim).size(k.d.text - 1.0));
                    for preset in presets {
                        if widgets::check_item(ui, k, same(&preset.face, face), &preset.name, "").clicked() {
                            *face = rk_chara::face::Face {
                                shaking: face.shaking,
                                ..preset.face
                            };
                            ui.close();
                        }
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut name).desired_width(rect.width() - 70.0));
                if ui.button("Save").clicked() {
                    save = true;
                }
            });
        });
    });
    st.expression_name = name.clone();
    if save || delete.is_some() {
        let mut own = own;
        if let Some(i) = delete {
            own.remove(i);
        }
        if save {
            own.retain(|p| p.name != name);
            own.push(ec::Preset {
                name: name.trim().to_owned(),
                face: *face,
            });
            st.expression_name.clear();
        }
        match expressions_path().map(|p| ec::save(&p, &own)) {
            Some(Err(e)) => st.log.error(format!("Could not save the expressions: {e:#}")),
            _ => {
                if let Some((saved, _)) = st.expressions.as_mut() {
                    *saved = own;
                }
            }
        }
    }
}

/// A hand's pattern as CharaStudio steps through them, arrows to either
/// side (`PatternInfo`), with the patterns by name in a list between; 0
/// is the animation's own hand. Returns whether the key mark was clicked.
#[allow(clippy::too_many_arguments)]
fn hand_row(
    ui: &mut Ui,
    k: &Look,
    name: &str,
    off: Option<&str>,
    ui_id: egui::Id,
    side: usize,
    value: &mut i32,
    key: (bool, bool),
) -> bool {
    let mut options: Vec<(i32, String)> = vec![(0, "Animation".to_owned())];
    if let Some(lists) = crate::live::hand_lists_read() {
        options.extend(lists[side].iter().map(|(n, a)| (*n, a.label())));
    }
    if !options.iter().any(|(n, _)| n == value) {
        options.push((*value, format!("Pattern {value}")));
    }
    let mut clicked = false;
    row(ui, k, name, off, |ui, rect| {
        let (rect, click) = key_mark(ui, k, rect, ui_id.with("key"), key);
        clicked = click;
        let side_w = rect.height();
        let left = Rect::from_min_size(rect.min, Vec2::splat(side_w));
        let right = Rect::from_min_size(Pos2::new(rect.right() - side_w, rect.top()), Vec2::splat(side_w));
        let field = Rect::from_min_max(
            Pos2::new(left.right() + 2.0, rect.top()),
            Pos2::new(right.left() - 2.0, rect.bottom()),
        );
        let at = options.iter().position(|(n, _)| n == value).unwrap_or(0);
        for (cell, step, glyph) in [(left, -1i32, icon::CARET_LEFT), (right, 1, icon::CARET_RIGHT)] {
            let resp = ui.interact(cell, ui_id.with(("step", step)), Sense::click());
            widgets::icon_cell(ui, k, cell, &resp, glyph, false);
            if resp.clicked() {
                let count = options.len() as i32;
                *value = options[(at as i32 + step).rem_euclid(count) as usize].0;
            }
        }
        let mut chosen = at;
        let names: Vec<&str> = options.iter().map(|(_, s)| s.as_str()).collect();
        widgets::combo(ui, k, field, ui_id.with("list"), &mut chosen, &names);
        if chosen != at {
            *value = options[chosen].0;
        }
    });
    clicked
}

/// A pattern as its picture, which opens a grid of all the pictures to
/// pick from. Without pictures yet the pattern's name stands in.
#[allow(clippy::too_many_arguments)]
fn pattern_row(
    ui: &mut Ui,
    k: &Look,
    st: &mut State,
    id: crate::scene::ObjectId,
    set: crate::live::Pictures,
    name: &str,
    off: Option<&str>,
    ui_id: egui::Id,
    label_of: &dyn Fn(i32) -> String,
    value: &mut i32,
    key: (bool, bool),
) -> bool {
    let mut clicked = false;
    let pictures: Vec<(i32, std::sync::Arc<egui::ColorImage>)> = st
        .live
        .as_mut()
        .and_then(|l| l.pictures(id, set))
        .map(<[_]>::to_vec)
        .unwrap_or_default();
    // Drawn at twice the size they show in the row, sharper on any screen
    // and larger in the grid.
    let [w, h] = set.size();
    let (w, h) = (w as f32 * 0.25, h as f32 * 0.25);
    sized_row(ui, k, name, off, (h + 4.0 - k.d.row).max(0.0), |ui, rect| {
        let (rect, click) = key_mark(ui, k, rect, ui_id.with("key"), key);
        clicked = click;
        let field = Rect::from_min_size(rect.min, Vec2::new(rect.width(), h + 4.0));
        let resp = ui.interact(field, ui_id, Sense::click());
        let p = ui.painter();
        p.rect_filled(field, k.radius(), if resp.hovered() { k.t.hover } else { k.t.control });
        let tile = Rect::from_min_size(field.min + Vec2::splat(2.0), Vec2::new(w, h));
        let current = pictures.iter().find(|(n, _)| n == value);
        match current {
            Some((_, image)) => {
                let texture = picture_texture(ui, image);
                p.image(
                    texture.id(),
                    tile,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                p.rect_filled(tile, 2.0, k.t.header);
            }
        }
        label(
            &p.with_clip_rect(field.with_max_x(field.right() - 22.0)),
            k,
            Pos2::new(tile.right() + 8.0, field.center().y),
            &label_of(*value),
            k.t.text,
        );
        glyph(
            p,
            k,
            Pos2::new(field.right() - 12.0, field.center().y),
            widgets::caret_of(&resp),
            10.0,
            k.t.text,
        );
        egui::Popup::menu(&resp).show(|ui| {
            let columns = 4;
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                egui::Grid::new(ui_id.with("grid"))
                    .spacing(Vec2::splat(4.0))
                    .show(ui, |ui| {
                        for (i, (n, image)) in pictures.iter().enumerate() {
                            let (cell, cell_resp) = ui.allocate_exact_size(Vec2::new(w * 2.0, h * 2.0), Sense::click());
                            let texture = picture_texture(ui, image);
                            ui.painter().image(
                                texture.id(),
                                cell,
                                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                Color32::WHITE,
                            );
                            let ring = if n == value {
                                k.t.select
                            } else if cell_resp.hovered() {
                                k.t.text_bright
                            } else {
                                Color32::TRANSPARENT
                            };
                            ui.painter()
                                .rect_stroke(cell, 2.0, egui::Stroke::new(1.5, ring), egui::StrokeKind::Inside);
                            if cell_resp.on_hover_text(label_of(*n)).clicked() {
                                *value = *n;
                                ui.close();
                            }
                            if (i + 1) % columns == 0 {
                                ui.end_row();
                            }
                        }
                    });
            });
        });
    });
    clicked
}

/// A pattern picture's texture, kept for as long as the picture.
fn picture_texture(ui: &Ui, image: &std::sync::Arc<egui::ColorImage>) -> egui::TextureHandle {
    let key = egui::Id::new(("pattern picture", std::sync::Arc::as_ptr(image) as usize));
    if let Some(texture) = ui.ctx().data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        return texture;
    }
    let texture = ui
        .ctx()
        .load_texture("pattern picture", (**image).clone(), egui::TextureOptions::LINEAR);
    ui.ctx().data_mut(|d| d.insert_temp(key, texture.clone()));
    texture
}

fn pose_sections(ui: &mut Ui, k: &Look, st: &mut State, id: crate::scene::ObjectId, content: &mut Content) {
    let Content::Character { pose_layer, .. } = content else {
        return;
    };
    let ui_id = ui.id().with("pose");
    if !pose_layer.as_ref().is_some_and(|l| l.fk_on) {
        return;
    }
    let joints = st.live.as_ref().and_then(|l| l.joints(id));
    let waiting = joints.is_none().then_some("The character is not built");
    if section(ui, k, st, 16, "Bone") {
        let name = st
            .pose_bone
            .clone()
            .filter(|n| joints.as_ref().is_some_and(|j| j.iter().any(|(_, b)| b == n)));
        let edit = name
            .as_deref()
            .map(|n| crate::keying::shown(st, id, n))
            .unwrap_or_default();
        let off = waiting.or(name.is_none().then_some("No bone chosen"));
        row(ui, k, "Bone", off, |ui, rect| {
            let shown = name
                .as_deref()
                .map_or_else(|| "None".to_owned(), crate::bones::pose_label);
            label(
                ui.painter(),
                k,
                Pos2::new(rect.left() + 4.0, rect.center().y),
                &shown,
                k.t.text,
            );
            if let Some(raw) = name.as_deref().filter(|raw| *raw != shown) {
                ui.interact(rect, ui_id.with("bone name"), Sense::hover())
                    .on_hover_text(raw);
            }
        });
        // The turn over the animation, in Unity's angles. A trace of
        // rounding is no turn, not a minus zero.
        let (y, x, z) = edit.rot.to_euler(glam::EulerRot::YXZ);
        let tidy = |a: f32| if a.abs() < 1e-4 { 0.0 } else { a };
        let shown = [tidy(x.to_degrees()), tidy(y.to_degrees()), tidy(z.to_degrees())];
        let mut angles = shown;
        let mut reset = false;
        let turned = edit.rot != glam::Quat::IDENTITY;
        row(ui, k, "Rotation", off, |ui, rect| {
            // The angles, and at their end a way back to the animation's
            // turn while the bone is turned by hand.
            let side = rect.height();
            let fields = Rect::from_min_max(rect.min, Pos2::new(rect.right() - side - 4.0, rect.bottom()));
            vec3(ui, k, fields, ui_id.with("rotation"), &mut angles, 0.5);
            if turned {
                let button = Rect::from_min_size(Pos2::new(fields.right() + 4.0, rect.top()), Vec2::splat(side));
                let resp = ui
                    .interact(button, ui_id.with("reset"), Sense::click())
                    .on_hover_text("Reset");
                let fill = if resp.hovered() { k.t.hover } else { k.t.control };
                ui.painter().rect_filled(button, k.radius(), fill);
                glyph(
                    ui.painter(),
                    k,
                    button.center(),
                    icon::ARROW_COUNTER_CLOCKWISE,
                    14.0,
                    k.t.text,
                );
                reset = resp.clicked();
            }
        });
        if let Some(name) = name.filter(|_| reset || angles != shown) {
            let rot = if reset {
                glam::Quat::IDENTITY
            } else {
                rk_chara::scene::euler_deg(angles[0], angles[1], angles[2])
            };
            let edit = rk_chara::pose::BoneEdit { rot, ..edit };
            if crate::keying::keyed(st, id, &name) {
                crate::keying::write(st, id, &name, edit, false);
            } else {
                let layer = pose_layer.get_or_insert_with(Default::default);
                if edit.is_identity() {
                    layer.fk.remove(&name);
                } else {
                    layer.fk.insert(name, edit);
                }
            }
        }
        section_end(ui, k);
    }
}

/// Who the character is (part 1), what it likes (part 2) or what it is
/// like in bed (part 3), as sections of the Profile page. The values start
/// as the card's; changing one keeps the change in the scene, for this
/// character only.
fn profile_page(ui: &mut Ui, k: &Look, st: &mut State, p: &mut Profile, page: usize) {
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
        if section(ui, k, st, 12, "H traits") {
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
}

/// The way back to the card's own profile, likes and traits.
fn card_data(ui: &mut Ui, k: &Look, st: &mut State, p: &mut Profile, card: &Profile) {
    if section(ui, k, st, 11, "Card data") {
        let from_card = Profile {
            male: p.male,
            ..card.clone()
        };
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
fn header(ui: &mut Ui, k: &Look, st: &mut State, glyph_: &str, kind_label: &str, color: Color32, name: &str) {
    // The type says nothing new under a name that is the type.
    let kind_label = if kind_label == name { "" } else { kind_label };
    let t = &k.t;
    let h = k.d.row + 14.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
    let y = rect.center().y + 1.0;
    let p = ui.painter();
    match k.d.props {
        Props::Inspector => {
            // Unity's header: a field holding the name, the type beside it.
            let kind_w = widgets::text_width(ui, kind_label, k.font()).ceil();
            let field = Rect::from_min_max(
                Pos2::new(rect.left() + 34.0, y - k.field() / 2.0),
                Pos2::new(rect.right() - kind_w - 18.0, y + k.field() / 2.0),
            );
            glyph(p, k, Pos2::new(rect.left() + 19.0, y), glyph_, 17.0, color);
            widgets::field_box(p, k, field, false, false);
            p.text(
                Pos2::new(field.left() + 7.0, y),
                Align2::LEFT_CENTER,
                name,
                k.font(),
                t.text_bright,
            );
            label(p, k, Pos2::new(field.right() + 8.0, y), kind_label, t.text_dim);
        }
        _ => {
            glyph(p, k, Pos2::new(rect.left() + 19.0, y), glyph_, 18.0, color);
            let x = p
                .text(
                    Pos2::new(rect.left() + 34.0, y),
                    Align2::LEFT_CENTER,
                    name,
                    k.bold(k.d.text + 1.0),
                    t.text_bright,
                )
                .right();
            label(p, k, Pos2::new(x + 8.0, y), kind_label, t.text_dim);
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
        widgets::search(&mut child, k, &mut st.property_search, "Search", w);
        let query = st.property_search.trim().to_lowercase();
        ui.data_mut(|d| d.insert_temp(Id::new(SEARCH), query));
        let gear = icon_button(&mut child, k, icon::GEAR_SIX, false).on_hover_text("Options");
        egui::Popup::menu(&gear).show(|ui| {
            for (text, open) in [("Expand all", true), ("Collapse all", false)] {
                if ui.button(text).clicked() {
                    st.sections.iter_mut().for_each(|s| *s = open);
                    ui.close();
                }
            }
        });
    }
}

/// The header of a section; returns whether the section is open.
/// Where the rows find what the header's search box holds.
const SEARCH: &str = "property search";

/// What the search box holds, lowercase; empty for nothing.
fn searched(ui: &Ui) -> String {
    ui.data(|d| d.get_temp::<String>(Id::new(SEARCH))).unwrap_or_default()
}

fn section(ui: &mut Ui, k: &Look, st: &mut State, index: usize, title: &str) -> bool {
    // While searching, the matching rows stand alone, without sections.
    if !searched(ui).is_empty() {
        return true;
    }
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
    if !searched(ui).is_empty() {
        return;
    }
    let gap = match k.d.props {
        Props::Details => 0.0,
        Props::Inspector => 5.0,
        Props::Joined => 8.0,
    };
    ui.add_space(gap);
}

/// A map's times of day as their pictures side by side, under each its
/// name; a time the map lacks shows an empty frame and cannot be chosen.
fn time_pictures(
    ui: &mut Ui,
    k: &Look,
    id: Id,
    time: &mut MapTime,
    times: &[MapTime],
    pictures: &[Option<egui::TextureHandle>; 3],
) {
    let t = &k.t;
    const GAP: f32 = 4.0;
    let width = ui.available_width();
    // The pictures take the value column; their height follows its width.
    let (split, right) = value_span(k, width);
    let w = ((right - split - 2.0 * GAP) / 3.0).floor();
    let h = (w * 9.0 / 16.0).round();
    let extra = h + 28.0 - k.field();
    sized_row(ui, k, "Time of day", None, extra, |ui, rect| {
        for (i, when) in MapTime::ALL.into_iter().enumerate() {
            let cell = Rect::from_min_size(
                Pos2::new(rect.left() + i as f32 * (w + GAP), rect.top() + 4.0),
                Vec2::new(w, h),
            );
            let has = times.contains(&when);
            let sense = if has { Sense::click() } else { Sense::hover() };
            let resp = ui.interact(cell.with_max_y(cell.bottom() + 18.0), id.with(i), sense);
            if resp.clicked() && has {
                *time = when;
            }
            let on = *time == when;
            let p = ui.painter();
            p.rect_filled(cell, 0.0, t.input);
            match &pictures[i] {
                Some(texture) => {
                    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                    let tint = if on || resp.hovered() {
                        Color32::WHITE
                    } else {
                        Color32::from_gray(200)
                    };
                    p.image(texture.id(), cell, uv, tint);
                }
                None => widgets::boxed(p, cell, 0.0, t.input, t.border),
            }
            if on {
                p.rect_stroke(cell, 0.0, egui::Stroke::new(2.0, t.select), egui::StrokeKind::Outside);
            } else if resp.hovered() && has {
                p.rect_stroke(cell, 0.0, egui::Stroke::new(1.0, t.hover), egui::StrokeKind::Outside);
            }
            let color = if on {
                t.text_bright
            } else if !has {
                t.border
            } else {
                t.text_dim
            };
            p.text(
                Pos2::new(cell.center().x, cell.bottom() + 10.0),
                Align2::CENTER_CENTER,
                when.label(),
                k.small(),
                color,
            );
        }
    });
}

/// Where the value column of a row as wide as `width` starts and ends,
/// measured from the row's left.
fn value_span(k: &Look, width: f32) -> (f32, f32) {
    let (share, least, from, to) = match k.d.props {
        Props::Details => (0.38, 108.0, 9.0, 10.0),
        Props::Inspector => (0.34, 100.0, 0.0, 8.0),
        Props::Joined => (0.33, 100.0, 0.0, 12.0),
    };
    ((width * share).max(least).round() + from, width - to)
}

/// One property: its name in the left column, and `add` drawing the value
/// into the rectangle of the right column. `off` greys the value and takes
/// it out of use, and is what the pointer is told about why.
fn row(ui: &mut Ui, k: &Look, name: &str, off: Option<&str>, add: impl FnOnce(&mut Ui, Rect)) {
    sized_row(ui, k, name, off, 0.0, add);
}

/// A row as `row`, `extra` taller, its name on the first line and its
/// value growing down from there.
fn sized_row(ui: &mut Ui, k: &Look, name: &str, off: Option<&str>, extra: f32, add: impl FnOnce(&mut Ui, Rect)) {
    let query = searched(ui);
    if query.is_empty() || name.to_lowercase().contains(&query) {
        sized_row_shown(ui, k, name, off, extra, add);
    }
}

fn sized_row_shown(ui: &mut Ui, k: &Look, name: &str, off: Option<&str>, extra: f32, add: impl FnOnce(&mut Ui, Rect)) {
    let t = &k.t;
    let name_color = if off.is_some() { t.text_dim } else { t.text };
    let (h, share, least) = match k.d.props {
        Props::Details => (k.d.row + 2.0, 0.38, 108.0),
        Props::Inspector => (k.d.row + 2.0, 0.34, 100.0),
        Props::Joined => (k.d.row + 4.0, 0.33, 100.0),
    };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h + extra), Sense::hover());
    let split = (rect.left() + (rect.width() * share).max(least)).round();
    // The name stands on the first line, as in a row of one line.
    let y = rect.top() + h / 2.0;
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
        Pos2::new(to, (y - half).round() + k.field() + extra),
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
