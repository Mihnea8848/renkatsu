// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The Map mode's palette: the map's own controls, at hand without
//! selecting the map: its time of day, its light, the effects that follow
//! it (fog, sun shafts, colour filter) and its walls.

use crate::scene::{Content, Effect, MapTime, ObjectId, FOG_PRESETS};
use crate::state::{Look, State};
use crate::widgets::{self, flat_button};
use egui::{Pos2, Rect, Ui, Vec2};
use egui_phosphor::regular as icon;

/// The scene's map, if it has one.
fn map_of(st: &State) -> Option<ObjectId> {
    st.doc
        .scene()
        .objects()
        .iter()
        .find(|o| matches!(o.content, Content::Map { .. }))
        .map(|o| o.id)
}

/// The first effect of a kind in the scene.
fn effect_of(st: &State, same: fn(&Effect) -> bool) -> Option<ObjectId> {
    st.doc
        .scene()
        .objects()
        .iter()
        .find(|o| matches!(&o.content, Content::Effect { effect } if same(effect)))
        .map(|o| o.id)
}

fn is_fog(e: &Effect) -> bool {
    matches!(e, Effect::Fog { .. })
}

fn is_shafts(e: &Effect) -> bool {
    matches!(e, Effect::SunShafts { .. })
}

fn is_filter(e: &Effect) -> bool {
    matches!(e, Effect::ColorFilter { .. })
}

/// Edits an object's content in one step of the history; a drag that is
/// still held stays one step.
fn edit(ui: &Ui, st: &mut State, id: ObjectId, change: impl FnOnce(&mut Content)) {
    let Some(mut object) = st.doc.scene().get(id).cloned() else {
        return;
    };
    let before = object.clone();
    change(&mut object.content);
    if object != before {
        let held = ui.input(|i| i.pointer.any_down());
        st.doc.edit(object, held);
    }
}

/// Which of the fog's presets its settings are, if any.
fn fog_preset(st: &State, id: ObjectId) -> Option<usize> {
    let Some(Content::Effect {
        effect:
            Effect::Fog {
                color,
                height,
                visibility,
                mist,
                ..
            },
    }) = st.doc.scene().get(id).map(|o| &o.content)
    else {
        return None;
    };
    let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
    let same = |x: Option<f32>, y: Option<f32>| match (x, y) {
        (Some(x), Some(y)) => near(x, y),
        (x, y) => x.is_none() && y.is_none(),
    };
    FOG_PRESETS.iter().position(|p| {
        same(*visibility, p.visibility)
            && same(*mist, p.mist)
            && (p.visibility.is_none() || (*color == p.color && near(*height, p.height)))
    })
}

fn set_fog_preset(ui: &Ui, st: &mut State, id: ObjectId, preset: usize) {
    let Some(p) = FOG_PRESETS.get(preset) else {
        return;
    };
    edit(ui, st, id, |content| {
        if let Content::Effect {
            effect:
                Effect::Fog {
                    color,
                    height,
                    start_distance,
                    visibility,
                    mist,
                },
        } = content
        {
            *color = p.color;
            *height = p.height;
            *visibility = p.visibility;
            *mist = p.mist;
            *start_distance = 0.0;
        }
    });
}

fn shown(st: &State, id: Option<ObjectId>) -> bool {
    id.and_then(|id| st.doc.scene().get(id)).is_some_and(|o| o.visible)
}

/// The palette's width for the Map mode: a name and a control a row.
pub const WIDTH: f32 = 300.0;
/// Where a row's control starts, from the palette's left.
const NAME: f32 = 112.0;
/// The rows the palette shows at most, for its height.
const ROWS: f32 = 8.0;

/// The height of the palette's body, under its header.
pub fn height(k: &Look) -> f32 {
    ROWS * (k.field() + 8.0) + 12.0
}

/// A row's control rectangle at `y`, its name painted.
fn row(ui: &Ui, k: &Look, area: Rect, y: &mut f32, name: &str) -> Rect {
    let h = k.field() + 8.0;
    let line = Rect::from_min_size(Pos2::new(area.left(), *y), Vec2::new(area.width(), h));
    *y += h;
    widgets::label(
        ui.painter(),
        k,
        Pos2::new(line.left() + 12.0, line.center().y),
        name,
        k.t.text,
    );
    Rect::from_min_max(
        Pos2::new(line.left() + NAME, line.top() + 4.0),
        Pos2::new(line.right() - 10.0, line.bottom() - 4.0),
    )
}

