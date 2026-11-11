//! The sun: where it stands over a site at a given date and time, when it
//! rises and sets, and when the seasons turn.
//!
//! **The algorithm** is NOAA's solar position calculator — the equations
//! of Meeus's *Astronomical Algorithms* (ch. 25, the low-precision solar
//! coordinates; ch. 28, the equation of time) as the NOAA spreadsheet
//! arranges them: the Julian century from the instant in UT, the sun's
//! mean longitude and anomaly, the equation of centre, the apparent
//! longitude corrected for nutation, the obliquity, then declination and
//! the equation of time; local time becomes true solar time through the
//! equation of time and the longitude, the hour angle follows, and the
//! zenith and azimuth come from the spherical triangle. Elevation carries
//! NOAA's atmospheric refraction correction (about 0.57° at the horizon,
//! nothing above 85°), so `elevation` is what an observer sees and
//! `elevation_geometric` is where the sun's centre is; sunrise and sunset
//! are the times the geometric centre is 0.833° below the horizon, the
//! convention every almanac uses.
//!
//! **Accuracy.** Meeus's series place the sun to 0.01° of ecliptic
//! longitude between 1950 and 2050, and the spreadsheet's own claim is a
//! minute on sunrise and sunset within ±72° of latitude. What this module
//! promises is coarser, and deliberately so: **azimuth and elevation to
//! 0.5°**, sunrise and sunset to a minute. The slack is for what the model
//! does not know — the refraction correction assumes a standard atmosphere
//! (1010 mb, 10 °C) and the true bending varies with the weather by tenths
//! of a degree near the horizon; a time given to the minute moves the sun a
//! quarter of a degree; and a shadow study is drawn from a tessellation. The
//! tests hold the implementation to much tighter figures — an independent
//! evaluation of the Astronomical Almanac's low-precision formulae agrees to
//! 0.05° over a grid of dates and places, solar noon on an equinox stands
//! `90° − |latitude|` high to 0.1°, the equation of time reaches its
//! textbook extrema — so the 0.5° is a contract with margin, not a
//! measurement.
//!
//! **Frames.** [`direction`] turns an azimuth (compass: 0 north, 90 east,
//! clockwise from above) and an elevation into a unit vector *toward* the
//! sun in a frame with `+y` north, `+x` east and `+z` up, which is the
//! frame a Cadpole model sits in unless it says otherwise; [`compass_to_model`]
//! rotates that vector for a model whose north is not `+y` (`--north`,
//! `render_defaults(north=)`). The seasons ([`season_instants`], ch. 27 of
//! Meeus) are good to a minute over 2000–2100 and are given as calendar
//! dates in UTC.
//!
//! Every transcendental goes through [`crate::fmath`], so a script's sun is
//! the same bits on every target.

use crate::fmath;
use crate::geom::Vec3;

/// A calendar date, proleptic Gregorian.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    pub fn new(year: i32, month: u32, day: u32) -> Result<Date, String> {
        if !(1..=12).contains(&month) {
            return Err(format!("month {month} is not between 1 and 12"));
        }
        let days = days_in_month(year, month);
        if day == 0 || day > days {
            return Err(format!("{year:04}-{month:02} has {days} days, not {day}"));
        }
        Ok(Date { year, month, day })
    }

    /// `YYYY-MM-DD`.
    pub fn parse(s: &str) -> Result<Date, String> {
        let s = s.trim();
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
            return Err(format!("{s:?} is not a date of the form YYYY-MM-DD"));
        }
        let year: i32 = parts[0].parse().map_err(|_| format!("{s:?}: {:?} is not a year", parts[0]))?;
        let month: u32 = parts[1].parse().map_err(|_| format!("{s:?}: {:?} is not a month", parts[1]))?;
        let day: u32 = parts[2].parse().map_err(|_| format!("{s:?}: {:?} is not a day", parts[2]))?;
        Date::new(year, month, day).map_err(|e| format!("{s:?}: {e}"))
    }

    /// The Julian day at 0h UT of this date.
    pub fn julian_day(&self) -> f64 {
        let (y, m, d) = (self.year as i64, self.month as i64, self.day as i64);
        let a = (14 - m) / 12;
        let yy = y + 4800 - a;
        let mm = m + 12 * a - 3;
        let jdn = d + (153 * mm + 2) / 5 + 365 * yy + yy / 4 - yy / 100 + yy / 400 - 32045;
        jdn as f64 - 0.5
    }

    /// The calendar date and the fraction of the day (0 at 0h UT) of a
    /// Julian day.
    pub fn from_julian(jd: f64) -> (Date, f64) {
        let jd = jd + 0.5;
        let z = jd.floor();
        let f = jd - z;
        let a = if z < 2299161.0 {
            z
        } else {
            let alpha = ((z - 1867216.25) / 36524.25).floor();
            z + 1.0 + alpha - (alpha / 4.0).floor()
        };
        let b = a + 1524.0;
        let c = ((b - 122.1) / 365.25).floor();
        let d = (365.25 * c).floor();
        let e = ((b - d) / 30.6001).floor();
        let day = (b - d - (30.6001 * e).floor()) as u32;
        let month = if e < 14.0 { e - 1.0 } else { e - 13.0 } as u32;
        let year = if month > 2 { c - 4716.0 } else { c - 4715.0 } as i32;
        (Date { year, month, day }, f)
    }
}

impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
            if leap { 29 } else { 28 }
        }
    }
}

/// A time of day as hours since midnight: `HH:MM`, `HH:MM:SS`, or a plain
/// number of hours (`13.5`).
pub fn parse_time(s: &str) -> Result<f64, String> {
    let s = s.trim();
    if let Ok(h) = s.parse::<f64>() {
        if !h.is_finite() || !(0.0..24.0).contains(&h) {
            return Err(format!("{s:?} is not a time of day (0 to 24 hours)"));
        }
        return Ok(h);
    }
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return Err(format!("{s:?} is not a time of the form HH:MM"));
    }
    let h: f64 = parts[0].parse().map_err(|_| format!("{s:?}: {:?} is not an hour", parts[0]))?;
    let m: f64 = parts[1].parse().map_err(|_| format!("{s:?}: {:?} is not a minute", parts[1]))?;
    let sec: f64 = if parts.len() == 3 { parts[2].parse().map_err(|_| format!("{s:?}: {:?} is not a second", parts[2]))? } else { 0.0 };
    if !(0.0..24.0).contains(&h) || !(0.0..60.0).contains(&m) || !(0.0..60.0).contains(&sec) {
        return Err(format!("{s:?} is not a time of day"));
    }
    Ok(h + m / 60.0 + sec / 3600.0)
}

/// Hours since midnight as `HH:MM`, to the nearest minute.
pub fn format_time(hours: f64) -> String {
    let total = (hours * 60.0).round().rem_euclid(1440.0) as u32;
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// Where the observer stands: latitude and longitude in degrees (north and
/// east positive), and the clock's offset from UTC in hours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Site {
    pub latitude: f64,
    pub longitude: f64,
    pub tz_hours: f64,
}

/// The standard-time zone a longitude falls in, as an offset from UTC in
/// hours: the meridian nearest the site, with no daylight saving. What a
/// site takes when nothing says otherwise.
pub fn default_tz(longitude: f64) -> f64 {
    (longitude / 15.0).round()
}

/// The sun over a site at one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunPosition {
    /// Compass azimuth in degrees: 0 north, 90 east, 180 south, 270 west.
    pub azimuth: f64,
    /// Apparent elevation above the horizon in degrees, refraction included.
    pub elevation: f64,
    /// Geometric elevation of the sun's centre, no refraction.
    pub elevation_geometric: f64,
    /// Declination in degrees.
    pub declination: f64,
    /// Equation of time in minutes (apparent minus mean solar time).
    pub equation_of_time: f64,
    /// Hour angle in degrees, negative before solar noon.
    pub hour_angle: f64,
    /// Solar noon, hours since midnight, local clock.
    pub solar_noon: f64,
    /// Sunrise and sunset, hours since midnight, local clock; `None` on a
    /// day the sun never rises or never sets at this latitude.
    pub sunrise: Option<f64>,
    pub sunset: Option<f64>,
}

impl SunPosition {
    /// The sun's centre is above the horizon.
    pub fn is_up(&self) -> bool {
        self.elevation_geometric > 0.0
    }
}

fn rad(d: f64) -> f64 {
    d.to_radians()
}

fn deg(r: f64) -> f64 {
    r.to_degrees()
}

/// The solar quantities that depend only on the instant: declination,
/// equation of time (minutes), and the geometric mean longitude (for the
/// tests). `jd` is the Julian day in UT.
fn solar_at(jd: f64) -> (f64, f64) {
    let jc = (jd - 2451545.0) / 36525.0;
    let gmls = (280.46646 + jc * (36000.76983 + jc * 0.0003032)).rem_euclid(360.0);
    let gmas = 357.52911 + jc * (35999.05029 - 0.0001537 * jc);
    let eeo = 0.016708634 - jc * (0.000042037 + 0.0000001267 * jc);
    let sec = fmath::sin(rad(gmas)) * (1.914602 - jc * (0.004817 + 0.000014 * jc))
        + fmath::sin(rad(2.0 * gmas)) * (0.019993 - 0.000101 * jc)
        + fmath::sin(rad(3.0 * gmas)) * 0.000289;
    let stl = gmls + sec;
    let omega = 125.04 - 1934.136 * jc;
    let sal = stl - 0.00569 - 0.00478 * fmath::sin(rad(omega));
    let moe = 23.0 + (26.0 + (21.448 - jc * (46.815 + jc * (0.00059 - jc * 0.001813))) / 60.0) / 60.0;
    let oc = moe + 0.00256 * fmath::cos(rad(omega));
    let declination = deg(fmath::asin(fmath::sin(rad(oc)) * fmath::sin(rad(sal))));
    let var_y = {
        let t = fmath::tan(rad(oc / 2.0));
        t * t
    };
    let eot = 4.0
        * deg(var_y * fmath::sin(2.0 * rad(gmls)) - 2.0 * eeo * fmath::sin(rad(gmas))
            + 4.0 * eeo * var_y * fmath::sin(rad(gmas)) * fmath::cos(2.0 * rad(gmls))
            - 0.5 * var_y * var_y * fmath::sin(4.0 * rad(gmls))
            - 1.25 * eeo * eeo * fmath::sin(2.0 * rad(gmas)));
    (declination, eot)
}

