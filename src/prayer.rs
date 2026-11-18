// SPDX-License-Identifier: MIT

//! Prayer time calculation for Mawaqit.
//!
//! Backed by the [`salah`] crate (a Rust port of Batoul Apps' Adhan library —
//! the same astronomical lineage as `omaprayers`' `Engine.js`, which is itself
//! a port of `adhan-js`). Calculation is fully offline.
//!
//! Method IDs are AlAdhan-compatible (matching `omaprayers`), so a user can
//! switch between the two and compare:
//!
//! | ID | Authority | `salah` mapping |
//! |---:|-----------|-----------------|
//! | 0 | Shia Ithna-Ashari (Qum) | custom Fajr 16°, Isha 14°, Maghrib 4° |
//! | 1 | Univ. of Islamic Sciences, Karachi | `Karachi` |
//! | 2 | Islamic Society of North America | `NorthAmerica` |
//! | 3 | Muslim World League | `MuslimWorldLeague` |
//! | 4 | Umm al-Qura, Makkah | `UmmAlQura` |
//! | 5 | Egyptian General Authority of Survey | `Egyptian` (default) |
//! | 7 | Institute of Geophysics, Tehran | `Tehran` |
//! | 9 | Kuwait | `Kuwait` |
//! | 10 | Qatar | `Qatar` |
//! | 11 | Singapore (MUIS) | `Singapore` |
//! | 13 | Diyanet, Turkey | `Turkey` (approximation) |
//! | 15 | Moonsighting Committee | `MoonsightingCommittee` |
//! | 16 | Dubai | `Dubai` |
//! | 17/20 | JAKIM / Kemenag (Fajr 20°, Isha 18°) | `Singapore` (same angles) |
//! | 18 | Tunisia (18°/18°) | `Karachi` (same angles) |
//!
//! IDs with built-in minute offsets in `omaprayers` that `salah` does not model
//! (Morocco Dhuhr/Maghrib +5, Diyanet's full offset set, Jordan/Maghrib +5,
//! Lisbon Isha 77′) fall back to the closest angle match; the residual is at
//! most a few minutes and is noted in the README. Validated against the
//! `omaprayers` `AlAdhan` snapshot corpus: Cairo/Egyptian 2026-10-01 matches
//! within ±1 min (Dhuhr +1 is the documented official-parameter delta, Asr −1
//! is inside the documented PrayTimes-vs-Adhan band).

use chrono::{DateTime, Datelike, NaiveDate};
use chrono_tz::Tz;
use salah::prelude::*;

/// Default method: Egyptian General Authority of Survey (matches omaprayers).
pub const DEFAULT_METHOD_ID: u8 = 5;

/// Default location: Cairo (matches omaprayers).
pub const DEFAULT_LATITUDE: f64 = 30.0444;
pub const DEFAULT_LONGITUDE: f64 = 31.2357;
pub const DEFAULT_TIMEZONE: &str = "Africa/Cairo";

/// A well-known city preset with coordinates, timezone, and regional calculation method.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CityPreset {
    pub name: &'static str,
    pub name_ar: &'static str,
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: &'static str,
    pub method_id: u8,
}