/// The Map mode's palette body, in `area` under the palette's header.
pub fn panel(ui: &mut Ui, k: &Look, st: &mut State, area: Rect) {
    let mut y = area.top() + 6.0;
    let Some(map) = map_of(st) else {
        // No map yet: one to add.
        let place = Rect::from_min_size(
            Pos2::new(area.left() + 10.0, y),
            Vec2::new(area.width() - 20.0, k.field()),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(place));
        if flat_button(&mut child, k, Some(icon::PLUS), "Add map", false, false).clicked() {
            st.open_map_library();
        }
        return;
    };
    let Some(Content::Map {
        map: file,
        time,
        hide_walls,
        light_intensity,
        filter_decay,
        ..
    }) = st.doc.scene().get(map).map(|o| o.content.clone())
    else {
        return;
    };
    let name = st.doc.scene().get(map).map(|o| o.name.clone()).unwrap_or_default();
    let times = st
        .map_pictures
        .sidecar(&st.project.resolve(&file))
        .map_or_else(Vec::new, |f| f.times.clone());

    // The map's name, and another map.
    let rect = row(ui, k, area, &mut y, "");
    let line = Rect::from_min_max(
        Pos2::new(area.left(), rect.top() - 4.0),
        Pos2::new(area.right(), rect.bottom() + 4.0),
    );
    widgets::glyph(
        ui.painter(),
        k,
        Pos2::new(line.left() + 18.0, line.center().y),
        icon::MAP_TRIFOLD,
        14.0,
        k.t.text_bright,
    );
    widgets::label(
        ui.painter(),
        k,
        Pos2::new(line.left() + 32.0, line.center().y),
        &name,
        k.t.text_bright,
    );
    let change = Rect::from_min_max(
        Pos2::new(line.right() - 118.0, rect.top()),
        Pos2::new(line.right() - 10.0, rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(change)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    if flat_button(&mut child, k, Some(icon::SWAP), "Change map", false, false).clicked() {
        st.open_map_library();
    }

    let rect = row(ui, k, area, &mut y, "Time of day");
    let options: Vec<&str> = MapTime::ALL.iter().map(|t| t.label()).collect();
    let mut chosen = MapTime::ALL.iter().position(|t| *t == time).unwrap_or(0);
    widgets::segmented(ui, k, rect, ui.id().with("map time"), &mut chosen, &options);
    let picked = MapTime::ALL[chosen];
    if picked != time && times.contains(&picked) {
        edit(ui, st, map, |c| {
            if let Content::Map { time, .. } = c {
                *time = picked;
            }
        });
    }

    let rect = row(ui, k, area, &mut y, "Light intensity");
    let mut value = light_intensity;
    widgets::number(
        ui,
        k,
        rect,
        ui.id().with("map light"),
        &mut value,
        widgets::Marker::None,
        true,
        0.005,
    );
    let value = value.clamp(0.0, 1.0);
    edit(ui, st, map, |c| {
        if let Content::Map { light_intensity, .. } = c {
            *light_intensity = value;
        }
    });

    let rect = row(ui, k, area, &mut y, "Filter decay");
    const STEPS: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
    let names = ["None", "25%", "50%", "75%", "100%"];
    let mut chosen = STEPS
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - filter_decay).abs().total_cmp(&(b.1 - filter_decay).abs()))
        .map_or(2, |(i, _)| i);
    let was = chosen;
    widgets::combo(ui, k, rect, ui.id().with("map decay"), &mut chosen, &names);
    if chosen != was {
        edit(ui, st, map, |c| {
            if let Content::Map { filter_decay, .. } = c {
                *filter_decay = STEPS[chosen];
            }
        });
    }

    if let Some(fog) = effect_of(st, is_fog) {
        let rect = row(ui, k, area, &mut y, "Fog");
        let mut names = vec!["Off"];
        names.extend(FOG_PRESETS.iter().map(|p| p.name));
        names.push("Custom");
        let on = shown(st, Some(fog));
        let mut chosen = if on {
            fog_preset(st, fog).map_or(names.len() - 1, |p| p + 1)
        } else {
            0
        };
        let was = chosen;
        widgets::combo(ui, k, rect, ui.id().with("map fog"), &mut chosen, &names);
        if chosen != was {
            if chosen == 0 {
                st.doc.set_visible(fog, false);
            } else if chosen <= FOG_PRESETS.len() {
                set_fog_preset(ui, st, fog, chosen - 1);
                st.doc.set_visible(fog, true);
            }
        }
    }
    for (name, which) in [
        ("Sun shafts", effect_of(st, is_shafts)),
        ("Colour filter", effect_of(st, is_filter)),
    ] {
        if let Some(id) = which {
            let rect = row(ui, k, area, &mut y, name);
            let mut on = shown(st, Some(id));
            widgets::checkbox(ui, k, rect, ui.id().with(name), &mut on, "");
            st.doc.set_visible(id, on);
        }
    }
    let rect = row(ui, k, area, &mut y, "Hide walls");
    let mut on = hide_walls;
    widgets::checkbox(ui, k, rect, ui.id().with("map walls"), &mut on, "");
    if on != hide_walls {
        edit(ui, st, map, |c| {
            if let Content::Map { hide_walls, .. } = c {
                *hide_walls = on;
            }
        });
    }
}