/// NOAA's refraction correction, degrees, for a geometric elevation.
fn refraction(elev: f64) -> f64 {
    if elev > 85.0 {
        0.0
    } else if elev > 5.0 {
        let t = fmath::tan(rad(elev));
        (58.1 / t - 0.07 / (t * t * t) + 0.000086 / (t * t * t * t * t)) / 3600.0
    } else if elev > -0.575 {
        (1735.0 + elev * (-518.2 + elev * (103.4 + elev * (-12.79 + elev * 0.711)))) / 3600.0
    } else {
        -20.772 / fmath::tan(rad(elev)) / 3600.0
    }
}

/// The sun over `site` at `hours` since midnight, local clock, on `date`.
pub fn position(site: &Site, date: Date, hours: f64) -> SunPosition {
    let lat = site.latitude;
    let lon = site.longitude;
    let tz = site.tz_hours;
    let jd = date.julian_day() + hours / 24.0 - tz / 24.0;
    let (declination, eot) = solar_at(jd);
    // Sunrise, sunset and noon are taken at the day's noon, as the
    // spreadsheet does; they move by seconds over a day.
    let (decl_noon, eot_noon) = solar_at(date.julian_day() + 0.5 - tz / 24.0);
    let solar_noon = (720.0 - 4.0 * lon - eot_noon + tz * 60.0) / 60.0;
    let cos_ha = fmath::cos(rad(90.833)) / (fmath::cos(rad(lat)) * fmath::cos(rad(decl_noon)))
        - fmath::tan(rad(lat)) * fmath::tan(rad(decl_noon));
    let (sunrise, sunset) = if cos_ha.abs() <= 1.0 {
        let ha = deg(fmath::acos(cos_ha));
        (Some(solar_noon - ha * 4.0 / 60.0), Some(solar_noon + ha * 4.0 / 60.0))
    } else {
        (None, None)
    };
    let tst = (hours * 60.0 + eot + 4.0 * lon - 60.0 * tz).rem_euclid(1440.0);
    let hour_angle = if tst / 4.0 < 0.0 { tst / 4.0 + 180.0 } else { tst / 4.0 - 180.0 };
    let cos_zenith = fmath::sin(rad(lat)) * fmath::sin(rad(declination))
        + fmath::cos(rad(lat)) * fmath::cos(rad(declination)) * fmath::cos(rad(hour_angle));
    let zenith = deg(fmath::acos(cos_zenith.clamp(-1.0, 1.0)));
    let elevation_geometric = 90.0 - zenith;
    let elevation = elevation_geometric + refraction(elevation_geometric);
    let sin_zenith = fmath::sin(rad(zenith));
    let azimuth = if sin_zenith.abs() < 1e-9 {
        // The sun at the zenith: the azimuth is undefined; south is the
        // conventional answer.
        180.0
    } else {
        let c = ((fmath::sin(rad(lat)) * fmath::cos(rad(zenith))) - fmath::sin(rad(declination)))
            / (fmath::cos(rad(lat)) * sin_zenith);
        let a = deg(fmath::acos(c.clamp(-1.0, 1.0)));
        if hour_angle > 0.0 { (a + 180.0).rem_euclid(360.0) } else { (540.0 - a).rem_euclid(360.0) }
    };
    SunPosition {
        azimuth,
        elevation,
        elevation_geometric,
        declination,
        equation_of_time: eot,
        hour_angle,
        solar_noon,
        sunrise,
        sunset,
    }
}

/// The unit vector toward a point of the sky at compass azimuth `az` and
/// elevation `el` (degrees), in the frame `+x` east, `+y` north, `+z` up.
pub fn direction(az: f64, el: f64) -> Vec3 {
    let (sa, ca) = fmath::sin_cos(rad(az));
    let (se, ce) = fmath::sin_cos(rad(el));
    Vec3::new(sa * ce, ca * ce, se).normalized()
}

