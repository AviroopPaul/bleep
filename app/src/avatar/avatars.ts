// The floating buddies. Each one is a single SVG built from its shape, a color and a finish.
// Moods are CSS only (theme.css, "avatar"), keyed on data-mood. Design: docs/avatar-options.html.
// Keep in step with src-tauri/src/avatar.rs (kinds, default colors) and icons/tray/*.svg.
import { DROID_FULL, DROID_HEAD } from "../droid/droidSvg";

export type AvatarKind = "blip" | "gumdrop" | "sprout" | "boop" | "puff" | "spark" | "droid";
export type Finish = "flat" | "glossy" | "plush";
export type Mood = "idle" | "thinking" | "alert" | "happy" | "listening";
export interface AvatarConfig {
  kind: AvatarKind;
  color: string;
  finish: Finish;
}

export const DEFAULT_AVATAR: AvatarConfig = { kind: "blip", color: "#3D6BFF", finish: "glossy" };
export const FINISHES: { id: Finish; name: string }[] = [
  { id: "flat", name: "Flat" },
  { id: "glossy", name: "Glossy" },
  { id: "plush", name: "Plush" },
];
export const PALETTE: { name: string; hex: string }[] = [
  { name: "Blue", hex: "#3D6BFF" },
  { name: "Violet", hex: "#8B5CF6" },
  { name: "Pink", hex: "#FF4FA3" },
  { name: "Coral", hex: "#FF6B4A" },
  { name: "Sun", hex: "#FFB020" },
  { name: "Mint", hex: "#1FC48E" },
  { name: "Sky", hex: "#38BDF8" },
];

const INK = "#1B1F24";

interface Face { x: number; y: number; s: number; w: number; h: number; cheeks?: boolean }
interface Buddy {
  id: AvatarKind;
  name: string;
  /** Default color. */
  color: string;
  /** Top of the body, for the mood effects. */
  top: number;
  eye?: string;
  face?: Face;
  hl?: string;
  extra?: string;
  body?: (paint: string) => string;
  pixel?: boolean;
  /** Only the droid: its own artwork, no color or finish. */
  fixed?: boolean;
}

export const BUDDIES: Buddy[] = [
  {
    id: "blip", name: "Blip", color: "#3D6BFF", eye: "#fff", top: 28,
    face: { x: 60, y: 58, s: 11, w: 8, h: 14 },
    hl: '<ellipse cx="41" cy="42" rx="9" ry="4.5" transform="rotate(-35 41 42)" fill="#fff" opacity=".45"/>',
    body: (p) => `<path d="M50 28 H70 A26 26 0 0 1 96 54 V64 A26 26 0 0 1 70 90 H57 L41 103 L43 88 A26 26 0 0 1 24 64 V54 A26 26 0 0 1 50 28 Z" fill="${p}"/>`,
  },
  {
    id: "gumdrop", name: "Gumdrop", color: "#1FC48E", eye: INK, top: 26,
    face: { x: 60, y: 78, s: 10, w: 6.5, h: 9, cheeks: true },
    hl: '<ellipse cx="50" cy="52" rx="6" ry="3.5" transform="rotate(-55 50 52)" fill="#fff" opacity=".45"/>',
    body: (p) => `<path d="M60 26 C71 26 100 72 99 88 C98 101 84 106 60 106 C36 106 22 101 21 88 C20 72 49 26 60 26 Z" fill="${p}"/>`,
  },
  {
    id: "sprout", name: "Sprout", color: "#FF6B4A", eye: INK, top: 12,
    face: { x: 60, y: 74, s: 12, w: 7, h: 10, cheeks: true },
    hl: '<ellipse cx="43" cy="54" rx="7" ry="4" transform="rotate(-50 43 54)" fill="#fff" opacity=".45"/>',
    extra: `<g class="leaf"><path d="M60 38 V24" stroke="#1E9E5A" stroke-width="3.4" stroke-linecap="round"/>
      <path d="M60 25 C53 12 41 13 37 19 C43 27 53 28 60 25Z" fill="#2FBF71"/>
      <path d="M60 23 C65 8 80 7 85 13 C80 23 68 26 60 23Z" fill="#3BD07F"/></g>`,
    body: (p) => `<path d="M60 36 C84 36 96 56 96 76 C96 96 80 106 60 106 C40 106 24 96 24 76 C24 56 36 36 60 36Z" fill="${p}"/>`,
  },
  { id: "boop", name: "Boop", color: "#8B5CF6", top: 16, pixel: true },
  {
    id: "puff", name: "Puff", color: "#38BDF8", eye: INK, top: 33,
    face: { x: 60, y: 80, s: 12, w: 7, h: 9.5, cheeks: true },
    hl: '<ellipse cx="52" cy="44" rx="9" ry="4.5" transform="rotate(-20 52 44)" fill="#fff" opacity=".5"/>',
    body: (p) => `<g fill="${p}"><circle cx="41" cy="74" r="20"/><circle cx="62" cy="58" r="25"/><circle cx="85" cy="71" r="18"/><rect x="24" y="70" width="73" height="34" rx="17"/></g>`,
  },
  {
    id: "spark", name: "Spark", color: "#FFB020", eye: INK, top: 20,
    face: { x: 60, y: 64, s: 8, w: 5.5, h: 7.5 },
    hl: '<ellipse cx="53" cy="44" rx="3" ry="6" transform="rotate(20 53 44)" fill="#fff" opacity=".5"/>',
    body: (p) => `<path d="M60 20 Q73 50 102 62 Q73 74 60 104 Q47 74 18 62 Q47 50 60 20Z" fill="${p}" stroke="${p}" stroke-width="9" stroke-linejoin="round"/>`,
  },
  { id: "droid", name: "Droid", color: "#2F5C93", top: 0, fixed: true },
];

