// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The editor's colours as tokens applied to egui's style. They follow
//! Unreal Engine 5's dark scheme as remembered from its style code (input
//! #0F0F0F, background #151515, panel #242424, header #2F2F2F, dropdown
//! #383838, hover #575757, foreground #C0C0C0, selection #0070E0), refined by
//! eye on screenshots. Sizes and arrangement belong to `design.rs`.

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, Style, TextStyle, Vec2, Visuals};

/// The family of the semibold cut of the interface font.
pub const SEMIBOLD: &str = "semibold";
/// The family of the icon font.
pub const ICONS: &str = "phosphor";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// Recessed fields and the console.
    pub input: Color32,
    /// Behind everything: the menu row, tab wells, the status bar.
    pub well: Color32,
    /// Panel bodies.
    pub panel: Color32,
    /// Menus, dropdown lists and tooltips.
    pub menu: Color32,
    /// Section and column headers inside panels.
    pub header: Color32,
    /// Buttons and dropdowns at rest.
    pub control: Color32,
    /// Buttons and dropdowns under the pointer.
    pub hover: Color32,
    /// A row under the pointer.
    pub row_hover: Color32,
    /// Outlines of fields and overlays.
    pub border: Color32,
    /// Hairlines inside a panel.
    pub line: Color32,
    /// Dividers between panels.
    pub divider: Color32,
    pub text: Color32,
    pub text_bright: Color32,
    pub text_dim: Color32,
    /// Selection and focus.
    pub select: Color32,
    /// A selection that is not the focus: the active tool of a mode.
    pub select_soft: Color32,
    /// The selection colour for icons and text on dark surfaces.
    pub select_bright: Color32,
    /// Warnings and playing states, used sparingly.
    pub accent: Color32,
    pub play: Color32,
    pub stop: Color32,
    /// Markers of the X, Y and Z axes.
    pub axis: [Color32; 3],
    /// Radius of small controls; panels are always square.
    pub radius: u8,
}

pub const THEME: Theme = Theme {
    input: Color32::from_rgb(0x0f, 0x0f, 0x0f),
    well: Color32::from_rgb(0x15, 0x15, 0x15),
    panel: Color32::from_rgb(0x24, 0x24, 0x24),
    menu: Color32::from_rgb(0x1c, 0x1c, 0x1c),
    header: Color32::from_rgb(0x2f, 0x2f, 0x2f),
    control: Color32::from_rgb(0x38, 0x38, 0x38),
    hover: Color32::from_rgb(0x57, 0x57, 0x57),
    row_hover: Color32::from_rgb(0x2e, 0x2e, 0x2e),
    border: Color32::from_rgb(0x3a, 0x3a, 0x3a),
    line: Color32::from_rgb(0x1b, 0x1b, 0x1b),
    divider: Color32::from_rgb(0x0b, 0x0b, 0x0b),
    text: Color32::from_rgb(0xc0, 0xc0, 0xc0),
    text_bright: Color32::from_rgb(0xf2, 0xf2, 0xf2),
    text_dim: Color32::from_rgb(0x86, 0x86, 0x86),
    select: Color32::from_rgb(0x00, 0x70, 0xe0),
    select_soft: Color32::from_rgb(0x1c, 0x3a, 0x5c),
    select_bright: Color32::from_rgb(0x4c, 0xa2, 0xff),
    accent: Color32::from_rgb(0xf0, 0x8d, 0x00),
    play: Color32::from_rgb(0x5f, 0xc2, 0x4a),
    stop: Color32::from_rgb(0xdc, 0x48, 0x40),
    axis: [
        Color32::from_rgb(0xd6, 0x4a, 0x4a),
        Color32::from_rgb(0x6c, 0xb2, 0x3e),
        Color32::from_rgb(0x3d, 0x8a, 0xea),
    ],
    radius: 3,
};

impl Theme {
    /// A colour made see-through, for overlays on the viewport.
    pub fn glass(color: Color32, alpha: u8) -> Color32 {
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
    }

    /// Writes the tokens into egui's style, for the widgets egui draws
    /// itself: menus, popups, tooltips, text fields and scroll bars.
    pub fn apply(&self, style: &mut Style, text: f32) {
        let r = CornerRadius::same(self.radius);
        let mut v = Visuals::dark();
        v.override_text_color = Some(self.text);
        v.panel_fill = self.panel;
        v.window_fill = self.menu;
        v.extreme_bg_color = self.input;
        v.faint_bg_color = self.well;
        v.code_bg_color = self.input;
        v.window_stroke = Stroke::new(1.0, self.border);
        v.window_corner_radius = CornerRadius::ZERO;
        v.menu_corner_radius = CornerRadius::same(2);
        v.window_shadow = egui::Shadow::NONE;
        v.popup_shadow = egui::Shadow {
            offset: [0, 3],
            blur: 10,
            spread: 0,
            color: Color32::from_black_alpha(120),
        };
        v.selection.bg_fill = self.select;
        v.selection.stroke = Stroke::new(1.0, self.select);
        v.hyperlink_color = self.select;
        v.text_cursor.stroke = Stroke::new(1.0, self.text_bright);
        for (w, fill, text) in [
            (&mut v.widgets.noninteractive, self.panel, self.text_dim),
            (&mut v.widgets.inactive, self.control, self.text),
            (&mut v.widgets.hovered, self.control, self.text_bright),
            (&mut v.widgets.active, self.select, self.text_bright),
            (&mut v.widgets.open, self.hover, self.text_bright),
        ] {
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.corner_radius = r;
            w.fg_stroke = Stroke::new(1.0, text);
            w.bg_stroke = Stroke::NONE;
            w.expansion = 0.0;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        style.visuals = v;

        style.text_styles = [
            (TextStyle::Small, FontId::new(text - 2.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(text, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(text, FontFamily::Proportional)),
            (
                TextStyle::Heading,
                FontId::new(text + 2.0, FontFamily::Name(SEMIBOLD.into())),
            ),
            (TextStyle::Monospace, FontId::new(text - 1.0, FontFamily::Monospace)),
        ]
        .into();

        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(6.0, 4.0);
        s.button_padding = Vec2::new(8.0, 4.0);
        s.interact_size = Vec2::new(40.0, 22.0);
        s.indent = 14.0;
        s.window_margin = egui::Margin::same(6);
        s.menu_margin = egui::Margin::same(5);
        s.scroll = egui::style::ScrollStyle::floating();
        s.scroll.bar_width = 8.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::Design;

    #[test]
    fn panels_stay_square_and_text_stays_readable() {
        let mut style = Style::default();
        THEME.apply(&mut style, 13.0);
        assert_eq!(style.visuals.window_corner_radius, CornerRadius::ZERO);
        const { assert!(THEME.radius <= 4) };
        assert!(style.visuals.menu_corner_radius.nw <= 4);
        for design in Design::ALL {
            let spec = design.spec();
            assert!((12.0..=13.0).contains(&spec.text), "{} text size", spec.name);
            assert!(spec.row >= 20.0, "{} row height", spec.name);
        }
    }
}
