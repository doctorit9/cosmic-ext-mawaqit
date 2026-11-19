// SPDX-License-Identifier: MIT

// Exact float comparison is intended in this module: `watch_config` requires
// `PartialEq` for change detection, where bit-exact equality is correct.
#![allow(clippy::float_cmp)]

use crate::prayer::{
    DEFAULT_LATITUDE, DEFAULT_LONGITUDE, DEFAULT_METHOD_ID, DEFAULT_TIMEZONE, PrayerKind,
};
use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

/// Font family choices for the applet and panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FontFamilyChoice {
    #[default]
    SystemDefault,
    NotoSans,
    NotoKufiArabic,
    NotoNaskhArabic,
    Cairo,
    Amiri,
    DejaVuSans,
    Monospace,
    Custom,
}

impl FontFamilyChoice {
    pub const ALL: [Self; 9] = [
        Self::SystemDefault,
        Self::NotoSans,
        Self::NotoKufiArabic,
        Self::NotoNaskhArabic,
        Self::Cairo,
        Self::Amiri,
        Self::DejaVuSans,
        Self::Monospace,
        Self::Custom,
    ];

    #[must_use]
    pub fn family_name(self, custom: &str) -> &'static str {
        match self {
            Self::SystemDefault => "",
            Self::NotoSans => "Noto Sans",
            Self::NotoKufiArabic => "Noto Kufi Arabic",
            Self::NotoNaskhArabic => "Noto Naskh Arabic",
            Self::Cairo => "Cairo",
            Self::Amiri => "Amiri",
            Self::DejaVuSans => "DejaVu Sans",
            Self::Monospace => "monospace",
            Self::Custom => {
                let trimmed = custom.trim();
                if trimmed.is_empty() {
                    ""
                } else {
                    intern_font_name(trimmed)
                }
            }
        }
    }

    #[must_use]
    pub fn display_name(self, custom: &str) -> String {
        match self {
            Self::SystemDefault => "System Default".to_string(),
            Self::NotoSans => "Noto Sans".to_string(),
            Self::NotoKufiArabic => "Noto Kufi Arabic".to_string(),
            Self::NotoNaskhArabic => "Noto Naskh Arabic".to_string(),
            Self::Cairo => "Cairo".to_string(),
            Self::Amiri => "Amiri".to_string(),
            Self::DejaVuSans => "DejaVu Sans".to_string(),
            Self::Monospace => "Monospace".to_string(),
            Self::Custom => {
                let trimmed = custom.trim();
                if trimmed.is_empty() {
                    "Custom".to_string()
                } else {
                    format!("Custom ({trimmed})")
                }
            }
        }
    }
}

/// Maximum number of custom font names that may be interned into static memory.
/// Prevents unbounded memory leaks if custom names are typed repeatedly in the GUI.
const MAX_INTERNED_FONTS: usize = 32;

static FONT_POOL: LazyLock<Mutex<HashSet<&'static str>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[cfg(test)]
pub(crate) fn interned_font_count() -> usize {
    let pool = match FONT_POOL.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    pool.len()
}

fn intern_font_name(name: &str) -> &'static str {
    // Sanitize input: strip control characters and cap at 64 characters
    let sanitized: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect();
    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        return "";
    }

    let mut pool = match FONT_POOL.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    if let Some(&existing) = pool.get(trimmed) {
        return existing;
    }

    // Memory cap: reuse existing entry if full
    if pool.len() >= MAX_INTERNED_FONTS {
        return pool.iter().next().copied().unwrap_or("");
    }

    let leaked: &'static str = Box::leak(trimmed.to_string().into_boxed_str());
    pool.insert(leaked);
    leaked
}

/// Panel button arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum PanelLayout {
    /// Automatic: adapts dynamically to panel orientation (horizontal in top/bottom bar, stacked in side bar).
    #[default]
    Auto,
    /// Name + countdown side by side, e.g. "Asr 42m".
    Horizontal,
    /// Name over countdown on two lines (vertical, for narrow panels).
    Stacked,
}