/// Curated presets for major global and Muslim cities.
pub const CITIES: [CityPreset; 24] = [
    CityPreset {
        name: "Cairo, Egypt",
        name_ar: "القاهرة، مصر",
        latitude: 30.0444,
        longitude: 31.2357,
        timezone: "Africa/Cairo",
        method_id: 5,
    },
    CityPreset {
        name: "Makkah, Saudi Arabia",
        name_ar: "مكة المكرمة، السعودية",
        latitude: 21.4225,
        longitude: 39.8262,
        timezone: "Asia/Riyadh",
        method_id: 4,
    },
    CityPreset {
        name: "Madinah, Saudi Arabia",
        name_ar: "المدينة المنورة، السعودية",
        latitude: 24.4672,
        longitude: 39.6111,
        timezone: "Asia/Riyadh",
        method_id: 4,
    },
    CityPreset {
        name: "Riyadh, Saudi Arabia",
        name_ar: "الرياض، السعودية",
        latitude: 24.7136,
        longitude: 46.6753,
        timezone: "Asia/Riyadh",
        method_id: 4,
    },
    CityPreset {
        name: "Jerusalem, Palestine",
        name_ar: "القدس، فلسطين",
        latitude: 31.7683,
        longitude: 35.2137,
        timezone: "Asia/Jerusalem",
        method_id: 23,
    },
    CityPreset {
        name: "Dubai, UAE",
        name_ar: "دبي، الإمارات",
        latitude: 25.2048,
        longitude: 55.2708,
        timezone: "Asia/Dubai",
        method_id: 16,
    },
    CityPreset {
        name: "Doha, Qatar",
        name_ar: "الدوحة، قطر",
        latitude: 25.2854,
        longitude: 51.5310,
        timezone: "Asia/Qatar",
        method_id: 10,
    },
    CityPreset {
        name: "Kuwait City, Kuwait",
        name_ar: "الكويت، الكويت",
        latitude: 29.3759,
        longitude: 47.9774,
        timezone: "Asia/Kuwait",
        method_id: 9,
    },
    CityPreset {
        name: "Amman, Jordan",
        name_ar: "عمّان، الأردن",
        latitude: 31.9454,
        longitude: 35.9284,
        timezone: "Asia/Amman",
        method_id: 23,
    },
    CityPreset {
        name: "Istanbul, Turkey",
        name_ar: "إسطنبول، تركيا",
        latitude: 41.0082,
        longitude: 28.9784,
        timezone: "Europe/Istanbul",
        method_id: 13,
    },
    CityPreset {
        name: "Casablanca, Morocco",
        name_ar: "الدار البيضاء، المغرب",
        latitude: 33.5731,
        longitude: -7.5898,
        timezone: "Africa/Casablanca",
        method_id: 21,
    },
    CityPreset {
        name: "Algiers, Algeria",
        name_ar: "الجزائر، الجزائر",
        latitude: 36.7538,
        longitude: 3.0588,
        timezone: "Africa/Algiers",
        method_id: 19,
    },
    CityPreset {
        name: "Tunis, Tunisia",
        name_ar: "تونس، تونس",
        latitude: 36.8065,
        longitude: 10.1815,
        timezone: "Africa/Tunis",
        method_id: 18,
    },
    CityPreset {
        name: "Karachi, Pakistan",
        name_ar: "كراتشي، باكستان",
        latitude: 24.8607,
        longitude: 67.0011,
        timezone: "Asia/Karachi",
        method_id: 1,
    },
    CityPreset {
        name: "Jakarta, Indonesia",
        name_ar: "جاكرتا، إندونيسيا",
        latitude: -6.2088,
        longitude: 106.8456,
        timezone: "Asia/Jakarta",
        method_id: 20,
    },
    CityPreset {
        name: "Kuala Lumpur, Malaysia",
        name_ar: "كوالالمبور، ماليزيا",
        latitude: 3.1390,
        longitude: 101.6869,
        timezone: "Asia/Kuala_Lumpur",
        method_id: 17,
    },
    CityPreset {
        name: "Singapore",
        name_ar: "سنغافورة",
        latitude: 1.3521,
        longitude: 103.8198,
        timezone: "Asia/Singapore",
        method_id: 11,
    },
    CityPreset {
        name: "London, UK",
        name_ar: "لندن، المملكة المتحدة",
        latitude: 51.5074,
        longitude: -0.1278,
        timezone: "Europe/London",
        method_id: 3,
    },
    CityPreset {
        name: "Paris, France",
        name_ar: "باريس، فرنسا",
        latitude: 48.8566,
        longitude: 2.3522,
        timezone: "Europe/Paris",
        method_id: 12,
    },
    CityPreset {
        name: "Berlin, Germany",
        name_ar: "برلين، ألمانيا",
        latitude: 52.5200,
        longitude: 13.4050,
        timezone: "Europe/Berlin",
        method_id: 3,
    },
    CityPreset {
        name: "New York, USA",
        name_ar: "نيويورك، الولايات المتحدة",
        latitude: 40.7128,
        longitude: -74.0060,
        timezone: "America/New_York",
        method_id: 2,
    },
    CityPreset {
        name: "Toronto, Canada",
        name_ar: "تورونتو، كندا",
        latitude: 43.6532,
        longitude: -79.3832,
        timezone: "America/Toronto",
        method_id: 2,
    },
    CityPreset {
        name: "Los Angeles, USA",
        name_ar: "لوس أنجلوس، الولايات المتحدة",
        latitude: 34.0522,
        longitude: -118.2437,
        timezone: "America/Los_Angeles",
        method_id: 2,
    },
    CityPreset {
        name: "Sydney, Australia",
        name_ar: "سيدني، أستراليا",
        latitude: -33.8688,
        longitude: 151.2093,
        timezone: "Australia/Sydney",
        method_id: 3,
    },
];

