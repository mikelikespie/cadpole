// Line icons for the touch layout, drawn as inline SVG.
//
// The desktop toolbar spells its icons as text glyphs, which is fine where the
// platform font has them. A phone's often does not -- the glyph comes out as
// an emoji or an empty box -- so everything the narrow layout draws comes from
// here instead: one 24 x 24 path a piece, stroked in the current colour.

const PATHS = {
  params: "M4 7h9M17 7h3M4 17h3M11 17h9M15 4.5v5M9 14.5v5",
  layers: "M12 4l8 4-8 4-8-4zM4 12l8 4 8-4M4 16l8 4 8-4",
  model: "M5 4h6v4H5zM13 10h6v4h-6zM13 16h6v4h-6zM8 8v10h5M8 12h5",
  script: "M9 8l-4 4 4 4M15 8l4 4-4 4",
  inspector: "M12 4a8 8 0 1 0 0 16a8 8 0 1 0 0-16zM12 11v5M12 8v.01",
  messages: "M5 6h14v9h-9l-4 3v-3H5z",
  issues: "M12 5l8 14H4zM12 10v4M12 16.5v.01",
  examples: "M5 5h6v6H5zM13 5h6v6h-6zM5 13h6v6H5zM13 13h6v6h-6z",
  api: "M8 5c-2 0-2 2-2 3.5S6 12 4 12c2 0 2 2 2 3.5S6 19 8 19M16 5c2 0 2 2 2 3.5s0 3.5 2 3.5c-2 0-2 2-2 3.5S18 19 16 19",
  view: "M12 3l8 4.5v9L12 21l-8-4.5v-9zM12 12l8-4.5M12 12v9M12 12L4 7.5",
  more: "M6 12h.01M12 12h.01M18 12h.01",
  fit: "M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5",
  home: "M4 11l8-7 8 7M6.5 9.5V19h11V9.5",
  focus: "M12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6zM12 3v3M12 18v3M3 12h3M18 12h3",
  close: "M6 6l12 12M18 6L6 18",
  chevron: "M9 6l6 6-6 6",
  back: "M15 6l-6 6 6 6",
  section: "M12 4l7 3.5v4M12 4L5 7.5v4M2.5 13h19M5 15v1.5L12 20l7-3.5V15",
} as const;

export type IconName = keyof typeof PATHS;

/** An icon, hidden from the accessibility tree: the control it sits in carries the name. */
export function icon(name: IconName, size = 22): SVGSVGElement {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("class", `ico ico-${name}`);
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", name === "more" ? "2.6" : "1.7");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("focusable", "false");
  const path = document.createElementNS(ns, "path");
  path.setAttribute("d", PATHS[name]);
  svg.appendChild(path);
  return svg;
}

/**
 * The Cadpole mark: the tadpole, in its own colours. The same drawing is
 * the page's icon and its splash (index.html, manifest.webmanifest).
 */
export function cadpoleMark(size = 22): SVGSVGElement {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("class", "cadpole-mark");
  svg.setAttribute("viewBox", "15 3 102 122");
  svg.setAttribute("width", String(Math.round((size * 102) / 122)));
  svg.setAttribute("height", String(size));
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("focusable", "false");
  const add = (tag: string, attrs: Record<string, string>): void => {
    const el = document.createElementNS(ns, tag);
    for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
    svg.appendChild(el);
  };
  add("path", { d: "M112.1 24.6A53.7 53.7 0 1 0 66 111.3C69.6 125.9 90.4 126.2 106.3 120.6C102.5 104.8 87.4 87.8 67.5 89.7A23.4 23.4 0 1 1 84.5 46.2A17.5 17.5 0 0 0 112.1 24.6Z", fill: "#8fe3c4" });
  add("path", { d: "M112.1 24.6A53.7 53.7 0 1 0 66 111.3C73.9 115.1 91 116.2 99 116.9C94.5 110.2 82.4 97.2 67.5 89.7A23.4 23.4 0 1 1 84.5 46.2A17.5 17.5 0 0 0 112.1 24.6Z", fill: "#2fbf9f" });
  add("path", { d: "M69.6 25.8a10.5 10.5 0 1 0 21 0a10.5 10.5 0 1 0 -21 0Z", fill: "#fff" });
  add("path", { d: "M77.1 26.5a5.8 5.8 0 1 0 11.7 0a5.8 5.8 0 1 0 -11.7 0Z", fill: "#0b2e2a" });
  return svg;
}
