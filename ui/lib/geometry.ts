// Projection and rotation math mirrored from the Rust engine (core/src/stitch/geometry.rs).
// Frame: x right, y down, z forward. Camera rotation maps camera rays to world directions.

export type V3 = [number, number, number];
export type M3 = number[]; // row-major 3x3

export type Projection =
  | "Equirectangular"
  | "Cylindrical"
  | "Mercator"
  | "Rectilinear"
  | "Panini"
  | "Fisheye"
  | "Stereographic"
  | "LittlePlanet"
  | "Orthographic"
  | "TransverseMercator"
  | "Sinusoidal";

export const PROJECTIONS: Projection[] = [
  "Equirectangular",
  "Cylindrical",
  "Mercator",
  "Rectilinear",
  "Panini",
  "Fisheye",
  "Stereographic",
  "LittlePlanet",
  "Orthographic",
  "TransverseMercator",
  "Sinusoidal",
];

/** Index used by the GLSL shader. */
export const PROJ_INDEX: Record<Projection, number> = Object.fromEntries(PROJECTIONS.map((p, i) => [p, i])) as Record<Projection, number>;

export type LensType = "Rectilinear" | "FisheyeEquidistant" | "FisheyeEquisolid" | "FisheyeStereographic";

const PI = Math.PI;
const HALF_PI = Math.PI / 2;
const D2R = PI / 180;

export const norm = (v: V3): number => Math.hypot(v[0], v[1], v[2]);
export const normalize = (v: V3): V3 => {
  const n = norm(v) || 1;
  return [v[0] / n, v[1] / n, v[2] / n];
};
export const cross = (a: V3, b: V3): V3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
export const dot = (a: V3, b: V3): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

export function mul(m: M3, v: V3): V3 {
  return [m[0] * v[0] + m[1] * v[1] + m[2] * v[2], m[3] * v[0] + m[4] * v[1] + m[5] * v[2], m[6] * v[0] + m[7] * v[1] + m[8] * v[2]];
}
export function mmul(a: M3, b: M3): M3 {
  const r = new Array(9).fill(0);
  for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) for (let k = 0; k < 3; k++) r[i * 3 + j] += a[i * 3 + k] * b[k * 3 + j];
  return r;
}
export const transpose = (m: M3): M3 => [m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]];

const rx = (a: number): M3 => {
  const c = Math.cos(a), s = Math.sin(a);
  return [1, 0, 0, 0, c, -s, 0, s, c];
};
const ry = (a: number): M3 => {
  const c = Math.cos(a), s = Math.sin(a);
  return [c, 0, s, 0, 1, 0, -s, 0, c];
};
const rz = (a: number): M3 => {
  const c = Math.cos(a), s = Math.sin(a);
  return [c, -s, 0, s, c, 0, 0, 0, 1];
};

/** View rotation (view space -> world), degrees; positive pitch looks up. */
export function ypr(yaw: number, pitch: number, roll: number): M3 {
  return mmul(mmul(ry(yaw * D2R), rx(pitch * D2R)), rz(roll * D2R));
}

export function yprFromRotation(m: M3): [number, number, number] {
  const c = mul(m, [0, 0, 1]);
  const pitch = Math.asin(Math.max(-1, Math.min(1, -c[1])));
  const yaw = Math.atan2(c[0], c[2]);
  const r0 = mmul(ry(yaw), rx(pitch));
  const z = mmul(transpose(r0), m);
  const roll = Math.atan2(z[3], z[0]);
  return [yaw / D2R, pitch / D2R, roll / D2R];
}

export function wrapDeg(v: number): number {
  let x = v % 360;
  if (x > 180) x -= 360;
  if (x < -180) x += 360;
  return x;
}

/** (yaw, pitch) in degrees that centre the view on world direction d. */
export function yawPitchTowards(d: V3): [number, number] {
  const n = normalize(d);
  return [Math.atan2(n[0], n[2]) / D2R, Math.asin(Math.max(-1, Math.min(1, -n[1]))) / D2R];
}

