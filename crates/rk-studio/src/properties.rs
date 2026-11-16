// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The Properties panel: everything about the selection in sections of
//! rows, with names and values aligned in two columns. The values are the
//! selected object's own, from the scene model; what waits for a later
//! feature is shown greyed, and says what it waits for.

use crate::design::Props;
use crate::kinds::Kind;
use crate::scene::Content;
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
            "Select an object to see its properties",
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
    let built = st.live.as_ref().and_then(|live| live.built(object.id));
    let failure = st
        .live
        .as_ref()
        .and_then(|live| live.failure(object.id))
        .map(str::to_owned);
    let lights = st.doc.scene().key_light().map(|o| o.id) == Some(object.id);
    let holds = st.doc.scene().subtree(object.id).len() - 1;

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let id = ui.id();
        if kind != Kind::Folder && section(ui, k, st, 0, "Transform") {
            // What each of position, rotation and scale waits for, when it
            // does nothing yet for this kind of object.
            let off: [Option<&str>; 3] = match &object.content {
                Content::Character { .. } => [None; 3],
                Content::Light { from_camera, .. } => [
                    Some("A directional light has no position"),
                    (*from_camera).then_some("The light follows the camera. Choose Own under Direction to turn it"),
                    Some("A light has no size"),
                ],
                Content::Camera { .. } => [Some("Shot cameras arrive with baking"); 3],
                _ => [Some("Items and maps arrive with their feature"); 3],
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
        match &mut object.content {
            Content::Character {
                card,
                outfit,
                outdoor_shoes,
                simulate,
            } => {
                if section(ui, k, st, 1, "Character") {
                    row(ui, k, "Card", None, |ui, rect| {
                        let side = rect.height();
                        let field = Rect::from_min_max(rect.min, Pos2::new(rect.right() - side - 4.0, rect.bottom()));
                        let file = card.file_name().unwrap_or_default().to_string_lossy();
                        widgets::value_box(ui, k, field, &file);
                        ui.interact(field, id.with("card"), Sense::hover())
                            .on_hover_text(card.display().to_string());
                        let button = Rect::from_min_size(Pos2::new(field.right() + 4.0, rect.top()), Vec2::splat(side));
                        let resp = ui
                            .interact(button, id.with("browse"), Sense::click())
                            .on_hover_text("Show the cards in the Content panel");
                        let fill = if resp.hovered() { k.t.hover } else { k.t.control };
                        ui.painter().rect_filled(button, k.radius(), fill);
                        glyph(ui.painter(), k, button.center(), icon::FOLDER_OPEN, 14.0, k.t.text);
                        if resp.clicked() {
                            st.show_content = true;
                        }
                    });
                    let outfits: Vec<String> = (1..=built.map_or(1, |b| b.outfits.max(1)))
                        .map(|n| format!("Outfit {n}"))
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
                    } else if let Some(count) = built.map(|b| b.warnings).filter(|n| *n > 0) {
                        row(ui, k, "Warnings", None, |ui, rect| {
                            let text = format!("{count}, listed in the Console");
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
                if section(ui, k, st, 2, "Expression") {
                    let later = Some("Expressions arrive with posing");
                    for name in ["Eyes", "Mouth"] {
                        row(ui, k, name, later, |ui, rect| {
                            widgets::combo(ui, k, rect, id.with(name), &mut 0, &["Default"]);
                        });
                    }
                    section_end(ui, k);
                }
                if section(ui, k, st, 3, "Modules") {
                    let later =
                        Some("Every module runs for every character for now; choosing arrives with the Modules panel");
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
                    row(ui, k, "Type", Some("Point and spot lights arrive later"), |ui, rect| {
                        widgets::combo(ui, k, rect, id.with("light"), &mut 0, &["Directional"]);
                    });
                    row(ui, k, "Colour", Some("The light's colour arrives later"), |ui, rect| {
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
                            let text = "No: the first light that shows lights the scene";
                            ui.interact(rect, id.with("in-use"), Sense::hover())
                                .on_hover_text("The renderer has one light for now");
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
                    let later = Some("Shot cameras arrive with baking");
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
    if object != before {
        let held = ui.input(|i| i.pointer.any_down());
        st.doc.edit(object, held);
    }
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
