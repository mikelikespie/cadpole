//! The sun: `sun(lat, lon, date, time, tz=)` and `sun(az=, el=)`,
//! the `Sun` value they return, `solstices(year)` and `equinoxes(year)`,
//! and `render_defaults(sun=, north=, camera=)`, the document's own render
//! settings. The astronomy is `cadpole_core::sun`'s (NOAA's algorithm, good to
//! half a degree); this is its Starlark surface (docs/buildings.md).
//!
//! Nothing here is a feature: a `Sun` is a value like a number, and
//! `render_defaults` records a declaration the engine reads at render time,
//! as `animation()` does. A script that wants to orient a model by
//! the sun reads `.direction` and does its own arithmetic.

use allocative::Allocative;
use cadpole_core::context::{CameraSpec, RenderDefaults, fmt_num};
use cadpole_core::sun::{self, Date, Site, SunPosition, SunSpec};
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::starlark_simple_value;
use starlark::values::float::UnpackFloat;
use starlark::values::none::NoneType;
use starlark::values::{Heap, NoSerialize, ProvidesStaticType, StarlarkPagablePanic, StarlarkValue, UnpackValue, Value, ValueLike};
use starlark_derive::starlark_value;

use crate::animation::KView;
use crate::check::{num, opt_num};
use crate::convert::alloc_vec3;
use crate::globals::int_arg;
use crate::store::store;

/// What `sun(...)` returns.
#[derive(Debug, Clone, ProvidesStaticType, NoSerialize, StarlarkPagablePanic, Allocative)]
pub struct KSun {
    #[allocative(skip)]
    pub spec: SunSpec,
    /// The full position when the sun was computed for a place and a time;
    /// `None` for a sun placed by `az=`/`el=`.
    #[allocative(skip)]
    pub position: Option<SunPosition>,
    #[allocative(skip)]
    pub site: Option<Site>,
    #[allocative(skip)]
    pub date: Option<Date>,
    pub hours: Option<f64>,
}
starlark_simple_value!(KSun);

impl std::fmt::Display for KSun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sun({}: az {}, el {})", self.spec.label, fmt_num(round1(self.spec.azimuth)), fmt_num(round1(self.spec.elevation)))
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn opt_time<'v>(heap: Heap<'v>, h: Option<f64>) -> Value<'v> {
    match h {
        Some(h) => heap.alloc(sun::format_time(h)),
        None => Value::new_none(),
    }
}