/** Solve yaw/pitch (keeping roll) so world direction d is seen along view direction q. */
export function solveGrab(d: V3, q: V3, yaw: number, pitch: number, roll: number): [number, number] {
  let y = yaw, p = pitch;
  const f = (y: number, p: number): V3 => {
    const r = mul(ypr(y, p, roll), q);
    return [r[0] - d[0], r[1] - d[1], r[2] - d[2]];
  };
  for (let it = 0; it < 12; it++) {
    const r = f(y, p);
    if (norm(r) < 1e-10) break;
    const h = 1e-4;
    const a1 = f(y + h, p), a2 = f(y, p + h);
    const jy: V3 = [(a1[0] - r[0]) / h, (a1[1] - r[1]) / h, (a1[2] - r[2]) / h];
    const jp: V3 = [(a2[0] - r[0]) / h, (a2[1] - r[1]) / h, (a2[2] - r[2]) / h];
    const A = dot(jy, jy), B = dot(jy, jp), C = dot(jp, jp);
    const gy = dot(jy, r), gp = dot(jp, r);
    const det = A * C - B * B;
    if (Math.abs(det) < 1e-18) break;
    const dy = (C * gy - B * gp) / det;
    const dp = (A * gp - B * gy) / det;
    y -= Math.max(-30, Math.min(30, dy));
    p = Math.max(-90, Math.min(90, p - Math.max(-30, Math.min(30, dp))));
  }
  return [wrapDeg(y), p];
}

function rotationBetween(a: V3, b: V3): M3 {
  const an = normalize(a), bn = normalize(b);
  const v = cross(an, bn);
  const c = dot(an, bn);
  if (c < -0.999999) return [1, 0, 0, 0, -1, 0, 0, 0, -1];
  const k = 1 / (1 + c);
  return [
    v[0] * v[0] * k + c, v[0] * v[1] * k - v[2], v[0] * v[2] * k + v[1],
    v[1] * v[0] * k + v[2], v[1] * v[1] * k + c, v[1] * v[2] * k - v[0],
    v[2] * v[0] * k - v[1], v[2] * v[1] * k + v[0], v[2] * v[2] * k + c,
  ];
}

/** Rotate the view so the great circle through world dirs a, b becomes horizontal / vertical. */
export function level(view: { yaw: number; pitch: number; roll: number }, a: V3, b: V3, horizon: boolean): [number, number, number] | null {
  let n = cross(a, b);
  if (norm(n) < 1e-9) return null;
  n = normalize(n);
  const R = ypr(view.yaw, view.pitch, view.roll);
  const V = transpose(R);
  const nv = mul(V, n);
  let t: V3;
  if (horizon) t = [0, nv[1] >= 0 ? 1 : -1, 0];
  else {
    const h: V3 = [nv[0], 0, nv[2]];
    if (norm(h) < 1e-9) return null;
    t = normalize(h);
  }
  const C = rotationBetween(nv, t);
  const newV = mmul(C, V);
  return yprFromRotation(transpose(newV));
}

// ---------------------------------------------------------------------------------- lenses

export function lensRay(lens: LensType, x: number, y: number, f: number): V3 {
  if (lens === "Rectilinear") return normalize([x, y, f]);
  const r = Math.hypot(x, y);
  if (r < 1e-12) return [0, 0, 1];
  let theta: number;
  if (lens === "FisheyeEquidistant") theta = r / f;
  else if (lens === "FisheyeEquisolid") theta = 2 * Math.asin(Math.min(1, r / (2 * f)));
  else theta = 2 * Math.atan(r / (2 * f));
  const s = Math.sin(theta) / r;
  return [x * s, y * s, Math.cos(theta)];
}

// ----------------------------------------------------------------------------- projections

export function forward(p: Projection, d: V3): [number, number] | null {
  const [x, y, z] = d;
  switch (p) {
    case "Equirectangular": {
      const lon = Math.atan2(x, z), lat = Math.atan2(-y, Math.hypot(x, z));
      return [lon, -lat];
    }
    case "Sinusoidal": {
      const lon = Math.atan2(x, z), lat = Math.atan2(-y, Math.hypot(x, z));
      return [lon * Math.cos(lat), -lat];
    }
    case "Cylindrical": {
      const h = Math.hypot(x, z);
      if (h < 1e-9) return null;
      return [Math.atan2(x, z), y / h];
    }
    case "Mercator": {
      const lon = Math.atan2(x, z), lat = Math.atan2(-y, Math.hypot(x, z));
      if (Math.abs(lat) > 89 * D2R) return null;
      return [lon, -Math.log(Math.tan(PI / 4 + lat / 2))];
    }
    case "TransverseMercator": {
      const lon = Math.atan2(-y, z), lat = Math.atan2(x, Math.hypot(y, z));
      if (Math.abs(lat) > 89 * D2R) return null;
      return [Math.log(Math.tan(PI / 4 + lat / 2)), -lon];
    }
    case "Rectilinear":
      if (z <= 1e-6) return null;
      return [x / z, y / z];
    case "Panini": {
      const lon = Math.atan2(x, z);
      if (Math.abs(lon) >= PI * 0.97) return null;
      const h = Math.hypot(x, z);
      const s = 2 / (1 + Math.cos(lon));
      return [s * Math.sin(lon), (s * y) / h];
    }
    case "Fisheye": {
      const theta = Math.acos(Math.max(-1, Math.min(1, z)));
      const r = Math.hypot(x, y);
      if (r < 1e-12) return [0, 0];
      return [(theta * x) / r, (theta * y) / r];
    }
    case "Stereographic": {
      if (z <= -0.98) return null;
      const k = 2 / (1 + z);
      return [x * k, y * k];
    }
    case "LittlePlanet": {
      const lx = x, ly = -z, lz = y;
      if (lz <= -0.98) return null;
      const k = 2 / (1 + lz);
      return [lx * k, ly * k];
    }
    case "Orthographic":
      if (z < 0) return null;
      return [x, y];
  }
}