export const buddy = (kind: string): Buddy => BUDDIES.find((b) => b.id === kind) ?? BUDDIES[0];

/** Unknown values fall back to the defaults, like `AvatarConfig::normalized` in Rust. */
export function normalize(c: Partial<AvatarConfig> | null | undefined): AvatarConfig {
  const b = BUDDIES.find((x) => x.id === c?.kind);
  if (!b) return DEFAULT_AVATAR;
  const color = /^#[0-9a-f]{6}$/i.test(c?.color ?? "") ? c!.color!.toUpperCase() : b.color;
  const finish = FINISHES.some((f) => f.id === c?.finish) ? (c!.finish as Finish) : "glossy";
  return { kind: b.id, color, finish };
}

/** Switching buddy keeps a color the user picked, but swaps a default color for the new buddy's. */
export function switchKind(c: AvatarConfig, kind: AvatarKind): AvatarConfig {
  const color = c.color.toUpperCase() === buddy(c.kind).color ? buddy(kind).color : c.color;
  return { ...c, kind, color };
}

/** The finish a buddy really uses. Boop is pixel art and the droid has its own artwork. */
export const finishOf = (c: AvatarConfig): Finish | null => (buddy(c.kind).fixed ? null : buddy(c.kind).pixel ? "flat" : c.finish);

export function mix(hex: string, to: string, t: number): string {
  const a = parseInt(hex.slice(1), 16), b = parseInt(to.slice(1), 16);
  const c = [16, 8, 0].map((s) => Math.round(((a >> s) & 255) * (1 - t) + ((b >> s) & 255) * t));
  return "#" + c.map((v) => v.toString(16).padStart(2, "0")).join("");
}