/// A compass-frame vector (`+y` north) in a model whose north lies
/// `north_degrees` clockwise from `+y` when seen from above: `north=0`
/// keeps `+y`, `north=90` puts north along `+x`.
pub fn compass_to_model(v: Vec3, north_degrees: f64) -> Vec3 {
    if north_degrees == 0.0 {
        return v;
    }
    let (s, c) = fmath::sin_cos(rad(north_degrees));
    let north = Vec3::new(s, c, 0.0);
    let east = Vec3::new(c, -s, 0.0);
    east * v.x + north * v.y + Vec3::Z * v.z
}

/// The compass azimuth (0 north, 90 east) and elevation of a model-frame
/// direction, given where north is (the inverse of [`compass_to_model`]
/// after [`direction`]).
pub fn azimuth_elevation(v: Vec3, north_degrees: f64) -> (f64, f64) {
    let v = v.normalized();
    let (s, c) = fmath::sin_cos(rad(north_degrees));
    let north = Vec3::new(s, c, 0.0);
    let east = Vec3::new(c, -s, 0.0);
    let (e, n, u) = (v.dot(east), v.dot(north), v.z);
    let az = deg(fmath::atan2(e, n)).rem_euclid(360.0);
    let el = deg(fmath::asin(u.clamp(-1.0, 1.0)));
    (az, el)
}

/// The Julian ephemeris days of the March equinox, the June solstice, the
/// September equinox and the December solstice of `year` (Meeus ch. 27,
/// the 2000–2100 series with the 24 periodic terms), good to a minute.
pub fn season_instants(year: i32) -> [f64; 4] {
    let y = (year as f64 - 2000.0) / 1000.0;
    let base = [
        2451623.80984 + y * (365242.37404 + y * (0.05169 + y * (-0.00411 + y * -0.00057))),
        2451716.56767 + y * (365241.62603 + y * (0.00325 + y * (0.00888 + y * -0.00030))),
        2451810.21715 + y * (365242.01767 + y * (-0.11575 + y * (0.00337 + y * 0.00078))),
        2451900.05952 + y * (365242.74049 + y * (-0.06223 + y * (-0.00823 + y * 0.00032))),
    ];
    const TERMS: [(f64, f64, f64); 24] = [
        (485.0, 324.96, 1934.136),
        (203.0, 337.23, 32964.467),
        (199.0, 342.08, 20.186),
        (182.0, 27.85, 445267.112),
        (156.0, 73.14, 45036.886),
        (136.0, 171.52, 22518.443),
        (77.0, 222.54, 65928.934),
        (74.0, 296.72, 3034.906),
        (70.0, 243.58, 9037.513),
        (58.0, 119.81, 33718.147),
        (52.0, 297.17, 150.678),
        (50.0, 21.02, 2281.226),
        (45.0, 247.54, 29929.562),
        (44.0, 325.15, 31555.956),
        (29.0, 60.93, 4443.417),
        (18.0, 155.12, 67555.328),
        (17.0, 288.79, 4562.452),
        (16.0, 198.04, 62894.029),
        (14.0, 199.76, 31436.921),
        (12.0, 95.39, 14577.848),
        (12.0, 287.11, 31931.756),
        (12.0, 320.81, 34777.259),
        (9.0, 227.73, 1222.114),
        (8.0, 15.45, 16859.074),
    ];
    base.map(|jde0| {
        let t = (jde0 - 2451545.0) / 36525.0;
        let w = rad(35999.373 * t - 2.47);
        let dl = 1.0 + 0.0334 * fmath::cos(w) + 0.0007 * fmath::cos(2.0 * w);
        let s: f64 = TERMS.iter().map(|(a, b, c)| a * fmath::cos(rad(b + c * t))).sum();
        jde0 + 0.00001 * s / dl
    })
}

/// The dates (UTC) of the June and December solstices of `year`.
pub fn solstices(year: i32) -> [Date; 2] {
    let s = season_instants(year);
    [Date::from_julian(s[1]).0, Date::from_julian(s[3]).0]
}

/// The dates (UTC) of the March and September equinoxes of `year`.
pub fn equinoxes(year: i32) -> [Date; 2] {
    let s = season_instants(year);
    [Date::from_julian(s[0]).0, Date::from_julian(s[2]).0]
}

/// A sun a render or a study can use: where it is in the sky, and what it
/// was made from, for a label.
#[derive(Clone, Debug, PartialEq)]
pub struct SunSpec {
    /// Compass azimuth, degrees.
    pub azimuth: f64,
    /// Apparent elevation, degrees.
    pub elevation: f64,
    /// `2026-06-21 12:00` for a computed sun, `az 210 el 45` for a given one.
    pub label: String,
}

impl SunSpec {
    pub fn angles(azimuth: f64, elevation: f64) -> SunSpec {
        SunSpec { azimuth, elevation, label: format!("az {} el {}", trim_num(azimuth), trim_num(elevation)) }
    }

