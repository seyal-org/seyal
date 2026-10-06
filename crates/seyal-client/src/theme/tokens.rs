//! Portable product tokens. Hosts map `Srgb` into NSColor/Metal; this crate
//! never calls AppKit.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Srgb {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

impl Srgb {
    pub fn from_u8(red: f64, green: f64, blue: f64) -> Self {
        Self::from_u8_alpha(red, green, blue, 1.0)
    }

    pub fn from_u8_alpha(red: f64, green: f64, blue: f64, alpha: f64) -> Self {
        Self {
            red: red / 255.0,
            green: green / 255.0,
            blue: blue / 255.0,
            alpha,
        }
    }

    pub fn luminance(self) -> f64 {
        0.2126 * self.red + 0.7152 * self.green + 0.0722 * self.blue
    }

    pub fn with_alpha(self, alpha: f64) -> Self {
        Self {
            alpha: alpha.clamp(0.0, 1.0),
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorRole {
    Canvas,
    Container,
    UtilityReceded,
    UtilityActive,
    UtilityElevated,
    Overlay,
    AttentionFill,
    TextPrimary,
    TextSecondary,
    TextMuted,
    TextAttention,
    SeamRest,
    SeamHover,
    SeamFocus,
    SeamRunning,
    SeamAttention,
    Focus,
    Selection,
    Success,
    Warning,
    Danger,
    Information,
    AgentActivity,
    RemoteDegraded,
    /// Focused/selected Block border (Seyal Block Component, #1010).
    BlockFocus,
}

impl ColorRole {
    pub const ALL: [Self; 25] = [
        Self::Canvas,
        Self::Container,
        Self::UtilityReceded,
        Self::UtilityActive,
        Self::UtilityElevated,
        Self::Overlay,
        Self::AttentionFill,
        Self::TextPrimary,
        Self::TextSecondary,
        Self::TextMuted,
        Self::TextAttention,
        Self::SeamRest,
        Self::SeamHover,
        Self::SeamFocus,
        Self::SeamRunning,
        Self::SeamAttention,
        Self::Focus,
        Self::Selection,
        Self::Success,
        Self::Warning,
        Self::Danger,
        Self::Information,
        Self::AgentActivity,
        Self::RemoteDegraded,
        Self::BlockFocus,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedAppearance {
    Light,
    Dark,
}

pub fn palette_color(
    role: ColorRole,
    appearance: ResolvedAppearance,
    increase_contrast: bool,
) -> Srgb {
    let base = match appearance {
        ResolvedAppearance::Dark => dark(role),
        ResolvedAppearance::Light => light(role),
    };
    if increase_contrast {
        contrast_adjusted(base, appearance, role)
    } else {
        base
    }
}

fn dark(role: ColorRole) -> Srgb {
    match role {
        ColorRole::Canvas => Srgb::from_u8(10.0, 14.0, 20.0),
        ColorRole::Container => Srgb::from_u8(12.0, 15.0, 20.0),
        ColorRole::UtilityReceded => Srgb::from_u8(16.0, 21.0, 29.0),
        ColorRole::UtilityActive => Srgb::from_u8(20.0, 26.0, 36.0),
        ColorRole::UtilityElevated => Srgb::from_u8(24.0, 30.0, 42.0),
        ColorRole::Overlay => Srgb::from_u8(22.0, 28.0, 40.0),
        ColorRole::AttentionFill => Srgb::from_u8(42.0, 28.0, 24.0),
        ColorRole::TextPrimary => Srgb::from_u8(231.0, 234.0, 240.0),
        ColorRole::TextSecondary => Srgb::from_u8(157.0, 166.0, 184.0),
        ColorRole::TextMuted => Srgb::from_u8(100.0, 112.0, 132.0),
        ColorRole::TextAttention => Srgb::from_u8(249.0, 180.0, 140.0),
        ColorRole::SeamRest => Srgb::from_u8(26.0, 32.0, 42.0),
        ColorRole::SeamHover => Srgb::from_u8(34.0, 40.0, 52.0),
        ColorRole::SeamFocus => Srgb::from_u8(132.0, 100.0, 232.0),
        ColorRole::SeamRunning => Srgb::from_u8(245.0, 165.0, 36.0),
        ColorRole::SeamAttention => Srgb::from_u8(249.0, 112.0, 102.0),
        ColorRole::Focus => Srgb::from_u8(132.0, 100.0, 232.0),
        ColorRole::Selection => Srgb::from_u8(33.0, 28.0, 57.0),
        ColorRole::Success => Srgb::from_u8(56.0, 211.0, 159.0),
        ColorRole::Warning => Srgb::from_u8(245.0, 165.0, 36.0),
        ColorRole::Danger => Srgb::from_u8(249.0, 112.0, 102.0),
        ColorRole::Information => Srgb::from_u8(94.0, 160.0, 255.0),
        ColorRole::AgentActivity => Srgb::from_u8(132.0, 100.0, 232.0),
        ColorRole::RemoteDegraded => Srgb::from_u8(245.0, 165.0, 36.0),
        ColorRole::BlockFocus => Srgb::from_u8(59.0, 130.0, 246.0),
    }
}

fn light(role: ColorRole) -> Srgb {
    match role {
        ColorRole::Canvas => Srgb::from_u8(252.0, 252.0, 250.0),
        ColorRole::Container => Srgb::from_u8(246.0, 246.0, 244.0),
        ColorRole::UtilityReceded => Srgb::from_u8(238.0, 239.0, 236.0),
        ColorRole::UtilityActive => Srgb::from_u8(232.0, 234.0, 230.0),
        ColorRole::UtilityElevated => Srgb::from_u8(255.0, 255.0, 255.0),
        ColorRole::Overlay => Srgb::from_u8(255.0, 255.0, 255.0),
        ColorRole::AttentionFill => Srgb::from_u8(255.0, 244.0, 238.0),
        ColorRole::TextPrimary => Srgb::from_u8(28.0, 32.0, 38.0),
        ColorRole::TextSecondary => Srgb::from_u8(90.0, 98.0, 110.0),
        ColorRole::TextMuted => Srgb::from_u8(130.0, 138.0, 148.0),
        ColorRole::TextAttention => Srgb::from_u8(160.0, 70.0, 40.0),
        ColorRole::SeamRest => Srgb::from_u8(220.0, 222.0, 218.0),
        ColorRole::SeamHover => Srgb::from_u8(196.0, 200.0, 194.0),
        ColorRole::SeamFocus => Srgb::from_u8(92.0, 70.0, 180.0),
        ColorRole::SeamRunning => Srgb::from_u8(180.0, 110.0, 12.0),
        ColorRole::SeamAttention => Srgb::from_u8(196.0, 64.0, 54.0),
        ColorRole::Focus => Srgb::from_u8(92.0, 70.0, 180.0),
        ColorRole::Selection => Srgb::from_u8(232.0, 226.0, 250.0),
        ColorRole::Success => Srgb::from_u8(20.0, 140.0, 100.0),
        ColorRole::Warning => Srgb::from_u8(180.0, 110.0, 12.0),
        ColorRole::Danger => Srgb::from_u8(196.0, 64.0, 54.0),
        ColorRole::Information => Srgb::from_u8(40.0, 110.0, 190.0),
        ColorRole::AgentActivity => Srgb::from_u8(92.0, 70.0, 180.0),
        ColorRole::RemoteDegraded => Srgb::from_u8(180.0, 110.0, 12.0),
        ColorRole::BlockFocus => Srgb::from_u8(37.0, 99.0, 235.0),
    }
}

fn contrast_adjusted(color: Srgb, appearance: ResolvedAppearance, role: ColorRole) -> Srgb {
    match role {
        ColorRole::TextPrimary
        | ColorRole::TextSecondary
        | ColorRole::TextMuted
        | ColorRole::TextAttention => {
            if appearance == ResolvedAppearance::Dark {
                Srgb {
                    red: (color.red + 0.08).min(1.0),
                    green: (color.green + 0.08).min(1.0),
                    blue: (color.blue + 0.08).min(1.0),
                    alpha: color.alpha,
                }
            } else {
                Srgb {
                    red: (color.red - 0.08).max(0.0),
                    green: (color.green - 0.08).max(0.0),
                    blue: (color.blue - 0.08).max(0.0),
                    alpha: color.alpha,
                }
            }
        }
        ColorRole::SeamRest | ColorRole::SeamHover => {
            if appearance == ResolvedAppearance::Dark {
                Srgb {
                    red: (color.red + 0.12).min(1.0),
                    green: (color.green + 0.12).min(1.0),
                    blue: (color.blue + 0.12).min(1.0),
                    alpha: 1.0,
                }
            } else {
                Srgb {
                    red: (color.red - 0.12).max(0.0),
                    green: (color.green - 0.12).max(0.0),
                    blue: (color.blue - 0.12).max(0.0),
                    alpha: 1.0,
                }
            }
        }
        _ => color,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub base: f64,
    pub xs: f64,
    pub sm: f64,
    pub md: f64,
    pub lg: f64,
    pub window_padding: f64,
    pub content_padding_horizontal: f64,
    pub content_padding_vertical: f64,
    pub sidebar_padding: f64,
    pub inspector_padding: f64,
    pub tab_spacing: f64,
    pub block_seam_spacing: f64,
    pub composer_inset_horizontal: f64,
    pub composer_inset_vertical: f64,
    pub control_spacing: f64,
    pub pane_separator_thickness: f64,
    pub utility_rail_width: f64,
    pub left_context_width: f64,
    pub left_context_min_width: f64,
    pub inspector_width: f64,
    pub inspector_min_width: f64,
    pub inspector_rail_width: f64,
    pub top_chrome_height: f64,
    pub composer_min_height: f64,
    pub composer_max_height: f64,
    pub min_interactive_size: f64,
    pub tab_min_width: f64,
    pub tab_max_width: f64,
    pub block_corner_radius: f64,
    pub pane_corner_radius: f64,
    pub composer_corner_radius: f64,
    pub overlay_corner_radius: f64,
    pub seam_width: f64,
    pub terminal_padding: f64,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            base: 4.0,
            xs: 4.0,
            sm: 8.0,
            md: 12.0,
            lg: 16.0,
            window_padding: 0.0,
            content_padding_horizontal: 12.0,
            content_padding_vertical: 10.0,
            sidebar_padding: 10.0,
            inspector_padding: 10.0,
            tab_spacing: 8.0,
            block_seam_spacing: 8.0,
            composer_inset_horizontal: 12.0,
            composer_inset_vertical: 8.0,
            control_spacing: 8.0,
            pane_separator_thickness: 1.0,
            utility_rail_width: 36.0,
            left_context_width: 220.0,
            left_context_min_width: 180.0,
            inspector_width: 248.0,
            inspector_min_width: 200.0,
            inspector_rail_width: 36.0,
            top_chrome_height: 48.0,
            composer_min_height: 52.0,
            composer_max_height: 116.0,
            min_interactive_size: 28.0,
            tab_min_width: 118.0,
            tab_max_width: 190.0,
            block_corner_radius: 0.0,
            pane_corner_radius: 0.0,
            composer_corner_radius: 6.0,
            overlay_corner_radius: 8.0,
            seam_width: 1.0,
            terminal_padding: 8.0,
        }
    }
}

impl Metrics {
    pub fn with_user_padding(self, window: f64, terminal: f64) -> Self {
        Self {
            window_padding: window,
            terminal_padding: terminal,
            ..self
        }
    }

    pub fn validate(self) -> bool {
        self.left_context_width >= self.left_context_min_width
            && self.inspector_width >= self.inspector_min_width
            && self.composer_min_height <= self.composer_max_height
            && self.min_interactive_size >= 20.0
            && self.seam_width > 0.0
            && self.seam_width <= 2.0
            && self.block_corner_radius == 0.0
            && self.pane_corner_radius == 0.0
            && self.base == self.xs
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionSettings {
    pub allows_motion: bool,
    pub focus_duration: f64,
    pub overlay_duration: f64,
}

impl MotionSettings {
    pub fn canonical(reduced_motion: bool) -> Self {
        Self {
            allows_motion: !reduced_motion,
            focus_duration: if reduced_motion { 0.0 } else { 0.12 },
            overlay_duration: if reduced_motion { 0.0 } else { 0.16 },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AccessibilitySignals {
    pub reduce_transparency: bool,
    pub reduce_motion: bool,
    pub increase_contrast: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DepthLevel {
    Truth,
    RecededUtility,
    ActiveUtility,
    Attention,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialIntent {
    Opaque,
    Tonal,
    Frosted,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedMaterial {
    pub depth: DepthLevel,
    pub intent: MaterialIntent,
    pub color: Srgb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeamRole {
    Rest,
    Hover,
    Focus,
    Running,
    Attention,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypographyRole {
    WindowTitle,
    SectionLabel,
    UiBody,
    UiSecondary,
    Metadata,
    Tab,
    SidebarRow,
    InspectorHeading,
    Action,
    Composer,
    Terminal,
}

impl TypographyRole {
    pub const ALL: [Self; 11] = [
        Self::WindowTitle,
        Self::SectionLabel,
        Self::UiBody,
        Self::UiSecondary,
        Self::Metadata,
        Self::Tab,
        Self::SidebarRow,
        Self::InspectorHeading,
        Self::Action,
        Self::Composer,
        Self::Terminal,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontWeight {
    Regular,
    Medium,
    Semibold,
    Bold,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FontSpec {
    pub family: String,
    pub fallbacks: Vec<String>,
    pub size: f64,
    pub weight: FontWeight,
    pub line_height: f64,
    pub tracking: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedFontSpec {
    pub family: String,
    pub fallbacks: Vec<String>,
    pub point_size: f64,
}

pub fn typography_specs(
    ui: &ResolvedFontSpec,
    terminal: &ResolvedFontSpec,
) -> Vec<(TypographyRole, FontSpec)> {
    let body = ui.point_size;
    let small = (body - 2.0).max(9.0);
    let ui_spec = |size: f64, weight: FontWeight, line_height: f64, tracking: f64| FontSpec {
        family: ui.family.clone(),
        fallbacks: ui.fallbacks.clone(),
        size,
        weight,
        line_height,
        tracking,
    };
    vec![
        (
            TypographyRole::WindowTitle,
            ui_spec(body, FontWeight::Semibold, body + 4.0, 0.0),
        ),
        (
            TypographyRole::SectionLabel,
            ui_spec(small, FontWeight::Semibold, small + 3.0, 0.4),
        ),
        (
            TypographyRole::UiBody,
            ui_spec(body, FontWeight::Regular, body + 4.0, 0.0),
        ),
        (
            TypographyRole::UiSecondary,
            ui_spec(body, FontWeight::Regular, body + 4.0, 0.0),
        ),
        (
            TypographyRole::Metadata,
            ui_spec(small, FontWeight::Regular, small + 3.0, 0.0),
        ),
        (
            TypographyRole::Tab,
            ui_spec(body, FontWeight::Medium, body + 4.0, 0.0),
        ),
        (
            TypographyRole::SidebarRow,
            ui_spec(body, FontWeight::Regular, body + 4.0, 0.0),
        ),
        (
            TypographyRole::InspectorHeading,
            ui_spec(small, FontWeight::Semibold, small + 3.0, 0.3),
        ),
        (
            TypographyRole::Action,
            ui_spec(body, FontWeight::Medium, body + 4.0, 0.0),
        ),
        (
            TypographyRole::Composer,
            FontSpec {
                family: terminal.family.clone(),
                fallbacks: terminal.fallbacks.clone(),
                size: body,
                weight: FontWeight::Semibold,
                line_height: body + 6.0,
                tracking: 0.0,
            },
        ),
        (
            TypographyRole::Terminal,
            FontSpec {
                family: terminal.family.clone(),
                fallbacks: terminal.fallbacks.clone(),
                size: terminal.point_size,
                weight: FontWeight::Regular,
                line_height: terminal.point_size + 5.0,
                tracking: 0.0,
            },
        ),
    ]
}

pub const LUA_ACCEPTED_INPUT: &str = "SeyalConfigPatch";
pub const LUA_RUNTIME_STATUS: &str = "deferred; no Lua VM in this milestone";
pub const LUA_FORBIDDEN_DOMAINS: [&str; 8] = [
    "keystrokes",
    "PTY input/output",
    "VT parsing",
    "terminal-grid updates",
    "damage tracking",
    "Metal rendering",
    "per-frame presentation",
    "direct NSView mutation",
];
