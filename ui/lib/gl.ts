// WebGL2 renderer: reprojects the equirectangular sphere map into any projection per screen pixel.

import { PROJ_INDEX, type M3, type Projection } from "./geometry";

const VS = `#version 300 es
in vec2 aPos;
void main() { gl_Position = vec4(aPos, 0.0, 1.0); }`;

const FS = `#version 300 es
precision highp float;
uniform sampler2D uTex;
uniform vec2 uViewport;     // device px
uniform vec2 uOrigin;       // device px, top-left of content rect
uniform float uK;           // device px per plane unit
uniform vec4 uBounds;       // u0, v0, u1, v1
uniform int uProj;
uniform mat3 uR;            // view -> world
uniform vec2 uFlip;         // 1 = mirrored
uniform vec4 uSph;          // u0, v0, scale, wraps
uniform vec2 uSphSize;
uniform int uMode;          // 0 = display, 1 = coverage mask
uniform float uChecker;     // checker cell size in device px
uniform vec3 uBg;
out vec4 outColor;

const float PI = 3.14159265358979;
const float HALF_PI = 1.57079632679490;

vec3 invStereo(float u, float v) {
  float r2 = (u*u + v*v) / 4.0;
  float d = 1.0 + r2;
  return vec3(u/d, v/d, (1.0 - r2)/d);
}

// returns w = 0 when invalid
vec4 inverseProj(int p, float u, float v) {
  if (p == 0) { // equirectangular
    if (abs(v) > HALF_PI || abs(u) > PI) return vec4(0.0);
    float lat = -v;
    return vec4(cos(lat)*sin(u), -sin(lat), cos(lat)*cos(u), 1.0);
  } else if (p == 1) { // cylindrical
    if (abs(u) > PI) return vec4(0.0);
    return vec4(normalize(vec3(sin(u), v, cos(u))), 1.0);
  } else if (p == 2) { // mercator
    if (abs(u) > PI) return vec4(0.0);
    float lat = 2.0*atan(exp(-v)) - HALF_PI;
    return vec4(cos(lat)*sin(u), -sin(lat), cos(lat)*cos(u), 1.0);
  } else if (p == 3) { // rectilinear
    return vec4(normalize(vec3(u, v, 1.0)), 1.0);
  } else if (p == 4) { // panini
    float lon = 2.0*atan(u/2.0);
    float s = 2.0/(1.0 + cos(lon));
    return vec4(normalize(vec3(sin(lon), v/s, cos(lon))), 1.0);
  } else if (p == 5) { // fisheye
    float th = sqrt(u*u + v*v);
    if (th > PI) return vec4(0.0);
    if (th < 1e-9) return vec4(0.0, 0.0, 1.0, 1.0);
    float s = sin(th)/th;
    return vec4(u*s, v*s, cos(th), 1.0);
  } else if (p == 6) { // stereographic
    return vec4(invStereo(u, v), 1.0);
  } else if (p == 7) { // little planet
    vec3 l = invStereo(u, v);
    return vec4(l.x, l.z, -l.y, 1.0);
  } else if (p == 8) { // orthographic
    float r2 = u*u + v*v;
    if (r2 > 1.0) return vec4(0.0);
    return vec4(u, v, sqrt(1.0 - r2), 1.0);
  } else if (p == 9) { // transverse mercator
    if (abs(v) > PI) return vec4(0.0);
    float lat = 2.0*atan(exp(u)) - HALF_PI;
    float lon = -v;
    float c = cos(lat);
    return vec4(sin(lat), -c*sin(lon), c*cos(lon), 1.0);
  } else { // sinusoidal
    if (abs(v) > HALF_PI) return vec4(0.0);
    float lat = -v;
    float c = cos(lat);
    if (c < 1e-9) return vec4(0.0);
    float lon = u / c;
    if (abs(lon) > PI) return vec4(0.0);
    return vec4(c*sin(lon), -sin(lat), c*cos(lon), 1.0);
  }
}

void main() {
  vec2 sp = vec2(gl_FragCoord.x, uViewport.y - gl_FragCoord.y);
  vec2 rel = (sp - uOrigin) / uK;           // plane units from top-left
  vec2 size = uBounds.zw - uBounds.xy;
  if (rel.x < 0.0 || rel.y < 0.0 || rel.x > size.x || rel.y > size.y) {
    outColor = uMode == 1 ? vec4(0.0) : vec4(uBg, 1.0);
    return;
  }
  if (uFlip.x > 0.5) rel.x = size.x - rel.x;
  if (uFlip.y > 0.5) rel.y = size.y - rel.y;
  float u = uBounds.x + rel.x;
  float v = uBounds.y + rel.y;
  vec4 d = inverseProj(uProj, u, v);
  float cov = 0.0;
  vec3 col = vec3(0.0);
  if (d.w > 0.5) {
    vec3 w = uR * d.xyz;
    float lon = atan(w.x, w.z);
    float lat = atan(-w.y, length(w.xz));
    float sx = (lon - uSph.x) * uSph.z;
    float sy = (-lat - uSph.y) * uSph.z;
    bool inside = sy >= -0.5 && sy <= uSphSize.y - 0.5;
    if (uSph.w < 0.5) inside = inside && sx >= -0.5 && sx <= uSphSize.x - 0.5;
    if (inside) {
      vec4 t = texture(uTex, vec2((sx + 0.5) / uSphSize.x, (sy + 0.5) / uSphSize.y));
      cov = t.a;
      col = t.rgb;
    }
  }
  if (uMode == 1) { outColor = vec4(cov > 0.98 ? 1.0 : 0.0, 0.0, 0.0, 1.0); return; }
  vec2 cell = floor(sp / uChecker);
  float lb = dot(uBg, vec3(0.299, 0.587, 0.114));
  vec3 chk = mod(cell.x + cell.y, 2.0) < 1.0 ? uBg + (lb > 0.5 ? -0.05 : 0.045) : uBg + (lb > 0.5 ? -0.1 : 0.085);
  outColor = vec4(col + chk * (1.0 - cov), 1.0);
}`;