    /// The sun over `site` on `date` at `hours` (local clock).
    pub fn at(site: &Site, date: Date, hours: f64) -> SunSpec {
        let p = position(site, date, hours);
        SunSpec { azimuth: p.azimuth, elevation: p.elevation, label: format!("{date} {}", format_time(hours)) }
    }

    /// The unit vector toward the sun in a model whose north is
    /// `north_degrees` clockwise from `+y`.
    pub fn direction(&self, north_degrees: f64) -> Vec3 {
        compass_to_model(direction(self.azimuth, self.elevation), north_degrees)
    }

    /// Above the horizon.
    pub fn is_up(&self) -> bool {
        self.elevation > 0.0
    }
}

fn trim_num(v: f64) -> String {
    let s = format!("{v:.1}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Parse the one-line sun a request or a flag carries:
/// `lat,lon,YYYY-MM-DD,HH:MM[,tz]` (a site, a date and a local time, the
/// zone defaulting to the longitude's standard zone) or `az=<deg>,el=<deg>`
/// (a sun placed by hand). Empty is no sun.
pub fn parse_spec(s: &str) -> Result<Option<SunSpec>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    if s.contains('=') {
        let (mut az, mut el) = (None, None);
        for part in s.split(',') {
            let (k, v) = part.split_once('=').ok_or_else(|| format!("sun {s:?}: expected az=<deg>,el=<deg>"))?;
            let v: f64 = v.trim().parse().map_err(|_| format!("sun {s:?}: {:?} is not a number", v.trim()))?;
            match k.trim() {
                "az" | "azimuth" => az = Some(v),
                "el" | "elev" | "elevation" => el = Some(v),
                other => return Err(format!("sun {s:?}: unknown field {other:?}; expected az= and el=")),
            }
        }
        let (Some(az), Some(el)) = (az, el) else {
            return Err(format!("sun {s:?}: both az= and el= are needed"));
        };
        if !az.is_finite() || !el.is_finite() || !(-90.0..=90.0).contains(&el) {
            return Err(format!("sun {s:?}: el must be between -90 and 90"));
        }
        return Ok(Some(SunSpec::angles(az.rem_euclid(360.0), el)));
    }
    let (site, date, hours) = parse_site_spec(s)?.expect("a non-empty spec without '=' is the site form");
    Ok(Some(SunSpec::at(&site, date, hours)))
}

/// The site form of the one-line sun, `lat,lon,YYYY-MM-DD,HH:MM[,tz]`, as
/// the site, the date and the local time in hours; `Ok(None)` for an empty
/// string or the `az=,el=` form, which names no site.
pub fn parse_site_spec(s: &str) -> Result<Option<(Site, Date, f64)>, String> {
    let s = s.trim();
    if s.is_empty() || s.contains('=') {
        return Ok(None);
    }
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    if parts.len() != 4 && parts.len() != 5 {
        return Err(format!(
            "sun {s:?}: expected lat,lon,YYYY-MM-DD,HH:MM[,tz] (a site, a date and a local time) or az=<deg>,el=<deg>"
        ));
    }
    let lat: f64 = parts[0].parse().map_err(|_| format!("sun {s:?}: {:?} is not a latitude", parts[0]))?;
    let lon: f64 = parts[1].parse().map_err(|_| format!("sun {s:?}: {:?} is not a longitude", parts[1]))?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err(format!("sun {s:?}: latitude must be within ±90 and longitude within ±180"));
    }
    let date = Date::parse(parts[2]).map_err(|e| format!("sun {s:?}: {e}"))?;
    let hours = parse_time(parts[3]).map_err(|e| format!("sun {s:?}: {e}"))?;
    let tz = if parts.len() == 5 {
        let t: f64 = parts[4].parse().map_err(|_| format!("sun {s:?}: {:?} is not a UTC offset in hours", parts[4]))?;
        if !(-14.0..=14.0).contains(&t) {
            return Err(format!("sun {s:?}: a UTC offset is between -14 and +14 hours"));
        }
        t
    } else {
        default_tz(lon)
    };
    Ok(Some((Site { latitude: lat, longitude: lon, tz_hours: tz }, date, hours)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: Site = Site { latitude: 38.0, longitude: -121.0, tz_hours: -8.0 };

    /// Dates round-trip through the Julian day, and the epoch is J2000.
    #[test]
    fn julian_days_and_calendar_dates_agree() {
        assert_eq!(Date::new(2000, 1, 1).unwrap().julian_day(), 2451544.5);
        for (y, m, d) in [(2026, 9, 26), (2024, 2, 29), (1999, 12, 31), (2100, 3, 1), (2000, 3, 1)] {
            let date = Date::new(y, m, d).unwrap();
            let (back, f) = Date::from_julian(date.julian_day() + 0.25);
            assert_eq!((back, f), (date, 0.25), "{date}");
        }
        assert!(Date::new(2023, 2, 29).is_err());
        assert!(Date::parse("2024-13-01").is_err());
        assert_eq!(Date::parse("2024-06-20").unwrap(), Date { year: 2024, month: 6, day: 20 });
        assert_eq!(format_time(parse_time("09:30").unwrap()), "09:30");
        assert_eq!(parse_time("13.5").unwrap(), 13.5);
        assert!(parse_time("25:00").is_err());
    }

    /// The seasons, against the USNO's published instants (UTC) for 2024,
    /// 2025 and 2026, to ten minutes.
    #[test]
    fn solstices_and_equinoxes_match_the_almanac() {
        let want: [(i32, [(u32, u32, f64); 4]); 3] = [
            (2024, [(3, 20, 3.0 + 6.0 / 60.0), (6, 20, 20.0 + 51.0 / 60.0), (9, 22, 12.0 + 44.0 / 60.0), (12, 21, 9.0 + 20.0 / 60.0)]),
            (2025, [(3, 20, 9.0 + 1.0 / 60.0), (6, 21, 2.0 + 42.0 / 60.0), (9, 22, 18.0 + 19.0 / 60.0), (12, 21, 15.0 + 3.0 / 60.0)]),
            (2026, [(3, 20, 14.0 + 46.0 / 60.0), (6, 21, 8.0 + 24.0 / 60.0), (9, 23, 0.0 + 5.0 / 60.0), (12, 21, 20.0 + 50.0 / 60.0)]),
        ];
        for (year, events) in want {
            let got = season_instants(year);
            for (i, (m, d, h)) in events.iter().enumerate() {
                let (date, f) = Date::from_julian(got[i]);
                assert_eq!((date.month, date.day), (*m, *d), "{year} event {i}: {date}");
                let minutes = (f * 24.0 - h).abs() * 60.0;
                assert!(minutes < 10.0, "{year} event {i}: {date} {:.2}h vs {h:.2}h ({minutes:.1} min off)", f * 24.0);
            }
        }
        assert_eq!(solstices(2026).map(|d| d.to_string()), ["2026-06-21", "2026-12-21"]);
        assert_eq!(equinoxes(2026).map(|d| d.to_string()), ["2026-03-20", "2026-09-23"]);
    }

    /// The geometry every almanac agrees on: at solar noon on an equinox the
    /// sun stands `90 − |lat|` above the horizon, due south from the north;
    /// at the June solstice `90 − lat + 23.44`; at the December solstice
    /// `90 − lat − 23.44`.
    #[test]
    fn noon_elevation_follows_the_declination() {
        for (site, name) in [(SITE, "38 N"), (Site { latitude: 51.4769, longitude: 0.0, tz_hours: 0.0 }, "Greenwich")] {
            let cases = [
                ("2026-03-20", 0.0, 0.5),
                ("2026-09-23", 0.0, 0.5),
                ("2026-06-21", 23.44, 0.1),
                ("2026-12-21", -23.44, 0.1),
            ];
            for (date, decl, tol) in cases {
                let date = Date::parse(date).unwrap();
                let noon = position(&site, date, 12.0).solar_noon;
                let p = position(&site, date, noon);
                let want = 90.0 - site.latitude + decl;
                assert!(
                    (p.elevation_geometric - want).abs() < tol,
                    "{name} {date}: noon elevation {:.3} vs {want:.3}",
                    p.elevation_geometric
                );
                assert!((p.azimuth - 180.0).abs() < 0.5, "{name} {date}: noon azimuth {:.3}", p.azimuth);
                assert!(p.hour_angle.abs() < 0.2, "{name} {date}: hour angle at noon {:.3}", p.hour_angle);
                assert!((p.declination - decl).abs() < 0.5, "{name} {date}: declination {:.3} vs {decl}", p.declination);
            }
        }
    }

    /// The equation of time reaches its textbook extrema: about −14.2 min
    /// near 11 February and +16.4 min near 3 November, and crosses zero in
    /// mid April, mid June, the start of September and Christmas.
    #[test]
    fn the_equation_of_time_has_its_known_shape() {
        let eot = |m, d| position(&SITE, Date::new(2026, m, d).unwrap(), 12.0).equation_of_time;
        assert!((eot(2, 11) + 14.2).abs() < 0.4, "Feb 11: {}", eot(2, 11));
        assert!((eot(11, 3) - 16.4).abs() < 0.4, "Nov 3: {}", eot(11, 3));
        assert!((eot(5, 14) - 3.7).abs() < 0.4, "May 14: {}", eot(5, 14));
        assert!((eot(7, 26) + 6.5).abs() < 0.4, "Jul 26: {}", eot(7, 26));
        for (m, d) in [(4, 15), (6, 13), (9, 1), (12, 25)] {
            assert!(eot(m, d).abs() < 1.0, "{m}-{d}: {}", eot(m, d));
        }
    }

    /// Sunrise and sunset on the equinox are twelve hours apart, symmetric
    /// about solar noon, and shifted by the refraction convention (the day
    /// is a few minutes longer than twelve hours); the summer day at 38 N
    /// is about 14 h 48 min and the winter day 9 h 32 min, which
    /// is what the almanac lists.
    #[test]
    fn sunrise_and_sunset_have_the_right_day_length() {
        let day = |d: &str| {
            let p = position(&SITE, Date::parse(d).unwrap(), 12.0);
            (p.sunrise.unwrap(), p.sunset.unwrap(), p.solar_noon)
        };
        let (r, s, noon) = day("2026-03-20");
        assert!((s - r - 12.1).abs() < 0.1, "equinox day {:.3} h", s - r);
        assert!(((r + s) / 2.0 - noon).abs() < 1e-6);
        let (r, s, _) = day("2026-06-21");
        assert!((s - r - 14.78).abs() < 0.1, "summer day {:.3} h", s - r);
        let (r, s, _) = day("2026-12-21");
        assert!((s - r - 9.55).abs() < 0.1, "winter day {:.3} h", s - r);
        // Above the Arctic Circle in June the sun never sets.
        let p = position(&Site { latitude: 78.0, longitude: 15.0, tz_hours: 1.0 }, Date::parse("2026-06-21").unwrap(), 12.0);
        assert!(p.sunrise.is_none() && p.sunset.is_none());
        assert!(position(&Site { latitude: 78.0, longitude: 15.0, tz_hours: 1.0 }, Date::parse("2026-06-21").unwrap(), 0.0).is_up());
        // At sunrise the apparent elevation is at the horizon: the geometric
        // centre 0.833° below, refraction lifting it most of the way back.
        // The sunrise time is figured from the day's noon declination, as
        // NOAA's is, so the centre is within a few hundredths of a degree
        // (twenty seconds of time) of the convention.
        let (r, _, _) = day("2026-03-20");
        let p = position(&SITE, Date::parse("2026-03-20").unwrap(), r);
        assert!((p.elevation_geometric + 0.833).abs() < 0.15, "geometric elevation at sunrise {:.3}", p.elevation_geometric);
        // NOAA's refraction is piecewise and coarsest right at the horizon
        // (the branch below −0.575° lifts a centre at −0.9° by 0.37°, the
        // one above it by 0.47°), so the apparent elevation at sunrise is
        // only within half a degree of zero: the contract's figure.
        assert!(p.elevation.abs() < 0.6, "apparent elevation at sunrise {:.3}", p.elevation);
        assert!((p.azimuth - 90.0).abs() < 2.0, "the equinox sun rises due east: {:.2}", p.azimuth);
    }

    /// An independent evaluation: the Astronomical Almanac's low-precision
    /// formulae for the sun (page C24; 0.01° for 1950–2050) with the hour
    /// angle from Greenwich sidereal time rather than the equation of time.
    /// Two derivations that share no code agree to 0.05° in azimuth and
    /// geometric elevation over a grid of dates, hours and sites, which is
    /// the check that says the transcription above is right.
    #[test]
    fn agrees_with_the_astronomical_almanac_to_a_twentieth_of_a_degree() {
        fn almanac(site: &Site, jd_ut: f64) -> (f64, f64) {
            use fmath::{asin, atan2, cos, sin};
            let n = jd_ut - 2451545.0;
            let l = (280.460 + 0.9856474 * n).rem_euclid(360.0);
            let g = rad((357.528 + 0.9856003 * n).rem_euclid(360.0));
            let lambda = rad(l + 1.915 * sin(g) + 0.020 * sin(2.0 * g));
            let eps = rad(23.439 - 0.0000004 * n);
            let ra = atan2(cos(eps) * sin(lambda), cos(lambda));
            let dec = asin(sin(eps) * sin(lambda));
            let gmst_h = (18.697374558 + 24.06570982441908 * n).rem_euclid(24.0);
            let lst = rad(gmst_h * 15.0 + site.longitude);
            let ha = lst - ra;
            let lat = rad(site.latitude);
            let sin_alt = sin(lat) * sin(dec) + cos(lat) * cos(dec) * cos(ha);
            let alt = asin(sin_alt);
            // Azimuth from north, clockwise.
            let az = atan2(-sin(ha) * cos(dec), sin(dec) * cos(lat) - cos(dec) * sin(lat) * cos(ha));
            (deg(az).rem_euclid(360.0), deg(alt))
        }
        let sites = [
            SITE,
            Site { latitude: 51.4769, longitude: 0.0, tz_hours: 0.0 },
            Site { latitude: -33.87, longitude: 151.21, tz_hours: 10.0 },
            Site { latitude: 64.14, longitude: -21.94, tz_hours: 0.0 },
            Site { latitude: 1.35, longitude: 103.82, tz_hours: 8.0 },
        ];
        let dates = ["2024-01-15", "2024-06-20", "2026-03-20", "2026-09-26", "2030-11-03", "2040-07-04"];
        let mut worst = 0.0f64;
        let mut count = 0;
        for site in &sites {
            for d in dates {
                let date = Date::parse(d).unwrap();
                for h in [6.5, 9.0, 12.0, 15.25, 18.0] {
                    let p = position(site, date, h);
                    if p.elevation_geometric < 3.0 {
                        continue;
                    }
                    let jd = date.julian_day() + h / 24.0 - site.tz_hours / 24.0;
                    let (az, alt) = almanac(site, jd);
                    let daz = ((p.azimuth - az + 180.0).rem_euclid(360.0) - 180.0).abs();
                    let dalt = (p.elevation_geometric - alt).abs();
                    worst = worst.max(daz).max(dalt);
                    count += 1;
                    assert!(daz < 0.05 && dalt < 0.05, "{d} {h}h at {site:?}: NOAA az {:.3} el {:.3}, almanac az {az:.3} el {alt:.3}", p.azimuth, p.elevation_geometric);
                }
            }
        }
        assert!(count > 100, "{count} comparisons");
        assert!(worst < 0.05, "worst disagreement {worst:.4}°");
    }

    /// The direction vector's frame: `+y` north, `+x` east, `+z` up; a
    /// model whose north is turned takes the same sun turned with it, and
    /// the angles read back.
    #[test]
    fn directions_and_the_north_rotation() {
        let close = |a: Vec3, b: Vec3| (a - b).length() < 1e-9;
        assert!(close(direction(0.0, 0.0), Vec3::Y));
        assert!(close(direction(90.0, 0.0), Vec3::X));
        assert!(close(direction(180.0, 0.0), -Vec3::Y));
        assert!(close(direction(0.0, 90.0), Vec3::Z));
        let noon_south_45 = direction(180.0, 45.0);
        assert!(close(noon_south_45, Vec3::new(0.0, -0.5f64.sqrt(), 0.5f64.sqrt())));
        // North along +x (turned 90° clockwise): south is −x.
        assert!(close(compass_to_model(direction(180.0, 0.0), 90.0), -Vec3::X));
        assert!(close(compass_to_model(direction(90.0, 0.0), 90.0), -Vec3::Y));
        assert!(close(compass_to_model(Vec3::Z, 37.0), Vec3::Z));
        for north in [0.0, 30.0, 90.0, 200.0] {
            for (az, el) in [(0.0, 10.0), (135.0, 45.0), (270.0, 5.0), (359.0, 80.0)] {
                let v = compass_to_model(direction(az, el), north);
                let (a, e) = azimuth_elevation(v, north);
                let daz = ((a - az + 180.0).rem_euclid(360.0) - 180.0).abs();
                assert!(daz < 1e-9 && (e - el).abs() < 1e-9, "north {north}: ({az}, {el}) -> ({a}, {e})");
            }
        }
    }

    /// The one-line spec: both forms, the default zone, and the refusals.
    #[test]
    fn the_spec_parses_both_forms() {
        assert_eq!(parse_spec("").unwrap(), None);
        let s = parse_spec("az=210, el=45").unwrap().unwrap();
        assert_eq!((s.azimuth, s.elevation, s.label.as_str()), (210.0, 45.0, "az 210 el 45"));
        let s = parse_spec("38,-121,2026-06-21,12:00,-7").unwrap().unwrap();
        assert_eq!(s.label, "2026-06-21 12:00");
        let p = position(&Site { latitude: 38.0, longitude: -121.0, tz_hours: -7.0 }, Date::parse("2026-06-21").unwrap(), 12.0);
        assert_eq!((s.azimuth, s.elevation), (p.azimuth, p.elevation));
        // No zone: the longitude's standard zone, −8 at 121 W.
        let s = parse_spec("38,-121,2026-06-21,13:00").unwrap().unwrap();
        let p = position(&SITE, Date::parse("2026-06-21").unwrap(), 13.0);
        assert_eq!((s.azimuth, s.elevation), (p.azimuth, p.elevation));
        assert_eq!(default_tz(-121.0), -8.0);
        assert_eq!(default_tz(151.21), 10.0);
        for bad in ["az=10", "el=91,az=0", "38,-121", "91,0,2026-01-01,12:00", "37,0,2026-02-30,12:00", "37,0,2026-01-01,25:00", "37,0,2026-01-01,12:00,20"] {
            assert!(parse_spec(bad).is_err(), "{bad}");
        }
        assert!(s.direction(0.0).z > 0.9, "the midsummer noon sun is high: {:?}", s.direction(0.0));
    }
}