impl PanelLayout {
    /// Whether this layout results in stacked (vertical) text given the panel orientation.
    pub fn is_stacked_in(self, is_panel_horizontal: bool) -> bool {
        match self {
            Self::Auto => !is_panel_horizontal,
            Self::Horizontal => false,
            Self::Stacked => true,
        }
    }

    /// Whether the countdown goes on its own line below the name.
    #[allow(dead_code)]
    pub const fn is_stacked(self) -> bool {
        matches!(self, Self::Stacked)
    }
}

/// Per-prayer time adjustments in minutes (positive = later, negative = earlier).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct PrayerAdjustments {
    pub fajr: i8,
    pub sunrise: i8,
    pub dhuhr: i8,
    pub asr: i8,
    pub maghrib: i8,
    pub isha: i8,
}

/// Custom calculation angles for prayer times.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct CustomAngles {
    /// Fajr angle in degrees (default varies by method).
    pub fajr_angle: Option<f64>,
    /// Isha angle in degrees (default varies by method).
    pub isha_angle: Option<f64>,
    /// Isha interval in minutes after Maghrib (0-255, default varies by method).
    pub isha_interval: Option<u8>,
}

#[derive(Debug, Clone, CosmicConfigEntry, PartialEq)]
#[version = 4]
pub struct Config {
    /// Calculation method id (AlAdhan-compatible): 3=MWL, 2=ISNA, 5=Egypt, 4=UmmAlQura,
    /// 1=Karachi, 0=Jafari, etc. Default 5 (Egyptian, matches omaprayers default).
    pub method_id: u8,
    /// true = Hanafi Asr, false = Shafi/Standard Asr.
    pub hanafi: bool,
    /// Location label shown in popup.
    pub location_label: String,
    pub latitude: f64,
    pub longitude: f64,
    /// IANA timezone, e.g. "Africa/Cairo".
    pub timezone: String,
    /// 12h vs 24h display.
    pub use_24h: bool,
    /// Horizontal vs stacked (vertical) panel label.
    pub panel_layout: PanelLayout,
    /// Panel shows the next prayer's full clock time instead of a countdown.
    pub show_prayer_time: bool,
    /// Per-prayer time adjustments in minutes.
    pub prayer_adjustments: PrayerAdjustments,
    /// Custom calculation angles (overrides method defaults when set).
    pub custom_angles: CustomAngles,
    /// Whether to use custom angles instead of method defaults.
    pub use_custom_angles: bool,
    /// Notification settings for each prayer.
    pub notification_settings: NotificationSettings,
    /// Whether compact mode is enabled for the popup window.
    pub compact_mode: bool,
    /// Whether to display the 6-milestone celestial horizon track on the main page.
    pub show_celestial_horizon: bool,
    /// Whether to display the upcoming/active prayer hero card on the main page.
    pub show_hero_card: bool,
    /// Whether to show the Hijri calendar date in the header.
    pub show_hijri_date: bool,
    /// Selected font family preset.
    pub font_family: FontFamilyChoice,
    /// Custom font family name if `font_family == FontFamilyChoice::Custom`.
    pub custom_font_family: String,
    /// Panel text font size in points (0 = auto responsive).
    pub panel_font_size: u8,
    /// Popup UI font scale percentage (default 100%).
    pub popup_font_scale: u8,
}

impl Config {
    #[must_use]
    pub fn font(&self) -> cosmic::iced::Font {
        self.font_with_weight(cosmic::iced::font::Weight::Normal)
    }

    #[must_use]
    pub fn font_bold(&self) -> cosmic::iced::Font {
        self.font_with_weight(cosmic::iced::font::Weight::Bold)
    }