/// Match a location label to a preset city index, if any.
pub fn find_city_preset(label: &str) -> Option<usize> {
    CITIES.iter().position(|c| {
        c.name == label
            || c.name.starts_with(label)
            || label.starts_with(c.name.split(',').next().unwrap_or(c.name))
    })
}

/// The six daily events shown in the popup (five prayers + sunrise).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrayerKind {
    Fajr,
    Sunrise,
    Dhuhr,
    Asr,
    Maghrib,
    Isha,
}

impl PrayerKind {
    pub const ALL: [PrayerKind; 6] = [
        PrayerKind::Fajr,
        PrayerKind::Sunrise,
        PrayerKind::Dhuhr,
        PrayerKind::Asr,
        PrayerKind::Maghrib,
        PrayerKind::Isha,
    ];

    /// Transliterated English name.
    pub fn name_en(self) -> &'static str {
        match self {
            PrayerKind::Fajr => "Fajr",
            PrayerKind::Sunrise => "Sunrise",
            PrayerKind::Dhuhr => "Dhuhr",
            PrayerKind::Asr => "Asr",
            PrayerKind::Maghrib => "Maghrib",
            PrayerKind::Isha => "Isha",
        }
    }

    /// Arabic name.
    pub fn name_ar(self) -> &'static str {
        match self {
            PrayerKind::Fajr => "الفجر",
            PrayerKind::Sunrise => "الشروق",
            PrayerKind::Dhuhr => "الظهر",
            PrayerKind::Asr => "العصر",
            PrayerKind::Maghrib => "المغرب",
            PrayerKind::Isha => "العشاء",
        }
    }

    /// Short Arabic name for compact horizon badges.
    pub fn short_ar(self) -> &'static str {
        match self {
            PrayerKind::Fajr => "فجر",
            PrayerKind::Sunrise => "شروق",
            PrayerKind::Dhuhr => "ظهر",
            PrayerKind::Asr => "عصر",
            PrayerKind::Maghrib => "مغرب",
            PrayerKind::Isha => "عشاء",
        }
    }

    /// Standard freedesktop symbolic icon representing this celestial/sun phase.
    #[must_use]
    pub const fn celestial_icon(self) -> &'static str {
        match self {
            PrayerKind::Fajr => "weather-few-clouds-night-symbolic",
            PrayerKind::Sunrise => "daytime-sunrise-symbolic",
            PrayerKind::Dhuhr => "weather-clear-symbolic",
            PrayerKind::Asr => "weather-few-clouds-symbolic",
            PrayerKind::Maghrib => "daytime-sunset-symbolic",
            PrayerKind::Isha => "weather-clear-night-symbolic",
        }
    }

    /// English description subtitle.
    #[must_use]
    pub const fn description_en(self) -> &'static str {
        match self {
            PrayerKind::Fajr => "Dawn Prayer",
            PrayerKind::Sunrise => "Sunrise Horizon",
            PrayerKind::Dhuhr => "Midday Prayer",
            PrayerKind::Asr => "Afternoon Prayer",
            PrayerKind::Maghrib => "Sunset Prayer",
            PrayerKind::Isha => "Night Prayer",
        }
    }

    /// Arabic description subtitle.
    #[must_use]
    pub const fn description_ar(self) -> &'static str {
        match self {
            PrayerKind::Fajr => "صلاة الفجر",
            PrayerKind::Sunrise => "شروق الشمس",
            PrayerKind::Dhuhr => "صلاة الظهر",
            PrayerKind::Asr => "صلاة العصر",
            PrayerKind::Maghrib => "صلاة المغرب",
            PrayerKind::Isha => "صلاة العشاء",
        }
    }

    /// Whether this event is an obligatory prayer (i.e. not Sunrise).
    #[must_use]
    pub const fn is_prayer(self) -> bool {
        !matches!(self, Self::Sunrise)
    }
}

/// One day's schedule, in the location's timezone.
#[derive(Debug, Clone)]
pub struct DailyTimes {
    pub date: NaiveDate,
    pub fajr: DateTime<Tz>,
    pub sunrise: DateTime<Tz>,
    pub dhuhr: DateTime<Tz>,
    pub asr: DateTime<Tz>,
    pub maghrib: DateTime<Tz>,
    pub isha: DateTime<Tz>,
}