#[starlark_value(type = "Sun")]
impl<'v> StarlarkValue<'v> for KSun {
    fn get_attr(&self, attribute: &str, heap: Heap<'v>) -> Option<Value<'v>> {
        let p = self.position.as_ref();
        Some(match attribute {
            "azimuth" => heap.alloc(self.spec.azimuth),
            "elevation" => heap.alloc(self.spec.elevation),
            "direction" => alloc_vec3(heap, sun::direction(self.spec.azimuth, self.spec.elevation)),
            "up" => heap.alloc(self.spec.elevation > 0.0),
            "label" => heap.alloc(self.spec.label.as_str()),
            "sunrise" => opt_time(heap, p.and_then(|p| p.sunrise)),
            "sunset" => opt_time(heap, p.and_then(|p| p.sunset)),
            "solar_noon" => opt_time(heap, p.map(|p| p.solar_noon)),
            "declination" => match p {
                Some(p) => heap.alloc(p.declination),
                None => Value::new_none(),
            },
            "equation_of_time" => match p {
                Some(p) => heap.alloc(p.equation_of_time),
                None => Value::new_none(),
            },
            "date" => match self.date {
                Some(d) => heap.alloc(d.to_string()),
                None => Value::new_none(),
            },
            "time" => opt_time(heap, self.hours),
            "lat" => match self.site {
                Some(s) => heap.alloc(s.latitude),
                None => Value::new_none(),
            },
            "lon" => match self.site {
                Some(s) => heap.alloc(s.longitude),
                None => Value::new_none(),
            },
            "tz" => match self.site {
                Some(s) => heap.alloc(s.tz_hours),
                None => Value::new_none(),
            },
            _ => return None,
        })
    }
    fn dir_attr(&self) -> Vec<String> {
        [
            "azimuth", "elevation", "direction", "up", "label", "sunrise", "sunset", "solar_noon", "declination",
            "equation_of_time", "date", "time", "lat", "lon", "tz",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }
}

/// A time of day from a script: `"HH:MM"`, or a number of hours.
pub(crate) fn hours_of<'v>(v: Value<'v>, what: &str) -> anyhow::Result<f64> {
    if let Some(s) = v.unpack_str() {
        return sun::parse_time(s).map_err(|e| anyhow::anyhow!("{what}: {e}"));
    }
    if let Ok(Some(n)) = UnpackFloat::unpack_value(v) {
        let h = crate::check::finite(n.0, what)?;
        if !(0.0..24.0).contains(&h) {
            anyhow::bail!("{what}: a time of day is between 0 and 24 hours, got {}", fmt_num(h));
        }
        return Ok(h);
    }
    anyhow::bail!("{what}: expected \"HH:MM\" or a number of hours, got {}", v.get_type())
}

/// `sun(lat, lon, date, time, tz=)` as a value, for the other builtins
/// (`shadow_study(dates=, times=)`) that compute suns of their own.
pub(crate) fn sun_at(site: Site, date: Date, hours: f64) -> KSun {
    let position = sun::position(&site, date, hours);
    let spec = SunSpec::at(&site, date, hours);
    KSun { spec, position: Some(position), site: Some(site), date: Some(date), hours: Some(hours) }
}

pub(crate) fn site_of(lat: f64, lon: f64, tz: Option<f64>, what: &str) -> anyhow::Result<Site> {
    if !(-90.0..=90.0).contains(&lat) {
        anyhow::bail!("{what}: lat must be between -90 and 90 degrees, got {}", fmt_num(lat));
    }
    if !(-180.0..=180.0).contains(&lon) {
        anyhow::bail!("{what}: lon must be between -180 and 180 degrees, got {}", fmt_num(lon));
    }
    let tz = tz.unwrap_or_else(|| sun::default_tz(lon));
    if !(-14.0..=14.0).contains(&tz) {
        anyhow::bail!("{what}: tz is an offset from UTC in hours, between -14 and 14, got {}", fmt_num(tz));
    }
    Ok(Site { latitude: lat, longitude: lon, tz_hours: tz })
}

pub(crate) fn date_of(s: &str, what: &str) -> anyhow::Result<Date> {
    Date::parse(s).map_err(|e| anyhow::anyhow!("{what}: {e}"))
}

#[starlark_module]
pub(crate) fn sun_globals(builder: &mut GlobalsBuilder) {
    /// Where the sun stands: `sun(lat, lon, date, time, tz=)` computes it
    /// for a place (degrees, north and east positive), a date (`"YYYY-MM-DD"`)
    /// and a local clock time (`"HH:MM"`, or hours as a number), `tz=`
    /// being the clock's offset from UTC in hours (the longitude's standard
    /// zone when left out, with no daylight saving: longitude -121 is -8, or
    /// -7 in summer). `sun(az=, el=)` places one by hand, by compass
    /// azimuth (0 north, 90 east) and elevation. The answer is a `Sun`:
    /// `.azimuth`, `.elevation` (apparent, refraction included), `.direction`
    /// (a unit vector toward the sun with +y north, +x east, +z up), `.up`,
    /// `.sunrise`, `.sunset` and `.solar_noon` (local clock, `"HH:MM"`),
    /// `.declination`, `.equation_of_time` (minutes), and what it was made
    /// from (`.date`, `.time`, `.lat`, `.lon`, `.tz`, `.label`). NOAA's
    /// algorithm, good to half a degree; docs/buildings.md.
    fn sun<'v>(
        #[starlark(require = pos)] lat: Option<UnpackFloat>,
        #[starlark(require = pos)] lon: Option<UnpackFloat>,
        #[starlark(require = pos)] date: Option<&str>,
        #[starlark(require = pos)] time: Option<Value<'v>>,
        #[starlark(require = named)] tz: Option<UnpackFloat>,
        #[starlark(require = named)] az: Option<UnpackFloat>,
        #[starlark(require = named)] el: Option<UnpackFloat>,
    ) -> anyhow::Result<KSun> {
        let placed = az.is_some() || el.is_some();
        let sited = lat.is_some() || lon.is_some() || date.is_some() || time.is_some() || tz.is_some();
        if placed && sited {
            anyhow::bail!("sun: either a place and a time (sun(lat, lon, date, time)) or a direction (sun(az=, el=)), not both");
        }
        if placed {
            let (Some(az), Some(el)) = (az, el) else {
                anyhow::bail!("sun: az= and el= go together");
            };
            let az = num(&az, "sun: az")?;
            let el = num(&el, "sun: el")?;
            if !(-90.0..=90.0).contains(&el) {
                anyhow::bail!("sun: el must be between -90 and 90 degrees, got {}", fmt_num(el));
            }
            return Ok(KSun { spec: SunSpec::angles(az.rem_euclid(360.0), el), position: None, site: None, date: None, hours: None });
        }
        let (Some(lat), Some(lon), Some(date), Some(time)) = (lat, lon, date, time) else {
            anyhow::bail!("sun: expected sun(lat, lon, \"YYYY-MM-DD\", \"HH:MM\", tz=) or sun(az=, el=)");
        };
        let site = site_of(num(&lat, "sun: lat")?, num(&lon, "sun: lon")?, opt_num(&tz, "sun: tz")?, "sun")?;
        let date = date_of(date, "sun: date")?;
        let hours = hours_of(time, "sun: time")?;
        Ok(sun_at(site, date, hours))
    }