export interface SphereMeta {
  u0: number;
  v0: number;
  scale: number;
  w: number;
  h: number;
  wraps: boolean;
}

export interface DrawParams {
  originX: number; // device px
  originY: number;
  k: number; // device px per plane unit
  bounds: [number, number, number, number];
  projection: Projection;
  R: M3; // view -> world, row-major
  flipH: boolean;
  flipV: boolean;
  dpr: number;
  bg?: [number, number, number];
}

export class SphereRenderer {
  gl: WebGL2RenderingContext;
  prog: WebGLProgram;
  tex: WebGLTexture | null = null;
  sphere: SphereMeta | null = null;
  loc: Record<string, WebGLUniformLocation | null> = {};
  maxTex: number;

  constructor(public canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", { antialias: false, preserveDrawingBuffer: false, premultipliedAlpha: false });
    if (!gl) throw new Error("WebGL2 not available");
    this.gl = gl;
    this.maxTex = gl.getParameter(gl.MAX_TEXTURE_SIZE);
    const sh = (type: number, src: string) => {
      const s = gl.createShader(type)!;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) ?? "shader");
      return s;
    };
    const p = gl.createProgram()!;
    gl.attachShader(p, sh(gl.VERTEX_SHADER, VS));
    gl.attachShader(p, sh(gl.FRAGMENT_SHADER, FS));
    gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p) ?? "link");
    this.prog = p;
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]), gl.STATIC_DRAW);
    const a = gl.getAttribLocation(p, "aPos");
    gl.enableVertexAttribArray(a);
    gl.vertexAttribPointer(a, 2, gl.FLOAT, false, 0, 0);
    for (const n of ["uTex", "uViewport", "uOrigin", "uK", "uBounds", "uProj", "uR", "uFlip", "uSph", "uSphSize", "uMode", "uChecker", "uBg"]) {
      this.loc[n] = gl.getUniformLocation(p, n);
    }
  }

  setSphere(meta: SphereMeta, pixels: Uint8Array) {
    const gl = this.gl;
    if (this.tex) gl.deleteTexture(this.tex);
    const t = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, t);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, meta.w, meta.h, 0, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, meta.wraps ? gl.REPEAT : gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    this.tex = t;
    this.sphere = meta;
  }

  clearSphere() {
    if (this.tex) this.gl.deleteTexture(this.tex);
    this.tex = null;
    this.sphere = null;
  }

  private setUniforms(p: DrawParams, vw: number, vh: number, mode: number) {
    const gl = this.gl;
    const s = this.sphere!;
    gl.useProgram(this.prog);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.uniform1i(this.loc.uTex, 0);
    gl.uniform2f(this.loc.uViewport, vw, vh);
    gl.uniform2f(this.loc.uOrigin, p.originX, p.originY);
    gl.uniform1f(this.loc.uK, p.k);
    gl.uniform4f(this.loc.uBounds, p.bounds[0], p.bounds[1], p.bounds[2], p.bounds[3]);
    gl.uniform1i(this.loc.uProj, PROJ_INDEX[p.projection]);
    // GLSL mat3 is column-major: pass transpose flag false with column-major data
    const R = p.R;
    gl.uniformMatrix3fv(this.loc.uR, false, new Float32Array([R[0], R[3], R[6], R[1], R[4], R[7], R[2], R[5], R[8]]));
    gl.uniform2f(this.loc.uFlip, p.flipH ? 1 : 0, p.flipV ? 1 : 0);
    gl.uniform4f(this.loc.uSph, s.u0, s.v0, s.scale, s.wraps ? 1 : 0);
    gl.uniform2f(this.loc.uSphSize, s.w, s.h);
    gl.uniform1i(this.loc.uMode, mode);
    gl.uniform1f(this.loc.uChecker, 10 * p.dpr);
    const bg = p.bg ?? [0, 0, 0];
    gl.uniform3f(this.loc.uBg, bg[0], bg[1], bg[2]);
  }

  draw(p: DrawParams) {
    const gl = this.gl;
    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    if (!this.sphere || !this.tex) {
      const bg = p.bg ?? [0, 0, 0];
      gl.clearColor(bg[0], bg[1], bg[2], 1);
      gl.clear(gl.COLOR_BUFFER_BIT);
      return;
    }
    this.setUniforms(p, this.canvas.width, this.canvas.height, 0);
    gl.drawArrays(gl.TRIANGLES, 0, 6);
  }

  /** Coverage mask of the whole content rect at (w x h) resolution, in display orientation. */
  coverage(p: DrawParams, w: number, h: number): Uint8Array | null {
    if (!this.sphere || !this.tex) return null;
    const gl = this.gl;
    const fb = gl.createFramebuffer();
    const rt = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, rt);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
    gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, rt, 0);
    gl.viewport(0, 0, w, h);
    const k = w / (p.bounds[2] - p.bounds[0]);
    this.setUniforms({ ...p, originX: 0, originY: 0, k }, w, h, 1);
    gl.drawArrays(gl.TRIANGLES, 0, 6);
    const px = new Uint8Array(w * h * 4);
    gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, px);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.deleteFramebuffer(fb);
    gl.deleteTexture(rt);
    // readPixels is bottom-up; convert to top-down single channel
    const out = new Uint8Array(w * h);
    for (let y = 0; y < h; y++) {
      const src = (h - 1 - y) * w * 4;
      for (let x = 0; x < w; x++) out[y * w + x] = px[src + x * 4] > 127 ? 1 : 0;
    }
    return out;
  }
}
