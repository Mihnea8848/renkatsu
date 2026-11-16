// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's look as a set of tokens applied to egui's style. Several
//! variants exist side by side so they can be compared in the running app.
//! The values are provisional: docs/editor.md says they are to be measured
//! from Unreal Engine 5.8 and Unity 6, and they will be replaced then.

use egui::{Color32, CornerRadius, Stroke, Style, Vec2, Visuals};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub name: &'static str,
    /// Behind everything: the gaps between docked panels, the title area.
    pub base: Color32,
    /// Panel bodies.
    pub panel: Color32,
    /// Panel headers, the ribbon's tab row, the menu bar.
    pub header: Color32,
    /// Recessed fields, the viewport's surround, list backgrounds.
    pub field: Color32,
    /// Buttons at rest.
    pub control: Color32,
    pub control_hover: Color32,
    /// Dividers between panels and around fields.
    pub divider: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    /// Selection and focus.
    pub select: Color32,
    /// Active and playing states, used sparingly.
    pub active: Color32,
    /// Radius of small controls; panels are always square.
    pub control_radius: u8,
    /// Height of fields, buttons and tree rows.
    pub row: f32,
}

/// The variants offered for comparison, all within Unreal's colours. The
/// first follows Unreal 5's own dark style colours as remembered from its
/// style code (input #0F0F0F, background #151515, panel #242424, dropdown
/// #383838, hover #575757, foreground #C0C0C0, selection #0070E0); they are
/// to be checked against the real editor.
pub const THEMES: [Theme; 4] = [
    Theme {
        name: "Unreal 5",
        base: Color32::from_rgb(0x0f, 0x0f, 0x0f),
        panel: Color32::from_rgb(0x24, 0x24, 0x24),
        header: Color32::from_rgb(0x15, 0x15, 0x15),
        field: Color32::from_rgb(0x0f, 0x0f, 0x0f),
        control: Color32::from_rgb(0x38, 0x38, 0x38),
        control_hover: Color32::from_rgb(0x57, 0x57, 0x57),
        divider: Color32::from_rgb(0x0f, 0x0f, 0x0f),
        text: Color32::from_rgb(0xc0, 0xc0, 0xc0),
        text_dim: Color32::from_rgb(0x84, 0x84, 0x84),
        select: Color32::from_rgb(0x00, 0x70, 0xe0),
        active: Color32::from_rgb(0xf0, 0x8d, 0x00),
        control_radius: 3,
        row: 24.0,
    },
    Theme {
        name: "Unreal dark",
        base: Color32::from_rgb(0x15, 0x15, 0x15),
        panel: Color32::from_rgb(0x24, 0x24, 0x24),
        header: Color32::from_rgb(0x1a, 0x1a, 0x1a),
        field: Color32::from_rgb(0x0f, 0x0f, 0x0f),
        control: Color32::from_rgb(0x38, 0x38, 0x38),
        control_hover: Color32::from_rgb(0x46, 0x46, 0x46),
        divider: Color32::from_rgb(0x0b, 0x0b, 0x0b),
        text: Color32::from_rgb(0xc0, 0xc0, 0xc0),
        text_dim: Color32::from_rgb(0x80, 0x80, 0x80),
        select: Color32::from_rgb(0x00, 0x70, 0xe0),
        active: Color32::from_rgb(0xf0, 0x8d, 0x00),
        control_radius: 2,
        row: 22.0,
    },
    Theme {
        name: "Unreal dense",
        base: Color32::from_rgb(0x11, 0x11, 0x11),
        panel: Color32::from_rgb(0x1e, 0x1e, 0x1e),
        header: Color32::from_rgb(0x16, 0x16, 0x16),
        field: Color32::from_rgb(0x0a, 0x0a, 0x0a),
        control: Color32::from_rgb(0x30, 0x30, 0x30),
        control_hover: Color32::from_rgb(0x3e, 0x3e, 0x3e),
        divider: Color32::from_rgb(0x08, 0x08, 0x08),
        text: Color32::from_rgb(0xbc, 0xbc, 0xbc),
        text_dim: Color32::from_rgb(0x78, 0x78, 0x78),
        select: Color32::from_rgb(0x1a, 0x6f, 0xd6),
        active: Color32::from_rgb(0xe8, 0x86, 0x00),
        control_radius: 1,
        row: 19.0,
    },
    Theme {
        name: "Unreal soft",
        base: Color32::from_rgb(0x1b, 0x1b, 0x1c),
        panel: Color32::from_rgb(0x2a, 0x2a, 0x2c),
        header: Color32::from_rgb(0x21, 0x21, 0x22),
        field: Color32::from_rgb(0x16, 0x16, 0x17),
        control: Color32::from_rgb(0x3c, 0x3c, 0x3f),
        control_hover: Color32::from_rgb(0x4a, 0x4a, 0x4e),
        divider: Color32::from_rgb(0x10, 0x10, 0x11),
        text: Color32::from_rgb(0xcc, 0xcc, 0xcc),
        text_dim: Color32::from_rgb(0x8a, 0x8a, 0x8c),
        select: Color32::from_rgb(0x26, 0x7c, 0xe8),
        active: Color32::from_rgb(0xf2, 0x95, 0x1c),
        control_radius: 2,
        row: 24.0,
    },
];

impl Theme {
    pub fn by_name(name: &str) -> Option<Theme> {
        THEMES.iter().copied().find(|t| t.name.eq_ignore_ascii_case(name))
    }

    /// Writes the tokens into egui's style.
    pub fn apply(&self, style: &mut Style) {
        let r = CornerRadius::same(self.control_radius);
        let mut v = Visuals::dark();
        v.override_text_color = Some(self.text);
        v.panel_fill = self.panel;
        v.window_fill = self.panel;
        v.extreme_bg_color = self.field;
        v.faint_bg_color = self.header;
        v.code_bg_color = self.field;
        v.window_stroke = Stroke::new(1.0, self.divider);
        v.window_corner_radius = CornerRadius::ZERO;
        v.menu_corner_radius = r;
        v.window_shadow = egui::Shadow::NONE;
        v.popup_shadow = egui::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(140),
        };
        v.selection.bg_fill = self.select;
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.hyperlink_color = self.select;
        for (w, fill, text) in [
            (&mut v.widgets.noninteractive, self.panel, self.text_dim),
            (&mut v.widgets.inactive, self.control, self.text),
            (&mut v.widgets.hovered, self.control_hover, Color32::WHITE),
            (&mut v.widgets.active, self.select, Color32::WHITE),
            (&mut v.widgets.open, self.control_hover, Color32::WHITE),
        ] {
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.corner_radius = r;
            w.fg_stroke = Stroke::new(1.0, text);
            w.bg_stroke = Stroke::NONE;
            w.expansion = 0.0;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.divider);
        style.visuals = v;

        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(6.0, 4.0);
        s.button_padding = Vec2::new(8.0, 3.0);
        s.interact_size = Vec2::new(40.0, self.row);
        s.indent = 14.0;
        s.window_margin = egui::Margin::same(6);
        s.menu_margin = egui::Margin::same(4);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_are_found_by_name_and_keep_panels_square() {
        assert_eq!(Theme::by_name("unreal dense").map(|t| t.row), Some(19.0));
        assert!(Theme::by_name("rounded").is_none());
        let mut style = Style::default();
        THEMES[0].apply(&mut style);
        assert_eq!(style.visuals.window_corner_radius, CornerRadius::ZERO);
        assert!(THEMES.iter().all(|t| t.control_radius <= 3));
    }
}