function face(f: Face, eye: string): string {
  const { s, w, h } = f, my = h / 2 + 5;
  const dark = eye === INK;
  const pill = (x: number) =>
    `<rect x="${x - w / 2}" y="${-h / 2}" width="${w}" height="${h}" rx="${w / 2}" fill="${eye}"/>` +
    (dark ? `<circle cx="${x - w * 0.12}" cy="${-h * 0.2}" r="${w * 0.24}" fill="#fff"/>` : "");
  const arc = (x: number) =>
    `<path d="M${x - w / 2 - 1} 1.5 Q${x} ${-h / 2 - 3} ${x + w / 2 + 1} 1.5" fill="none" stroke="${eye}" stroke-width="${Math.max(2.4, w * 0.5)}" stroke-linecap="round"/>`;
  const cheeks = f.cheeks ? [-1, 1].map((d) => `<ellipse cx="${d * (s + w / 2 + 5)}" cy="${h / 2 + 1}" rx="4.6" ry="2.8" fill="#FF6F91" opacity=".5"/>`).join("") : "";
  return `<g transform="translate(${f.x} ${f.y})">
    <g class="look"><g class="wide"><g class="eyes-open">${pill(-s)}${pill(s)}</g></g>
    <g class="eyes-happy">${arc(-s)}${arc(s)}</g></g>${cheeks}
    <g class="mouth" fill="none" stroke="${eye}" stroke-width="2.3" stroke-linecap="round">
      <path class="m-idle" d="M-4 ${my} Q0 ${my + 3.6} 4 ${my}"/>
      <path class="m-happy" d="M-5.5 ${my - 0.5} Q0 ${my + 8} 5.5 ${my - 0.5} Z" fill="${eye}"/>
      <ellipse class="m-alert" cx="0" cy="${my + 2}" rx="2.6" ry="3.3" fill="${eye}" stroke="none"/>
      <path class="m-think" d="M-3 ${my + 1.5} L3.5 ${my}"/>
      <ellipse class="m-listen" cx="0" cy="${my + 1.2}" rx="2.8" ry="2.2" fill="${eye}" stroke="none"/>
    </g></g>`;
}

function fx(b: Buddy): string {
  const t = b.top, fy = b.face ? b.face.y : 64;
  const spark = (x: number, y: number, r: number) =>
    `<path d="M${x} ${y - r} Q${x + r * 0.25} ${y - r * 0.25} ${x + r} ${y} Q${x + r * 0.25} ${y + r * 0.25} ${x} ${y + r} Q${x - r * 0.25} ${y + r * 0.25} ${x - r} ${y} Q${x - r * 0.25} ${y - r * 0.25} ${x} ${y - r}Z" fill="#FFC53D"/>`;
  return `<g class="fx">
    <g class="fx-think"><circle cx="86" cy="${t - 2}" r="3.2"/><circle cx="95" cy="${t - 9}" r="3.8"/><circle cx="105" cy="${t - 17}" r="4.4"/></g>
    <g class="fx-alert"><g><circle cx="96" cy="${t + 2}" r="10" fill="#FF4D4F"/><rect x="94.3" y="${t - 4.5}" width="3.4" height="8.5" rx="1.7" fill="#fff"/><circle cx="96" cy="${t + 7.6}" r="1.9" fill="#fff"/></g></g>
    <g class="fx-listen"><path d="M14 ${fy - 9} Q8 ${fy} 14 ${fy + 9}"/><path d="M7 ${fy - 15} Q-2 ${fy} 7 ${fy + 15}"/><path d="M106 ${fy - 9} Q112 ${fy} 106 ${fy + 9}"/><path d="M113 ${fy - 15} Q122 ${fy} 113 ${fy + 15}"/></g>
    <g class="fx-happy">${spark(18, t + 6, 7)}${spark(104, t + 14, 6)}${spark(100, t + 58, 5)}</g>
  </g>`;
}

const PX = (cells: [number, number][], ox: number, oy: number, p: number, fill: string) =>
  `<path shape-rendering="crispEdges" fill="${fill}" d="${cells.map(([x, y]) => `M${ox + x * p} ${oy + y * p}h${p}v${p}h-${p}z`).join("")}"/>`;
const grid = (rows: string[]) => rows.flatMap((r, y) => [...r].flatMap((ch, x) => (ch === "#" ? [[x, y] as [number, number]] : [])));
const BOOP = grid([
  "..............",
  "......##......",
  "....######....",
  "..##########..",
  ".############.",
  ".############.",
  "##############",
  "##############",
  "##############",
  ".############.",
  ".############.",
  "..##########..",
  "..###....###..",
]);
const BOOP_BULB: [number, number][] = [[6, -1], [7, -1], [5, -2], [6, -2], [7, -2], [8, -2], [5, -3], [6, -3], [7, -3], [8, -3]];
const BOOP_STEM: [number, number][] = [[6, 0], [7, 0]];