    #[must_use]
    pub fn font_with_weight(&self, weight: cosmic::iced::font::Weight) -> cosmic::iced::Font {
        let name = self.font_family.family_name(&self.custom_font_family);
        let family = if name.is_empty() {
            cosmic::iced::font::Family::SansSerif
        } else if name == "monospace" {
            cosmic::iced::font::Family::Monospace
        } else {
            cosmic::iced::font::Family::Name(name)
        };
        cosmic::iced::Font {
            family,
            weight,
            stretch: cosmic::iced::font::Stretch::Normal,
            style: cosmic::iced::font::Style::Normal,
        }
    }
}

const fn default_true() -> bool {
    true
}

const fn default_pre_reminder_minutes() -> u8 {
    10
}

const fn default_volume() -> u8 {
    70
}

/// Sound preset / reciter for Adhan notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum AdhanVoice {
    #[default]
    Makkah,
    Madinah,
    AlAqsa,
    Alafasy,
    AbdulBasit,
    MuhammadRefaat,
    MustafaIsmail,
    SystemBell,
    CustomFile,
}

impl AdhanVoice {
    pub const ALL: [Self; 9] = [
        Self::Makkah,
        Self::Madinah,
        Self::AlAqsa,
        Self::Alafasy,
        Self::AbdulBasit,
        Self::MuhammadRefaat,
        Self::MustafaIsmail,
        Self::SystemBell,
        Self::CustomFile,
    ];

