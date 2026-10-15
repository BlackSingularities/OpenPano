// Panorama "types" shown to the user, and how each projection is steered.
//
// Rotation model: render.yaw/pitch/roll is the total view rotation sent to the engine. The pitch
// and roll that make the horizon level are remembered separately (render.level_pitch/level_roll),
// so for view-type projections "look up/down" and "rotate frame" are offsets from a level view and
// switching the panorama type never leaves a tilted, wavy horizon behind.
import type { Projection } from "./geometry";

export type Family = "sphere" | "strip" | "view" | "vertical" | "planet" | "fisheye";

/** strip: unrolled band (centre + horizon level) · view: camera with a direction and field of
 *  view · planet: little planet / tunnel. */
export type Kind = "strip" | "view" | "planet";

export const FAMILIES: { id: Family; projections: Projection[] }[] = [
  { id: "sphere", projections: ["Equirectangular", "Sinusoidal"] },
  { id: "strip", projections: ["Cylindrical", "Mercator"] },
  { id: "view", projections: ["Rectilinear", "Panini"] },
  { id: "vertical", projections: ["TransverseMercator"] },
  { id: "planet", projections: ["LittlePlanet"] },
  { id: "fisheye", projections: ["Fisheye", "Stereographic", "Orthographic"] },
];

export function familyOf(p: Projection): Family {
  return FAMILIES.find((f) => f.projections.includes(p))?.id ?? "strip";
}

export function kindOf(p: Projection): Kind {
  switch (p) {
    case "Equirectangular":
    case "Sinusoidal":
    case "Cylindrical":
    case "Mercator":
      return "strip";
    case "LittlePlanet":
      return "planet";
    default:
      return "view";
  }
}

/** Projections whose canvas is a window with an adjustable field of view (mirrors the engine). */
export function hasFov(p: Projection): boolean {
  return ["Rectilinear", "Panini", "Fisheye", "Stereographic", "LittlePlanet", "Orthographic"].includes(p);
}

/** Sensible slider range for the field of view, degrees. */
export function fovRange(p: Projection): [number, number] {
  switch (p) {
    case "Rectilinear":
      return [20, 160];
    case "Panini":
      return [40, 300];
    case "Orthographic":
      return [30, 180];
    case "LittlePlanet":
      return [90, 330];
    default:
      return [60, 340];
  }
}

/** Inverse of the engine's half_extent: plane coordinate -> angle from the centre (radians). */
function halfAngle(p: Projection, e: number): number {
  switch (p) {
    case "Rectilinear":
      return Math.atan(e);
    case "Fisheye":
      return e;
    case "Stereographic":
    case "LittlePlanet":
      return 2 * Math.atan(e / 2);
    case "Orthographic":
      return Math.asin(Math.min(1, e));
    case "Panini": {
      // s·sin(a) with s = 2/(1+cos a) = 2·tan(a/2)
      return 2 * Math.atan(e / 2);
    }
    default:
      return 0;
  }
}

/** Field of view (degrees) currently shown by a layout, for seeding the manual slider. */
export function fovOfLayout(p: Projection, L: { u0: number; u1: number; v0: number; v1: number }): number {
  const e = Math.max(Math.abs(L.u0), Math.abs(L.u1), Math.abs(L.v0), Math.abs(L.v1));
  const [lo, hi] = fovRange(p);
  return Math.round(Math.min(hi, Math.max(lo, (2 * halfAngle(p, e) * 180) / Math.PI)));
}

export const wrap180 = (v: number) => ((((v + 180) % 360) + 360) % 360) - 180;