function boop(color: string): { body: string; face: string } {
  const p = 6, ox = 18, oy = 28, W = "#fff";
  const body = PX(BOOP, ox, oy, p, color) + PX(BOOP_BULB, ox, oy, p, "#FFC53D") + PX(BOOP_STEM, ox, oy, p, mix(color, "#000000", 0.25));
  const f = `<g class="look"><g class="wide"><g class="eyes-open">${PX([[4, 5], [4, 6], [4, 7], [9, 5], [9, 6], [9, 7]], ox, oy, p, W)}</g></g>
    <g class="eyes-happy">${PX([[3, 7], [4, 6], [5, 7], [8, 7], [9, 6], [10, 7]], ox, oy, p, W)}</g></g>
    <g class="mouth"><g class="m-happy">${PX([[6, 9], [7, 9]], ox, oy, p, W)}</g><g class="m-alert">${PX([[6, 9], [6, 10], [7, 9], [7, 10]], ox, oy, p, W)}</g></g>
    ${PX([[2, 8], [11, 8]], ox, oy, p, "#FF8FB8")}`;
  return { body, face: f };
}

let uid = 0;

/** The buddy as SVG markup. `still` crops it for small spots (chat header, meeting bar):
 * no shadow, no mood effects. */
export function avatarSvg(cfg: AvatarConfig, opts: { still?: boolean } = {}): string {
  const c = normalize(cfg);
  const b = buddy(c.kind);
  if (b.fixed) return opts.still ? DROID_HEAD : DROID_FULL;
  const id = `av${++uid}`;
  const finish = finishOf(c);
  let defs = "", paint = c.color, filt = "", hl = "";
  if (finish === "glossy") {
    defs = `<linearGradient id="${id}g" gradientUnits="userSpaceOnUse" x1="0" y1="${b.top}" x2="0" y2="108"><stop offset="0" stop-color="${mix(c.color, "#ffffff", 0.38)}"/><stop offset=".55" stop-color="${c.color}"/><stop offset="1" stop-color="${mix(c.color, "#000000", 0.22)}"/></linearGradient>`;
    paint = `url(#${id}g)`;
    hl = b.hl ?? "";
  } else if (finish === "plush") {
    defs = `<radialGradient id="${id}g" gradientUnits="userSpaceOnUse" cx="46" cy="${b.top + 18}" r="80"><stop offset="0" stop-color="${mix(c.color, "#ffffff", 0.3)}"/><stop offset=".6" stop-color="${c.color}"/><stop offset="1" stop-color="${mix(c.color, "#000000", 0.28)}"/></radialGradient>
      <filter id="${id}f" x="-12%" y="-12%" width="124%" height="124%">
        <feTurbulence type="fractalNoise" baseFrequency="1.1" numOctaves="2" seed="4" result="n"/>
        <feDisplacementMap in="SourceGraphic" in2="n" scale="3" xChannelSelector="R" yChannelSelector="G" result="d"/>
        <feTurbulence type="fractalNoise" baseFrequency="2.4" numOctaves="1" seed="9" result="n2"/>
        <feColorMatrix in="n2" type="matrix" values="0 0 0 0 1  0 0 0 0 1  0 0 0 0 1  0 0 0 -2.2 1.0" result="lite"/>
        <feComposite in="lite" in2="d" operator="in" result="f1"/>
        <feComponentTransfer in="f1" result="fleck"><feFuncA type="linear" slope=".45"/></feComponentTransfer>
        <feMerge><feMergeNode in="d"/><feMergeNode in="fleck"/></feMerge>
      </filter>`;
    paint = `url(#${id}g)`;
    filt = ` filter="url(#${id}f)"`;
  }
  const art = b.pixel ? boop(c.color) : { body: b.body!(paint), face: face(b.face!, b.eye!) };
  const view = opts.still ? "10 4 100 106" : "0 0 120 130";
  const shadow = opts.still ? "" : '<ellipse class="shadow" cx="60" cy="121" rx="26" ry="4.5"/>';
  return `<svg viewBox="${view}" aria-hidden="true" data-kind="${c.kind}" data-finish="${finish}"><defs>${defs}</defs>${shadow}
    <g class="bob">${b.extra ?? ""}<g${filt}>${art.body}</g>${hl}${art.face}${opts.still ? "" : fx(b)}</g></svg>`;
}