function invStereo(u: number, v: number): V3 {
  const r2 = (u * u + v * v) / 4;
  const d = 1 + r2;
  return [u / d, v / d, (1 - r2) / d];
}

export function inverse(p: Projection, u: number, v: number): V3 | null {
  switch (p) {
    case "Equirectangular": {
      if (Math.abs(v) > HALF_PI || Math.abs(u) > PI) return null;
      const lat = -v;
      return [Math.cos(lat) * Math.sin(u), -Math.sin(lat), Math.cos(lat) * Math.cos(u)];
    }
    case "Sinusoidal": {
      if (Math.abs(v) > HALF_PI) return null;
      const lat = -v, c = Math.cos(lat);
      if (c < 1e-9) return null;
      const lon = u / c;
      if (Math.abs(lon) > PI) return null;
      return [c * Math.sin(lon), -Math.sin(lat), c * Math.cos(lon)];
    }
    case "Cylindrical":
      if (Math.abs(u) > PI) return null;
      return normalize([Math.sin(u), v, Math.cos(u)]);
    case "Mercator": {
      if (Math.abs(u) > PI) return null;
      const lat = 2 * Math.atan(Math.exp(-v)) - HALF_PI;
      return [Math.cos(lat) * Math.sin(u), -Math.sin(lat), Math.cos(lat) * Math.cos(u)];
    }
    case "TransverseMercator": {
      if (Math.abs(v) > PI) return null;
      const lat = 2 * Math.atan(Math.exp(u)) - HALF_PI, lon = -v, c = Math.cos(lat);
      return [Math.sin(lat), -c * Math.sin(lon), c * Math.cos(lon)];
    }
    case "Rectilinear":
      return normalize([u, v, 1]);
    case "Panini": {
      const lon = 2 * Math.atan(u / 2);
      const s = 2 / (1 + Math.cos(lon));
      return normalize([Math.sin(lon), v / s, Math.cos(lon)]);
    }
    case "Fisheye": {
      const theta = Math.hypot(u, v);
      if (theta > PI) return null;
      if (theta < 1e-12) return [0, 0, 1];
      const s = Math.sin(theta) / theta;
      return [u * s, v * s, Math.cos(theta)];
    }
    case "Stereographic":
      return invStereo(u, v);
    case "LittlePlanet": {
      const l = invStereo(u, v);
      return [l[0], l[2], -l[1]];
    }
    case "Orthographic": {
      const r2 = u * u + v * v;
      if (r2 > 1) return null;
      return [u, v, Math.sqrt(1 - r2)];
    }
  }
}

export function wraps(p: Projection): boolean {
  return p === "Equirectangular" || p === "Cylindrical" || p === "Mercator" || p === "Sinusoidal";
}

// ------------------------------------------------------------------------------- auto crop

/** Largest axis-aligned rectangle of covered cells (mask value > 0). Returns [x, y, w, h]. */
export function largestRect(mask: Uint8Array, w: number, h: number): [number, number, number, number] | null {
  const heights = new Int32Array(w);
  let best = [0, 0, 0, 0, 0];
  const stack: number[] = [];
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) heights[x] = mask[y * w + x] ? heights[x] + 1 : 0;
    stack.length = 0;
    for (let x = 0; x <= w; x++) {
      const cur = x < w ? heights[x] : 0;
      while (stack.length && heights[stack[stack.length - 1]] > cur) {
        const top = stack.pop()!;
        const hh = heights[top];
        const left = stack.length ? stack[stack.length - 1] + 1 : 0;
        const ww = x - left;
        if (hh * ww > best[0]) best = [hh * ww, left, y + 1 - hh, ww, hh];
      }
      stack.push(x);
    }
  }
  if (best[0] === 0) return null;
  return [best[1], best[2], best[3], best[4]];
}