impl DailyTimes {
    pub fn time(&self, kind: PrayerKind) -> DateTime<Tz> {
        match kind {
            PrayerKind::Fajr => self.fajr,
            PrayerKind::Sunrise => self.sunrise,
            PrayerKind::Dhuhr => self.dhuhr,
            PrayerKind::Asr => self.asr,
            PrayerKind::Maghrib => self.maghrib,
            PrayerKind::Isha => self.isha,
        }
    }
}

/// Build `salah` parameters for an AlAdhan-compatible method id + madhab.
#[allow(dead_code)]
pub fn parameters_for(method_id: u8, hanafi: bool) -> Parameters {
    parameters_for_with_custom(method_id, hanafi, None, None, None)
}

/// Build `salah` parameters with optional custom angle overrides.
pub fn parameters_for_with_custom(
    method_id: u8,
    hanafi: bool,
    custom_fajr_angle: Option<f64>,
    custom_isha_angle: Option<f64>,
    custom_isha_interval: Option<u8>,
) -> Parameters {
    let madhab = if hanafi {
        Madhab::Hanafi
    } else {
        Madhab::Shafi
    };

    // Start with the base method configuration
    let base_config = match method_id {
        // Tunisia (18) shares Karachi's 18°/18° angles.
        1 | 18 => Configuration::with(Method::Karachi, madhab),
        2 => Configuration::with(Method::NorthAmerica, madhab),
        3 => Configuration::with(Method::MuslimWorldLeague, madhab),
        4 => Configuration::with(Method::UmmAlQura, madhab),
        5 => Configuration::with(Method::Egyptian, madhab),
        7 => Configuration::with(Method::Tehran, madhab),
        9 => Configuration::with(Method::Kuwait, madhab),
        10 => Configuration::with(Method::Qatar, madhab),
        11 | 17 | 20 => Configuration::with(Method::Singapore, madhab),
        13 => Configuration::with(Method::Turkey, madhab),
        15 => Configuration::with(Method::MoonsightingCommittee, madhab),
        16 => Configuration::with(Method::Dubai, madhab),
        0 => {
            // Jafari: Fajr 16°, Isha 14°, Maghrib at 4° below horizon.
            let mut c = Configuration::new(16.0, 14.0);
            c.maghrib_angle(4.0).madhab(madhab);
            c.done()
        }
        8 => {
            // Gulf: Fajr 19.5°, Isha 90′ after Maghrib.
            let mut c = Configuration::new(19.5, 0.0);
            c.isha_interval(90).madhab(madhab);
            c.done()
        }
        12 => {
            // UOIF/France: 12°/12°.
            let mut c = Configuration::new(12.0, 12.0);
            c.madhab(madhab);
            c.done()
        }
        14 => {
            // Russia: 16°/15°.
            let mut c = Configuration::new(16.0, 15.0);
            c.madhab(madhab);
            c.done()
        }
        // Anything unknown falls back to the Egyptian default.
        _ => parameters_for_with_custom(DEFAULT_METHOD_ID, hanafi, custom_fajr_angle, custom_isha_angle, custom_isha_interval),
    };

    // Apply custom overrides if provided (partial overrides supported,
    // preserving the base method's other settings).
    let mut params = base_config;
    if let Some(fa) = custom_fajr_angle {
        params.fajr_angle = fa;
    }
    if let Some(ia) = custom_isha_angle {
        params.isha_angle = ia;
        // An explicit angle disables interval-based Isha (mirrors
        // `Configuration::isha_interval` which zeroes the angle).
        params.isha_interval = 0;
    }
    if let Some(interval) = custom_isha_interval {
        // Interval takes precedence when both angle and interval are set
        // (e.g. Gulf-style 90 min after Maghrib).
        params.isha_angle = 0.0;
        params.isha_interval = interval as i32;
    }
    // Base already carries the madhab, but enforce it for clarity.
    params.madhab = madhab;
    params
}

