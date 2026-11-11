import type { DisplayUnits, LengthUnit, ParamValue, Vec3 } from "./types.js";

export function debounce<A extends unknown[]>(
  fn: (...args: A) => void,
  ms: number,
): ((...args: A) => void) & { flush(): void; cancel(): void; pending(): boolean } {
  let timer: number | null = null;
  let lastArgs: A | null = null;
  const wrapped = (...args: A): void => {
    lastArgs = args;
    if (timer !== null) window.clearTimeout(timer);
    timer = window.setTimeout(() => {
      timer = null;
      const a = lastArgs;
      lastArgs = null;
      if (a) fn(...a);
    }, ms);
  };
  wrapped.flush = () => {
    if (timer !== null) {
      window.clearTimeout(timer);
      timer = null;
      const a = lastArgs;
      lastArgs = null;
      if (a) fn(...a);
    }
  };
  wrapped.cancel = () => {
    if (timer !== null) window.clearTimeout(timer);
    timer = null;
    lastArgs = null;
  };
  /** Whether a call is sitting on the timer, waiting to happen. */
  wrapped.pending = () => timer !== null;
  return wrapped;
}

/** Format a length in mm for display: trims noise, keeps up to 3 decimals. */
export function fmt(n: number, digits = 3): string {
  if (!Number.isFinite(n)) return "–";
  if (Math.abs(n) < 1e-9) return "0";
  const abs = Math.abs(n);
  if (abs >= 1e6 || abs < 1e-4) return n.toExponential(2);
  const s = n.toFixed(digits);
  // Trailing zeros are noise only after a decimal point: 100 is not 1.
  return s.includes(".") ? s.replace(/\.?0+$/, "") : s;
}

/**
 * A number as an editable field holds it: every digit that matters, so that
 * reading the field back gives the value (to 12 significant digits, which
 * drops only float noise such as 0.30000000000000004). Unlike `fmt`, which is
 * for reading, this never rounds to a few decimals or three digits of an
 * exponent. Exponent form only where JavaScript itself uses it (below 1e-6 or
 * from 1e21), which a number input accepts as written.
 */
export function fmtField(n: number): string {
  if (!Number.isFinite(n)) return "";
  return String(Number(n.toPrecision(12)));
}

export function fmtVec(v: Vec3, digits = 3): string {
  return `${fmt(v.x, digits)}, ${fmt(v.y, digits)}, ${fmt(v.z, digits)}`;
}

const UNIT_SCALE: Record<LengthUnit, number> = { mm: 1, cm: 10, m: 1000, in: 25.4, ft: 304.8, "ft-in": 304.8 };

/** Convert an internal millimetre value to the document's display unit. */
export function toDisplay(mm: number, unit: LengthUnit): number {
  return mm / UNIT_SCALE[unit];
}

/** Is `unit` one of the length units a `param(unit=)` can name? */
export function isLengthUnit(unit: string): unit is LengthUnit {
  return unit in UNIT_SCALE;
}

/**
 * Format a length in the document's display unit. `ft-in` writes the
 * architectural `45'-6"`, inches rounded to `1/fraction` (sixteenths by
 * default, 0 for decimal inches) — the same rule as `cadpole inspect` and the
 * drawings, so a size reads the same everywhere. The other units are a
 * decimal and the unit's name.
 */
export function fmtLen(mm: number, unit: LengthUnit, fraction = 16): string {
  if (unit === "ft-in") return fmtFtIn(mm, fraction);
  return `${fmt(toDisplay(mm, unit))} ${unit}`;
}

/**
 * A point in the document's display units: `fmtVec` for millimetres,
 * as it always was; each coordinate written as a length otherwise, so a house
 * in feet does not show its corners in millimetres.
 */
export function fmtPoint(v: Vec3, unit: LengthUnit, fraction = 16): string {
  if (unit === "mm") return fmtVec(v);
  return [v.x, v.y, v.z].map((c) => fmtLen(c, unit, fraction)).join(", ");
}

/**
 * A typed length back to millimetres, the inverse of `fmtLen`: what
 * the Section panel's fields accept. A bare number is in the display unit
 * (feet for `ft-in`); a written unit wins (`1200mm`, `1.2 m`, `54in`,
 * `4.5ft`), and the architect's forms read as written: `12'-6"`, `12' 6"`,
 * `12'6 1/2"`, `6"`, `4'`. Null for anything else.
 */