    #[must_use]
    #[allow(dead_code)]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Makkah => "makkah",
            Self::Madinah => "madinah",
            Self::AlAqsa => "alaqsa",
            Self::Alafasy => "alafasy",
            Self::AbdulBasit => "abdulbasit",
            Self::MuhammadRefaat => "refaat",
            Self::MustafaIsmail => "ismail",
            Self::SystemBell => "bell",
            Self::CustomFile => "custom",
        }
    }

    #[must_use]
    pub const fn filename(self) -> Option<&'static str> {
        match self {
            Self::Makkah => Some("makkah.mp3"),
            Self::Madinah => Some("madinah.mp3"),
            Self::AlAqsa => Some("alaqsa.mp3"),
            Self::Alafasy => Some("alafasy.mp3"),
            Self::AbdulBasit => Some("abdulbasit.mp3"),
            Self::MuhammadRefaat => Some("refaat.mp3"),
            Self::MustafaIsmail => Some("ismail.mp3"),
            Self::SystemBell | Self::CustomFile => None,
        }
    }

    #[must_use]
    pub const fn cdn_url(self) -> Option<&'static str> {
        match self {
            Self::Makkah => Some("https://cdn.jsdelivr.net/gh/Kiwifu/adhan-mp3@b9180a2bb769f74cff8e378ef2cc7aaf8db5cd5b/Ali_Ibn_Ahmad_Mala_1_-_Al_Haram_Al_Maki_(%D8%B9%D9%84%D9%8A_%D8%A8%D9%86_%D8%A3%D8%AD%D9%85%D8%AF_%D9%85%D9%84%D8%A7_-_%D8%A7%D9%84%D8%AD%D8%B1%D9%85_%D8%A7%D9%84%D9%85%D9%83%D9%8A).mp3"),
            Self::Madinah => Some("https://cdn.jsdelivr.net/gh/Kiwifu/adhan-mp3@b9180a2bb769f74cff8e378ef2cc7aaf8db5cd5b/Adhan_Al_Haram_Al_Madani_-_Al_Madinah_1_(%D8%A3%D8%B0%D8%A7%D9%86_%D8%A7%D9%84%D8%AD%D8%B1%D9%85_%D8%A7%D9%84%D9%85%D8%AF%D9%86%D9%8A_-_%D8%A7%D9%84%D9%85%D8%AF%D9%8A%D9%86%D8%A9_%D8%A7%D9%84%D9%85%D9%86%D9%88%D8%B1%D8%A9).mp3"),
            Self::AlAqsa => Some("https://cdn.jsdelivr.net/gh/Kiwifu/adhan-mp3@b9180a2bb769f74cff8e378ef2cc7aaf8db5cd5b/Najee_Qazaz_-_Al_Aqsa_Jerusalem_(%D9%86%D8%A7%D8%AC%D9%8A_%D9%82%D8%B2%D8%A7%D8%B2_-_%D8%A7%D9%84%D9%85%D8%B3%D8%AC%D8%AF_%D8%A7%D9%84%D8%A3%D9%82%D8%B5%D9%89_%D8%A7%D9%84%D9%82%D8%AF%D8%B3).mp3"),
            Self::Alafasy => Some("https://cdn.jsdelivr.net/gh/Kiwifu/adhan-mp3@b9180a2bb769f74cff8e378ef2cc7aaf8db5cd5b/Mishary_Rashid_Alafasy_1_-_Kuwait_(%D9%85%D8%B4%D8%A7%D8%B1%D9%8A_%D8%B1%D8%A7%D8%B4%D8%AF_%D8%A7%D9%84%D8%B9%D9%81%D8%A7%D8%B3%D9%8A_-_%D8%A7%D9%84%D9%83%D9%88%D9%8A%D8%AA).mp3"),
            Self::AbdulBasit => Some("https://cdn.jsdelivr.net/gh/Kiwifu/adhan-mp3@b9180a2bb769f74cff8e378ef2cc7aaf8db5cd5b/Abdulbasit_Abdusamad_2_-_Egypt_(%D8%B9%D8%A8%D8%AF_%D8%A7%D9%84%D8%A8%D8%A7%D8%B3%D8%B7_%D8%B9%D8%A8%D8%AF_%D8%A7%D9%84%D8%B5%D9%85%D8%AF_-_%D9%85%D8%B5%D8%B1).mp3"),
            Self::MuhammadRefaat => Some("https://cdn.jsdelivr.net/gh/Kiwifu/adhan-mp3@b9180a2bb769f74cff8e378ef2cc7aaf8db5cd5b/Muhammad_Refaat_-_Cairo_(%D9%85%D8%AD%D9%85%D8%AF_%D8%B1%D9%81%D8%B9%D8%AA_-_%D8%A7%D9%84%D9%82%D8%A7%D9%87%D8%B1%D8%A9).mp3"),
            Self::MustafaIsmail => Some("https://cdn.jsdelivr.net/gh/Kiwifu/adhan-mp3@b9180a2bb769f74cff8e378ef2cc7aaf8db5cd5b/Mustafa_Ismail_-_Egypt_(%D9%85%D8%B5%D8%B7%D9%81%D9%89_%D8%A5%D8%B3%D9%85%D8%A7%D8%B9%D9%8A%D9%84_-_%D9%85%D8%B5%D8%B1).mp3"),
            Self::SystemBell | Self::CustomFile => None,
        }
    }
}

/// Notification preferences for each prayer and pre-prayer reminders.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NotificationSettings {
    pub fajr: bool,
    pub dhuhr: bool,
    pub asr: bool,
    pub maghrib: bool,
    pub isha: bool,
    pub sound_enabled: bool,
    #[serde(default = "default_volume")]
    pub volume: u8, // 0-100
    #[serde(default)]
    pub muted: bool,
    #[serde(default = "default_true")]
    pub pre_reminder_enabled: bool,
    #[serde(default = "default_pre_reminder_minutes")]
    pub pre_reminder_minutes: u8, // 1-60
    #[serde(default)]
    pub pre_reminder_template: String,
    #[serde(default = "default_true")]
    pub adhan_alert_enabled: bool,
    #[serde(default)]
    pub adhan_template: String,
    #[serde(default)]
    pub adhan_voice: AdhanVoice,
    #[serde(default)]
    pub custom_adhan_path: String,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            fajr: true,
            dhuhr: true,
            asr: true,
            maghrib: true,
            isha: true,
            sound_enabled: true,
            volume: default_volume(),
            muted: false,
            pre_reminder_enabled: true,
            pre_reminder_minutes: default_pre_reminder_minutes(),
            pre_reminder_template: String::new(),
            adhan_alert_enabled: true,
            adhan_template: String::new(),
            adhan_voice: AdhanVoice::default(),
            custom_adhan_path: String::new(),
        }
    }
}