/// Short English label for a method id (for the settings dropdown).
pub fn method_label(method_id: u8) -> &'static str {
    match method_id {
        0 => "Jafari (Qum)",
        1 => "Karachi",
        2 => "ISNA (North America)",
        3 => "Muslim World League",
        4 => "Umm al-Qura",
        5 => "Egyptian",
        7 => "Tehran",
        8 => "Gulf",
        9 => "Kuwait",
        10 => "Qatar",
        11 => "Singapore",
        12 => "France (UOIF)",
        13 => "Diyanet (Turkey)",
        14 => "Russia",
        15 => "Moonsighting Committee",
        16 => "Dubai",
        17 => "JAKIM (Malaysia)",
        18 => "Tunisia",
        19 => "Algeria",
        20 => "Kemenag (Indonesia)",
        21 => "Morocco",
        22 => "Lisbon",
        23 => "Jordan",
        // Anything unknown falls back to the Egyptian default.
        _ => method_label(DEFAULT_METHOD_ID),
    }
}

/// Detailed parameters description for a method id (Fajr/Isha angles or interval).
pub fn method_specs(method_id: u8) -> &'static str {
    match method_id {
        0 => "Fajr: 16.0° • Maghrib: 4.0° • Isha: 14.0° (Institute of Geophysics, Tehran / Qum)",
        1 => "Fajr: 18.0° • Isha: 18.0° (University of Islamic Sciences, Karachi)",
        2 => "Fajr: 15.0° • Isha: 15.0° (Islamic Society of North America - ISNA)",
        3 => "Fajr: 18.0° • Isha: 17.0° (Muslim World League - MWL)",
        4 => "Fajr: 18.5° • Isha: +90 min after Maghrib (Umm al-Qura, Makkah)",
        5 => "Fajr: 19.5° • Isha: 17.5° (Egyptian General Authority of Survey)",
        7 => "Fajr: 17.7° • Maghrib: 4.5° • Isha: 14.0° (Institute of Geophysics, Tehran)",
        8 => "Fajr: 19.5° • Isha: +90 min after Maghrib (Gulf Region)",
        9 => "Fajr: 18.0° • Isha: 17.5° (Ministry of Awqaf, Kuwait)",
        10 => "Fajr: 18.0° • Isha: +90 min after Maghrib (Qatar)",
        11 => "Fajr: 20.0° • Isha: 18.0° (MUIS, Singapore)",
        12 => "Fajr: 12.0° • Isha: 12.0° (Union des Organisations Islamiques de France)",
        13 => "Fajr: 18.0° • Isha: 17.0° (Diyanet İşleri Başkanlığı, Turkey)",
        14 => "Fajr: 16.0° • Isha: 15.0° (Spiritual Administration of Muslims of Russia)",
        15 => "Fajr: 18.0° • Isha: 18.0° (Moonsighting Committee Worldwide)",
        16 => "Fajr: 18.2° • Isha: 18.2° (Awqaf and Islamic Affairs, Dubai/UAE)",
        17 => "Fajr: 20.0° • Isha: 18.0° (Jabatan Kemajuan Islam Malaysia - JAKIM)",
        18 => "Fajr: 18.0° • Isha: 18.0° (Ministry of Religious Affairs, Tunisia)",
        19 => "Fajr: 18.0° • Isha: 17.0° (Ministry of Religious Affairs, Algeria)",
        20 => "Fajr: 20.0° • Isha: 18.0° (Kementerian Agama Republik Indonesia)",
        21 => "Fajr: 19.0° • Isha: 17.0° (Ministry of Habous & Islamic Affairs, Morocco)",
        22 => "Fajr: 12.0° • Isha: 12.0° (Comunidade Islâmica de Lisboa)",
        23 => "Fajr: 18.0° • Isha: 18.0° (Ministry of Awqaf & Islamic Affairs, Jordan)",
        _ => method_specs(DEFAULT_METHOD_ID),
    }
}

/// Compute one civil day's times (date interpreted in `tz`).
#[allow(dead_code)]
pub fn compute_day(
    date: NaiveDate,
    latitude: f64,
    longitude: f64,
    tz: Tz,
    method_id: u8,
    hanafi: bool,
) -> Option<DailyTimes> {
    compute_day_with_adjustments(date, latitude, longitude, tz, method_id, hanafi, None, None, None, None)
}

