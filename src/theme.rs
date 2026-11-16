use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Theme, Visuals,
};
use gang_branch_2000::labels::Tone;

pub struct Palette {
    pub base: Color32,
    pub lane: Color32,
    pub raised: Color32,
    pub hairline: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub link: Color32,
    pub dirty: Color32,
    pub sage: Color32,
    pub amber: Color32,
    pub cyan: Color32,
    pub violet: Color32,
    pub red: Color32,
    on_amber: Color32,
    on_cyan: Color32,
    on_violet: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub const DARK: Palette = Palette {
    base: hex(0x171A1F),
    lane: hex(0x1C1F25),
    raised: hex(0x252A32),
    hairline: hex(0x2C313A),
    text: hex(0xE6E8EC),
    muted: hex(0x8C94A3),
    link: hex(0x7FA8E8),
    dirty: hex(0xC3A6F0),
    sage: hex(0x5E9C6E),
    amber: hex(0xE3A33B),
    cyan: hex(0x4DB3C4),
    violet: hex(0xA98BEA),
    red: hex(0xE06C6C),
    on_amber: hex(0x2A1C05),
    on_cyan: hex(0x06262B),
    on_violet: hex(0x1C1230),
};

pub const LIGHT: Palette = Palette {
    base: hex(0xF3F4F6),
    lane: hex(0xFFFFFF),
    raised: hex(0xFFFFFF),
    hairline: hex(0xE1E4E8),
    text: hex(0x1C2027),
    muted: hex(0x5E6673),
    link: hex(0x2F62B8),
    dirty: hex(0x7A5BD6),
    sage: hex(0x3E8E52),
    amber: hex(0xB7791F),
    cyan: hex(0x1D8DA3),
    violet: hex(0x7A5BD6),
    red: hex(0xC24040),
    on_amber: Color32::WHITE,
    on_cyan: Color32::WHITE,
    on_violet: Color32::WHITE,
};

impl Palette {
    pub fn tone(&self, tone: Tone) -> Color32 {
        match tone {
            Tone::Clean => self.sage,
            Tone::Behind => self.amber,
            Tone::Ahead => self.cyan,
            Tone::Unpublished => self.violet,
            Tone::Detached => self.muted,
            Tone::Error => self.red,
        }
    }

    /// Text colour on a button filled with the tone, or None when that tone gets no fill.
    pub fn on_tone(&self, tone: Tone) -> Option<Color32> {
        match tone {
            Tone::Behind => Some(self.on_amber),
            Tone::Ahead => Some(self.on_cyan),
            Tone::Unpublished => Some(self.on_violet),
            _ => None,
        }
    }

    pub fn error(&self) -> Color32 {
        self.red
    }
}

pub fn palette(ctx: &egui::Context) -> &'static Palette {
    match ctx.theme() {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
    }
}

pub fn medium() -> FontFamily {
    FontFamily::Name("medium".into())
}

pub fn branch() -> FontFamily {
    FontFamily::Name("branch".into())
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        (
            "fira",
            &include_bytes!("../assets/fonts/FiraSans-Book.ttf")[..],
        ),
        (
            "fira-medium",
            include_bytes!("../assets/fonts/FiraSans-Medium.ttf"),
        ),
        (
            "fira-condensed",
            include_bytes!("../assets/fonts/FiraSansCondensed-Medium.ttf"),
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    // egui's own fonts stay behind Fira for the glyphs it lacks.
    let fallbacks = fonts.families[&FontFamily::Proportional].clone();
    let with_fallbacks = |first: &str| {
        std::iter::once(first.to_owned())
            .chain(fallbacks.iter().cloned())
            .collect::<Vec<_>>()
    };
    fonts
        .families
        .insert(FontFamily::Proportional, with_fallbacks("fira"));
    fonts
        .families
        .insert(medium(), with_fallbacks("fira-medium"));
    fonts
        .families
        .insert(branch(), with_fallbacks("fira-condensed"));
    ctx.set_fonts(fonts);

    for (theme, palette) in [(Theme::Dark, &DARK), (Theme::Light, &LIGHT)] {
        ctx.style_mut_of(theme, |style| {
            style.text_styles = [
                (
                    TextStyle::Small,
                    FontId::new(12.0, FontFamily::Proportional),
                ),
                (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
                (TextStyle::Button, FontId::new(14.0, medium())),
                (TextStyle::Heading, FontId::new(20.0, branch())),
                (
                    TextStyle::Monospace,
                    FontId::new(13.0, FontFamily::Monospace),
                ),
            ]
            .into();
            style.spacing.item_spacing = egui::vec2(8.0, 4.0);
            style.spacing.button_padding = egui::vec2(12.0, 5.0);
            style.spacing.interact_size.y = 28.0;
            style.visuals = visuals(theme, palette);
        });
    }
}

fn visuals(theme: Theme, p: &Palette) -> Visuals {
    let mut v = match theme {
        Theme::Dark => Visuals::dark(),
        Theme::Light => Visuals::light(),
    };
    let radius = CornerRadius::same(6);
    v.panel_fill = p.base;
    v.window_fill = p.raised;
    v.window_stroke = Stroke::new(1.0, p.hairline);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(8);
    v.extreme_bg_color = p.base;
    v.text_edit_bg_color = Some(p.base);
    v.faint_bg_color = p.lane;
    v.hyperlink_color = p.link;
    v.error_fg_color = p.red;
    v.warn_fg_color = p.amber;
    v.weak_text_color = Some(p.muted);
    v.selection.bg_fill = p.link.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, p.link);

    let w = &mut v.widgets;
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.hairline);
    for state in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        state.corner_radius = radius;
        state.fg_stroke = Stroke::new(1.0, p.text);
        state.expansion = 0.0;
    }
    w.inactive.weak_bg_fill = p.lane;
    w.inactive.bg_fill = p.lane;
    w.inactive.bg_stroke = Stroke::new(1.0, p.hairline);
    w.hovered.weak_bg_fill = p.raised;
    w.hovered.bg_fill = p.raised;
    w.hovered.bg_stroke = Stroke::new(1.0, p.muted);
    w.active.weak_bg_fill = p.hairline;
    w.active.bg_fill = p.hairline;
    w.active.bg_stroke = Stroke::new(1.0, p.muted);
    w.open.weak_bg_fill = p.raised;
    w.open.bg_stroke = Stroke::new(1.0, p.hairline);
    v
}