    /// The dates (UTC) of the June and December solstices of a year, as
    /// `["YYYY-MM-DD", "YYYY-MM-DD"]`: the longest and shortest days, the
    /// two extremes a shadow study is drawn at. Good to a minute over
    /// 2000-2100 (Meeus).
    fn solstices<'v>(#[starlark(require = pos)] year: UnpackFloat, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        let y = int_arg(year, "solstices: year")?;
        Ok(heap.alloc(sun::solstices(y).iter().map(|d| d.to_string()).collect::<Vec<_>>()))
    }

    /// The dates (UTC) of the March and September equinoxes of a year, as
    /// `["YYYY-MM-DD", "YYYY-MM-DD"]`: the days the sun rises due east and
    /// stands `90 - latitude` degrees high at noon, the middle case of a
    /// shadow study.
    fn equinoxes<'v>(#[starlark(require = pos)] year: UnpackFloat, heap: Heap<'v>) -> anyhow::Result<Value<'v>> {
        let y = int_arg(year, "equinoxes: year")?;
        Ok(heap.alloc(sun::equinoxes(y).iter().map(|d| d.to_string()).collect::<Vec<_>>()))
    }

    /// The document's own render settings, which every render of
    /// it takes unless the request says otherwise: `sun=sun(...)` lights
    /// the scene with that sun and its shadows (`cadpole render --sun` wins
    /// over it); `north=` says where north is in model space, in degrees
    /// clockwise from +y seen from above (0: +y is north, the default; 90:
    /// +x is north), and turns the sun to match; `camera=view(eye=,
    /// target=, fov=)` is a free camera -- a neighbour's window, a point on
    /// the sidewalk -- that `cadpole render` uses when no `--view` or `--eye`
    /// is given. A declaration, not a feature: it builds nothing, and a
    /// document may make it once.
    fn render_defaults<'v>(
        #[starlark(require = named)] sun: Option<Value<'v>>,
        #[starlark(require = named)] north: Option<UnpackFloat>,
        #[starlark(require = named)] camera: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let st = store(eval)?;
        if st.in_test.get() {
            anyhow::bail!("render_defaults: cannot be declared inside a test");
        }
        let sun = match sun {
            None => None,
            Some(v) if v.is_none() => None,
            Some(v) => Some(
                v.downcast_ref::<KSun>()
                    .ok_or_else(|| anyhow::anyhow!("render_defaults: sun= takes a sun(...), got {}", v.get_type()))?
                    .spec
                    .clone(),
            ),
        };
        let north_degrees = opt_num(&north, "render_defaults: north")?;
        let camera = match camera {
            None => None,
            Some(v) if v.is_none() => None,
            Some(v) => {
                let k = v
                    .downcast_ref::<KView>()
                    .ok_or_else(|| anyhow::anyhow!("render_defaults: camera= takes a view(...), got {}", v.get_type()))?
                    .key;
                let (Some(eye), Some(target)) = (k.eye, k.target) else {
                    anyhow::bail!("render_defaults: camera= takes a free camera, view(eye=(x, y, z), target=(x, y, z)); a preset's az/el is a request's business (--view)");
                };
                if (target - eye).length() <= 0.0 {
                    anyhow::bail!("render_defaults: the camera's eye and target coincide");
                }
                Some(CameraSpec { eye, target, up: cadpole_core::Vec3::Z, fov_degrees: k.fov.unwrap_or(35.0) })
            }
        };
        let mut ctx = st.ctx.borrow_mut();
        if ctx.render_defaults.is_some() {
            anyhow::bail!("render_defaults declared twice; a document has one");
        }
        ctx.render_defaults = Some(RenderDefaults { sun, north_degrees, camera });
        Ok(NoneType)
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run};

    /// A sun for a place and a time carries the almanac's answers; a sun by
    /// angles carries none of them; the vector points the way the compass
    /// says.
    #[test]
    fn a_sun_reads_back() {
        let ctx = run(
            "s = sun(38.0, -121.0, \"2026-06-21\", \"13:00\", tz=-7)\n\
             assert_true(s.up)\n\
             assert_true(s.elevation > 70 and s.elevation < 80, msg=str(s.elevation))\n\
             assert_true(s.azimuth > 150 and s.azimuth < 210, msg=str(s.azimuth))\n\
             assert_eq(s.date, \"2026-06-21\")\n\
             assert_eq(s.time, \"13:00\")\n\
             assert_eq(s.tz, -7)\n\
             assert_true(s.sunrise < \"06:00\" and s.sunset > \"20:00\", msg=s.sunrise + \" \" + s.sunset)\n\
             assert_true(s.direction[2] > 0.9)\n\
             assert_true(s.direction[1] < 0, msg=\"the noon sun is in the south, so the vector leans -y\")\n\
             assert_eq(s.label, \"2026-06-21 13:00\")\n\
             n = sun(38.0, -121.0, \"2026-12-21\", 12)\n\
             assert_true(n.elevation < 30 and n.elevation > 25, msg=str(n.elevation))\n\
             assert_eq(n.tz, -8)\n\
             h = sun(az=90, el=30)\n\
             assert_eq(h.azimuth, 90)\n\
             assert_eq(h.date, None)\n\
             assert_eq(h.sunrise, None)\n\
             assert_true(abs(h.direction[0] - 0.866) < 0.001)\n\
             assert_eq(solstices(2026), [\"2026-06-21\", \"2026-12-21\"])\n\
             assert_eq(equinoxes(2026), [\"2026-03-20\", \"2026-09-23\"])\n\
             assert_true(\"az 90 el 30\" in str(h))\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
    }

    #[test]
    fn refusals() {
        for (src, needle) in [
            ("sun(37, -122, \"2026-06-21\", \"12:00\", az=1)", "not both"),
            ("sun(az=10)", "go together"),
            ("sun(37, -122)", "expected sun(lat, lon"),
            ("sun(95, -122, \"2026-06-21\", \"12:00\")", "lat must be"),
            ("sun(37, -122, \"2026-13-01\", \"12:00\")", "month 13"),
            ("sun(37, -122, \"2026-06-21\", \"25:00\")", "not a time"),
            ("sun(37, -122, \"2026-06-21\", 30)", "between 0 and 24"),
            ("sun(37, -122, \"2026-06-21\", \"12:00\", tz=20)", "between -14 and 14"),
            ("render_defaults(sun=3)", "takes a sun(...)"),
            ("render_defaults(camera=view(az=30))", "free camera"),
            ("render_defaults(camera=view(eye=(0, 0, 0), target=(0, 0, 0)))", "coincide"),
            ("render_defaults(north=0)\nrender_defaults(north=1)", "declared twice"),
        ] {
            let ctx = run(src);
            let e = errors(&ctx);
            assert_eq!(e.len(), 1, "{src}: {e:?}");
            assert!(e[0].contains(needle), "{src}: {} does not mention {needle:?}", e[0]);
        }
    }

    /// The declaration lands on the context, and in the model.
    #[test]
    fn render_defaults_are_recorded() {
        let ctx = run(
            "b = box(10, 10, 10)\n\
             render_defaults(sun=sun(az=200, el=50), north=30, camera=view(eye=(100, -100, 50), target=(5, 5, 5), fov=40))\n",
        );
        assert!(errors(&ctx).is_empty(), "{:?}", errors(&ctx));
        let d = ctx.render_defaults.clone().unwrap();
        assert_eq!((d.sun.as_ref().unwrap().azimuth, d.sun.as_ref().unwrap().elevation), (200.0, 50.0));
        assert_eq!(d.north_degrees, Some(30.0));
        let c = d.camera.unwrap();
        assert_eq!((c.eye.x, c.target.z, c.fov_degrees), (100.0, 5.0, 40.0));
        assert!(ctx.features.len() == 1, "a declaration is not a feature");
        let ctx = run("render_defaults(north=0)\n");
        assert!(errors(&ctx).is_empty());
        assert_eq!(ctx.render_defaults.unwrap(), cadpole_core::context::RenderDefaults { sun: None, north_degrees: Some(0.0), camera: None });
    }
}