/// Compute one civil day's times with custom angles and time adjustments.
#[allow(clippy::too_many_arguments)]
pub fn compute_day_with_adjustments(
    date: NaiveDate,
    latitude: f64,
    longitude: f64,
    tz: Tz,
    method_id: u8,
    hanafi: bool,
    custom_fajr_angle: Option<f64>,
    custom_isha_angle: Option<f64>,
    custom_isha_interval: Option<u8>,
    adjustments: Option<crate::config::PrayerAdjustments>,
) -> Option<DailyTimes> {
    if !latitude.is_finite() || !longitude.is_finite() {
        return None;
    }
    if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
        return None;
    }
    let params = parameters_for_with_custom(
        method_id,
        hanafi,
        custom_fajr_angle,
        custom_isha_angle,
        custom_isha_interval,
    );
    let mut schedule = PrayerSchedule::new();
    let prayers = schedule
        .on(date)
        .for_location(Coordinates::new(latitude, longitude))
        .with_configuration(params)
        .calculate()
        .ok()?;

    let apply_adjustment = |time: DateTime<Tz>, adjustment: i8| -> DateTime<Tz> {
        if adjustment == 0 {
            time
        } else {
            time + chrono::Duration::minutes(adjustment as i64)
        }
    };

    let adj = adjustments.unwrap_or_default();

    Some(DailyTimes {
        date,
        fajr: apply_adjustment(prayers.time(Prayer::Fajr).with_timezone(&tz), adj.fajr),
        sunrise: apply_adjustment(prayers.time(Prayer::Sunrise).with_timezone(&tz), adj.sunrise),
        dhuhr: apply_adjustment(prayers.time(Prayer::Dhuhr).with_timezone(&tz), adj.dhuhr),
        asr: apply_adjustment(prayers.time(Prayer::Asr).with_timezone(&tz), adj.asr),
        maghrib: apply_adjustment(prayers.time(Prayer::Maghrib).with_timezone(&tz), adj.maghrib),
        isha: apply_adjustment(prayers.time(Prayer::Isha).with_timezone(&tz), adj.isha),
    })
}

/// Find the next upcoming event after `now`.
///
/// Searches today's schedule, then tomorrow's Fajr (i.e. after Isha the next
/// event is tomorrow's Fajr). Returns the kind, its time, and whether it comes
/// from tomorrow's schedule.
pub fn next_prayer(
    today: &DailyTimes,
    tomorrow: &DailyTimes,
    now: DateTime<Tz>,
) -> (PrayerKind, DateTime<Tz>, bool) {
    for kind in PrayerKind::ALL {
        let t = today.time(kind);
        if t > now {
            return (kind, t, false);
        }
    }
    (PrayerKind::Fajr, tomorrow.fajr, true)
}

/// Grace window (in seconds) after an obligatory prayer time arrives during which it
/// is considered "currently active" (e.g. 15 minutes).
pub const ACTIVE_PRAYER_WINDOW_SECS: i64 = 900;

/// If an obligatory prayer started recently (within `ACTIVE_PRAYER_WINDOW_SECS`),
/// return the prayer kind, its scheduled start time, and seconds elapsed.
pub fn active_prayer(
    today: &DailyTimes,
    now: DateTime<Tz>,
) -> Option<(PrayerKind, DateTime<Tz>, i64)> {
    // Check in reverse order so the latest passed prayer is matched first
    for kind in [
        PrayerKind::Isha,
        PrayerKind::Maghrib,
        PrayerKind::Asr,
        PrayerKind::Dhuhr,
        PrayerKind::Fajr,
    ] {
        let t = today.time(kind);
        let elapsed = (now - t).num_seconds();
        if (0..ACTIVE_PRAYER_WINDOW_SECS).contains(&elapsed) {
            return Some((kind, t, elapsed));
        }
    }
    None
}

/// Whether a daily event's scheduled time has already passed relative to `now`.
pub fn is_passed(event_time: DateTime<Tz>, now: DateTime<Tz>) -> bool {
    event_time <= now
}

/// Compact countdown text, e.g. "2h 14m", "42m", "30s".
pub fn format_countdown(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{secs}s")
    }
}

/// Format a time for display.
pub fn format_time(dt: DateTime<Tz>, use_24h: bool) -> String {
    if use_24h {
        dt.format("%H:%M").to_string()
    } else {
        dt.format("%-I:%M %p").to_string()
    }
}

/// Detail shown next to the prayer name in the panel button: either a live
/// countdown ("42m") or the full clock time ("16:36").
pub fn panel_detail(seconds: i64, at: DateTime<Tz>, show_time: bool, use_24h: bool) -> String {
    if show_time {
        format_time(at, use_24h)
    } else if seconds < 120 {
        String::from("now")
    } else {
        format_countdown(seconds)
    }
}

