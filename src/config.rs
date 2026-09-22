// SPDX-License-Identifier: MIT

// Exact float comparison is intended in this module: `watch_config` requires
// `PartialEq` for change detection, where bit-exact equality is correct.
#![allow(clippy::float_cmp)]

use crate::prayer::{DEFAULT_LATITUDE, DEFAULT_LONGITUDE, DEFAULT_METHOD_ID, DEFAULT_TIMEZONE};
use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};

/// Panel button arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum PanelLayout {
    /// Name + countdown side by side, e.g. "Asr 42m".
    #[default]
    Horizontal,
    /// Name over countdown on two lines (vertical, for narrow panels).
    Stacked,
}

impl PanelLayout {
    /// Whether the countdown goes on its own line below the name.
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
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CustomAngles {
    /// Fajr angle in degrees (default varies by method).
    pub fajr_angle: Option<f64>,
    /// Isha angle in degrees (default varies by method).
    pub isha_angle: Option<f64>,
    /// Isha interval in minutes after Maghrib (0-255, default varies by method).
    pub isha_interval: Option<u8>,
}

impl Default for CustomAngles {
    fn default() -> Self {
        Self {
            fajr_angle: None,
            isha_angle: None,
            isha_interval: None,
        }
    }
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
}

/// Notification preferences for each prayer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct NotificationSettings {
    pub fajr: bool,
    pub dhuhr: bool,
    pub asr: bool,
    pub maghrib: bool,
    pub isha: bool,
    pub sound_enabled: bool,
    pub volume: u8, // 0-100
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
            notification_settings: NotificationSettings {
                fajr: true,
                dhuhr: true,
                asr: true,
                maghrib: true,
                isha: true,
                sound_enabled: true,
                volume: 70,
            },
        }
    }
}