export function parseLen(text: string, unit: LengthUnit): number | null {
  const s = text.trim().replace(/[′’]/g, "'").replace(/[″”]/g, '"');
  if (s === "") return null;
  const num = "[-+]?(?:\\d+(?:\\.\\d*)?|\\.\\d+)";
  // An inch figure: `6`, `6.5`, `6 1/2`, `1/2`.
  const inch = `(?:(\\d+(?:\\.\\d*)?)(?:\\s+(\\d+)\\/(\\d+))?|(\\d+)\\/(\\d+))`;
  const inches = (m: RegExpMatchArray, at: number): number => {
    if (m[at] !== undefined) return Number(m[at]) + (m[at + 1] !== undefined ? Number(m[at + 1]) / Number(m[at + 2]) : 0);
    if (m[at + 3] !== undefined) return Number(m[at + 3]) / Number(m[at + 4]);
    return 0;
  };
  // Feet, optionally with inches: 12'-6", 12' 6", 12'6", 4', -2'-3".
  const ftIn = s.match(new RegExp(`^(-)?\\s*(\\d+(?:\\.\\d*)?)\\s*'\\s*(?:-?\\s*${inch}\\s*"?)?$`));
  if (ftIn) {
    const mm = Number(ftIn[2]) * 304.8 + inches(ftIn, 3) * 25.4;
    return ftIn[1] ? -mm : mm;
  }
  // Inches alone: 6", 6 1/2", -3".
  const inOnly = s.match(new RegExp(`^(-)?\\s*${inch}\\s*"$`));
  if (inOnly) {
    const mm = inches(inOnly, 2) * 25.4;
    return inOnly[1] ? -mm : mm;
  }
  const withUnit = s.match(new RegExp(`^(${num})\\s*(mm|cm|m|in|ft)?$`, "i"));
  if (!withUnit) return null;
  const v = Number(withUnit[1]);
  if (!Number.isFinite(v)) return null;
  const u = (withUnit[2]?.toLowerCase() as LengthUnit | undefined) ?? unit;
  return v * UNIT_SCALE[u];
}

/** `3810` → `12'-6"`; `152.4` → `6"`; `-3810` → `-12'-6"`. */
export function fmtFtIn(mm: number, fraction = 16): string {
  if (!Number.isFinite(mm)) return "–";
  const sign = mm < -1e-9 ? "-" : "";
  const inchesTotal = Math.abs(mm) / 25.4;
  if (fraction <= 0) {
    const feet = Math.floor(inchesTotal / 12);
    const rest = inchesTotal - feet * 12;
    return feet === 0 ? `${sign}${fmt(rest, 2)}"` : `${sign}${feet}'-${fmt(rest, 2)}"`;
  }
  const units = Math.round(inchesTotal * fraction);
  let feet = Math.floor(units / (12 * fraction));
  let restUnits = units - feet * 12 * fraction;
  if (restUnits === 12 * fraction) {
    feet += 1;
    restUnits = 0;
  }
  const whole = Math.floor(restUnits / fraction);
  let num = restUnits - whole * fraction;
  let den = fraction;
  while (num > 0 && num % 2 === 0 && den % 2 === 0) {
    num /= 2;
    den /= 2;
  }
  const inches = num === 0 ? `${whole}` : whole === 0 ? `${num}/${den}` : `${whole} ${num}/${den}`;
  return feet === 0 ? `${sign}${inches}"` : `${sign}${feet}'-${inches}"`;
}

/**
 * A length parameter's readout. A `param(unit=)` value is a number
 * in the unit it names — `param("climb", 10, unit="ft")` is ten feet, and the
 * script writes `ft(climb)` — so the readout converts it to millimetres and
 * writes those in the document's `units(...)`: `45'-6"` for 45.5 ft under
 * `units.ft_in`, `3.81 m` for 3810 mm under `units.m`. With no declaration,
 * or a unit that is not a length (`deg`, `%`), it is the number and the
 * unit, as before.
 */
export function fmtParamLength(value: number, unit: string, display: DisplayUnits | undefined): string {
  if (display && isLengthUnit(unit)) return fmtLen(value * UNIT_SCALE[unit], display.unit, display.fraction);
  return unit ? `${fmt(value)} ${unit}` : fmt(value);
}

export function fmtArea(mm2: number, unit: LengthUnit): string {
  if (unit === "ft-in" || unit === "ft") return `${fmt(mm2 / (304.8 * 304.8), 1)} sq ft`;
  const s = UNIT_SCALE[unit];
  return `${fmt(mm2 / (s * s))} ${unit}²`;
}

export function fmtTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function paramValuesEqual(a: ParamValue, b: ParamValue): boolean {
  if (typeof a !== typeof b) return false;
  if (typeof a === "object" && typeof b === "object") return a.x === b.x && a.y === b.y && a.z === b.z;
  return a === b;
}

export function overridesEqual(a: Record<string, ParamValue>, b: Record<string, ParamValue>): boolean {
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  for (const k of ka) {
    const bv = b[k];
    const av = a[k];
    if (bv === undefined || av === undefined || !paramValuesEqual(av, bv)) return false;
  }
  return true;
}

export function stableJson(v: unknown): string {
  return JSON.stringify(v, (_k, val: unknown) => {
    if (val && typeof val === "object" && !Array.isArray(val)) {
      const o = val as Record<string, unknown>;
      return Object.keys(o)
        .sort()
        .reduce<Record<string, unknown>>((acc, k) => {
          acc[k] = o[k];
          return acc;
        }, {});
    }
    return val;
  });
}