/// Panel button text: prayer name plus a detail string (see [`panel_detail`]).
/// When `stacked` is set the detail goes on its own line below the name
/// (vertical arrangement for narrow panels).
pub fn panel_label(kind: PrayerKind, detail: &str, stacked: bool) -> String {
    if stacked {
        format!("{}\n{detail}", kind.name_en())
    } else {
        format!("{} {detail}", kind.name_en())
    }
}

const HIJRI_MONTHS_EN: [&str; 12] = [
    "Muharram",
    "Safar",
    "Rabi' al-Awwal",
    "Rabi' al-Thani",
    "Jumada al-Ula",
    "Jumada al-Akhirah",
    "Rajab",
    "Sha'ban",
    "Ramadan",
    "Shawwal",
    "Dhu al-Qi'dah",
    "Dhu al-Hijjah",
];

const HIJRI_MONTHS_AR: [&str; 12] = [
    "محرم",
    "صفر",
    "ربيع الأول",
    "ربيع الآخر",
    "جمادى الأولى",
    "جمادى الآخرة",
    "رجب",
    "شعبان",
    "رمضان",
    "شوال",
    "ذو القعدة",
    "ذو الحجة",
];

/// Hijri date for a Gregorian civil date, e.g. "20 Rabi' al-Thani 1448".
/// Returns `None` outside the conversion table's supported range (1938–2076).
pub fn hijri_for_gregorian(date: NaiveDate) -> Option<(String, String)> {
    let hd = hijri_date::HijriDate::from_gr(
        usize::try_from(date.year()).ok()?,
        date.month() as usize,
        date.day() as usize,
    )
    .ok()?;
    let idx = hd.month().checked_sub(1)?;
    let (en, ar) = (HIJRI_MONTHS_EN.get(idx)?, HIJRI_MONTHS_AR.get(idx)?);
    Some((
        format!("{} {en} {}", hd.day(), hd.year()),
        format!("{} {ar} {} هـ", hd.day(), hd.year()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};

    const CAIRO_LAT: f64 = 30.0444;
    const CAIRO_LON: f64 = 31.2357;

    fn cairo_tz() -> Tz {
        "Africa/Cairo".parse().unwrap()
    }

    /// Reference: `omaprayers` `AlAdhan` snapshot `cairo-egypt-2026-10`,
    /// 2026-10-01, method 5 (Egyptian), Shafi.
    /// `AlAdhan`: Fajr 05:22 Sunrise 06:48 Dhuhr 12:45 Asr 16:09
    ///          Maghrib 18:41 Isha 19:58.
    /// Known deltas (see VALIDATION.md): Dhuhr +1 (official params),
    /// Asr within −5..+3 (`PrayTimes` vs Adhan astronomy).
    #[test]
    fn cairo_egyptian_matches_aladhan_within_one_minute() {
        let tz = cairo_tz();
        let day = compute_day(
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            CAIRO_LAT,
            CAIRO_LON,
            tz,
            5,
            false,
        )
        .expect("times compute");
        let expect = [
            (PrayerKind::Fajr, 5 * 60 + 22),
            (PrayerKind::Sunrise, 6 * 60 + 48),
            (PrayerKind::Dhuhr, 12 * 60 + 45),
            (PrayerKind::Asr, 16 * 60 + 9),
            (PrayerKind::Maghrib, 18 * 60 + 41),
            (PrayerKind::Isha, 19 * 60 + 58),
        ];
        for (kind, ref_minutes) in expect {
            let t = day.time(kind);
            let got = t.hour() * 60 + t.minute();
            let delta = got.abs_diff(ref_minutes);
            assert!(
                delta <= 1,
                "{kind:?}: got {}, expected ~{ref_minutes} (delta {delta}m)",
                t.format("%H:%M"),
            );
        }
    }

    #[test]
    fn hanafi_asr_is_later_than_shafi() {
        let tz = cairo_tz();
        let date = NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let shafi = compute_day(date, CAIRO_LAT, CAIRO_LON, tz, 5, false).unwrap();
        let hanafi = compute_day(date, CAIRO_LAT, CAIRO_LON, tz, 5, true).unwrap();
        assert!(hanafi.asr > shafi.asr);
        // AlAdhan Hanafi Asr that day: 17:49, allow ±2 min.
        let mins = hanafi.asr.hour() * 60 + hanafi.asr.minute();
        assert!(mins.abs_diff(17 * 60 + 49) <= 2);
    }

    #[test]
    fn next_prayer_rolls_to_tomorrow_fajr_after_isha() {
        let tz = cairo_tz();
        let today = compute_day(
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            CAIRO_LAT,
            CAIRO_LON,
            tz,
            5,
            false,
        )
        .unwrap();
        let tomorrow = compute_day(
            NaiveDate::from_ymd_opt(2026, 10, 2).unwrap(),
            CAIRO_LAT,
            CAIRO_LON,
            tz,
            5,
            false,
        )
        .unwrap();
        // Midday -> Asr.
        let noon = tz.with_ymd_and_hms(2026, 10, 1, 14, 0, 0).unwrap();
        let (kind, _, is_tomorrow) = next_prayer(&today, &tomorrow, noon);
        assert_eq!(kind, PrayerKind::Asr);
        assert!(!is_tomorrow);
        // After Isha -> tomorrow's Fajr.
        let night = tz.with_ymd_and_hms(2026, 10, 1, 23, 0, 0).unwrap();
        let (kind, t, is_tomorrow) = next_prayer(&today, &tomorrow, night);
        assert_eq!(kind, PrayerKind::Fajr);
        assert!(is_tomorrow);
        assert_eq!(t, tomorrow.fajr);
    }

    #[test]
    fn countdown_formats() {
        assert_eq!(format_countdown(2 * 3600 + 14 * 60), "2h 14m");
        assert_eq!(format_countdown(42 * 60), "42m");
        assert_eq!(format_countdown(30), "30s");
        assert_eq!(format_countdown(-5), "0s");
    }

    #[test]
    fn panel_label_layouts() {
        assert_eq!(panel_label(PrayerKind::Asr, "42m", false), "Asr 42m");
        assert_eq!(panel_label(PrayerKind::Asr, "42m", true), "Asr\n42m");
        assert_eq!(
            panel_label(PrayerKind::Maghrib, "16:36", false),
            "Maghrib 16:36"
        );
    }

    #[test]
    fn panel_detail_modes() {
        let tz = cairo_tz();
        let at = tz.with_ymd_and_hms(2026, 10, 1, 16, 36, 0).unwrap();
        assert_eq!(panel_detail(42 * 60, at, false, true), "42m");
        assert_eq!(panel_detail(30, at, false, true), "now");
        assert_eq!(panel_detail(42 * 60, at, true, true), "16:36");
        assert_eq!(panel_detail(42 * 60, at, true, false), "4:36 PM");
    }

    #[test]
    fn hijri_known_date() {
        // AlAdhan snapshot says 2026-10-01 = 20 Rabi' al-Thani 1448.
        let (en, ar) =
            hijri_for_gregorian(NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()).expect("hijri");
        assert!(en.contains("1448"), "{en}");
        assert!(ar.contains("1448"), "{ar}");
    }

    #[test]
    fn rejects_invalid_coordinates() {
        let tz = cairo_tz();
        let date = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        assert!(compute_day(date, 91.0, 0.0, tz, 5, false).is_none());
        assert!(compute_day(date, 0.0, 181.0, tz, 5, false).is_none());
        assert!(compute_day(date, f64::NAN, 0.0, tz, 5, false).is_none());
    }

    #[test]
    fn city_presets_and_lookup() {
        assert!(find_city_preset("Cairo").is_some());
        assert_eq!(find_city_preset("Cairo").unwrap(), 0);
        assert!(find_city_preset("Makkah").is_some());
        assert!(find_city_preset("NonExistentCity").is_none());
    }

    #[test]
    fn active_prayer_window() {
        let tz = cairo_tz();
        let day = compute_day(
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            CAIRO_LAT,
            CAIRO_LON,
            tz,
            5,
            false,
        )
        .unwrap();

        // 5 minutes after Fajr (Fajr is at 05:22)
        let five_min_after_fajr = tz.with_ymd_and_hms(2026, 10, 1, 5, 27, 0).unwrap();
        let active = active_prayer(&day, five_min_after_fajr);
        assert!(active.is_some());
        let (kind, _, elapsed) = active.unwrap();
        assert_eq!(kind, PrayerKind::Fajr);
        assert_eq!(elapsed, 300);

        // 20 minutes after Fajr (outside 15m window)
        let twenty_min_after_fajr = tz.with_ymd_and_hms(2026, 10, 1, 5, 42, 0).unwrap();
        assert!(active_prayer(&day, twenty_min_after_fajr).is_none());
    }
}