impl NotificationSettings {
    /// Whether notifications are enabled for the given daily event.
    #[must_use]
    pub const fn is_prayer_enabled(&self, kind: PrayerKind) -> bool {
        match kind {
            PrayerKind::Fajr => self.fajr,
            PrayerKind::Sunrise => false,
            PrayerKind::Dhuhr => self.dhuhr,
            PrayerKind::Asr => self.asr,
            PrayerKind::Maghrib => self.maghrib,
            PrayerKind::Isha => self.isha,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            method_id: DEFAULT_METHOD_ID,
            hanafi: false,
            location_label: "Cairo".into(),
            latitude: DEFAULT_LATITUDE,
            longitude: DEFAULT_LONGITUDE,
            timezone: DEFAULT_TIMEZONE.into(),
            use_24h: true,
            panel_layout: PanelLayout::default(),
            show_prayer_time: false,
            prayer_adjustments: PrayerAdjustments::default(),
            custom_angles: CustomAngles::default(),
            use_custom_angles: false,
            notification_settings: NotificationSettings::default(),
            compact_mode: false,
            show_celestial_horizon: true,
            show_hero_card: true,
            show_hijri_date: true,
            font_family: FontFamilyChoice::default(),
            custom_font_family: String::new(),
            panel_font_size: 0,
            popup_font_scale: 100,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_layout_responsiveness() {
        // Auto layout adapts to panel orientation:
        // Horizontal panel (top/bottom) -> false (single line)
        // Vertical panel (dock/sidebar) -> true (stacked)
        assert!(!PanelLayout::Auto.is_stacked_in(true));
        assert!(PanelLayout::Auto.is_stacked_in(false));

        // Horizontal layout is always horizontal regardless of panel
        assert!(!PanelLayout::Horizontal.is_stacked_in(true));
        assert!(!PanelLayout::Horizontal.is_stacked_in(false));

        // Stacked layout is always stacked regardless of panel
        assert!(PanelLayout::Stacked.is_stacked_in(true));
        assert!(PanelLayout::Stacked.is_stacked_in(false));
    }

    #[test]
    fn test_font_family_choices() {
        assert_eq!(FontFamilyChoice::SystemDefault.family_name(""), "");
        assert_eq!(FontFamilyChoice::NotoSans.family_name(""), "Noto Sans");
        assert_eq!(FontFamilyChoice::NotoKufiArabic.family_name(""), "Noto Kufi Arabic");
        assert_eq!(FontFamilyChoice::Cairo.family_name(""), "Cairo");
        assert_eq!(FontFamilyChoice::Custom.family_name("  Amiri  "), "Amiri");
        assert_eq!(FontFamilyChoice::Custom.family_name(""), "");
    }

    #[test]
    fn test_config_defaults() {
        let config = Config::default();
        assert_eq!(config.panel_layout, PanelLayout::Auto);
        assert!(!config.compact_mode);
        assert!(config.show_celestial_horizon);
        assert!(config.show_hero_card);
        assert!(config.show_hijri_date);
        assert_eq!(config.font_family, FontFamilyChoice::SystemDefault);
        assert_eq!(config.panel_font_size, 0);
        assert_eq!(config.popup_font_scale, 100);
        assert!(config.custom_font_family.is_empty());
    }

    #[test]
    fn test_intern_font_name_bounded_memory() {
        let cleaned = intern_font_name("Inter\n\rFont");
        assert_eq!(cleaned, "InterFont");

        for i in 0..100 {
            let _ = intern_font_name(&format!("TestFont_{i}"));
        }

        assert!(interned_font_count() <= MAX_INTERNED_FONTS);
    }
}
