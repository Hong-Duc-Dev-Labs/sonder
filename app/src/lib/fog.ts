// Fog Backdrop utilities ported from egoist/mygo-music:
// HSV color transformation, dominant tint expansion, volumetric shader program,
// and audio-reactive bass pulse integrator.

export function hexToRgb(hex: string): [number, number, number] {
  const clean = hex.replace("#", "").trim();
  if (clean.length === 6) {
    const num = parseInt(clean, 16);
    if (!Number.isNaN(num)) {
      return [(num >> 16) / 255, ((num >> 8) & 0xff) / 255, (num & 0xff) / 255];
    }
  }
  if (clean.length === 3) {
    const r = parseInt(clean[0] + clean[0], 16);
    const g = parseInt(clean[1] + clean[1], 16);
    const b = parseInt(clean[2] + clean[2], 16);
    if (!Number.isNaN(r) && !Number.isNaN(g) && !Number.isNaN(b)) {
      return [r / 255, g / 255, b / 255];
    }
  }
  return [0.2, 0.2, 0.25];
}

export function rgbToHsv(r: number, g: number, b: number): [number, number, number] {
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const d = max - min;
  const v = max;
  if (max === 0) return [0, 0, 0];
  const s = d / max;
  if (d === 0) return [0, 0, v];

  let h = 0;
  if (max === r) {
    h = ((g - b) / d + (g < b ? 6 : 0)) * 60;
  } else if (max === g) {
    h = ((b - r) / d + 2) * 60;
  } else {
    h = ((r - g) / d + 4) * 60;
  }
  return [h, s, v];
}

export function hsvToRgb(h: number, s: number, v: number): [number, number, number] {
  const c = v * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  let r = 0;
  let g = 0;
  let b = 0;

  if (h < 60) {
    r = c;
    g = x;
  } else if (h < 120) {
    r = x;
    g = c;
  } else if (h < 180) {
    g = c;
    b = x;
  } else if (h < 240) {
    g = x;
    b = c;
  } else if (h < 300) {
    r = x;
    b = c;
  } else {
    r = c;
    b = x;
  }

  const m = v - c;
  return [r + m, g + m, b + m];
}

export type FogColors = {
  deep: [number, number, number, number];
  fog: [number, number, number, number];
};

/**
 * Calculates `deep` and `fog` shades according to MyGo Music's exact color balance:
 * - `deep`: Rich dark background (v ≈ 0.30).
 * - `fog`: Billowing illuminated cloud layer (v ≈ 0.72).
 * Ensures white typography remains legible across any cover artwork.
 */
export function fogColors(tintHex: string): FogColors {
  const [r, g, b] = hexToRgb(tintHex);
  const [h, rawS] = rgbToHsv(r, g, b);

  let s = rawS;
  if (s > 0.12) {
    s = Math.min(Math.max(s * 1.3, 0.6), 0.85);
  }

  const [dr, dg, db] = hsvToRgb(h, s, 0.3);
  const [fr, fg, fb] = hsvToRgb(h, s * 0.6, 0.72);

  return {
    deep: [dr, dg, db, 1],
    fog: [fr, fg, fb, 1],
  };
}

export const FOG_VERTEX_SHADER = `
attribute vec2 a_position;
void main() {
  gl_Position = vec4(a_position, 0.0, 1.0);
}
`;

export const FOG_FRAGMENT_SHADER = `
precision mediump float;

uniform float u_time;
uniform float u_bass;
uniform vec4 u_deep;
uniform vec4 u_fog;
uniform vec2 u_resolution;

float hash(vec2 p) {
  vec3 p3 = fract(vec3(p.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  float a = hash(i);
  float b = hash(i + vec2(1.0, 0.0));
  float c = hash(i + vec2(0.0, 1.0));
  float d = hash(i + vec2(1.0, 1.0));
  return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

float fbm(vec2 p) {
  float v = 0.0;
  float amp = 0.5;
  for (int i = 0; i < 5; i++) {
    v += amp * noise(p);
    p = p * 2.03 + vec2(1.7, 9.2);
    amp *= 0.5;
  }
  return v;
}

void main() {
  vec2 pos = gl_FragCoord.xy;
  vec2 size = u_resolution;
  vec2 uv = pos / size;
  vec2 p = pos / size.y * 2.4;
  float t = u_time;
  vec2 rise = vec2(0.0, t * 0.1);

  vec2 q = vec2(
    fbm(p + rise + vec2(0.0, t * 0.04)),
    fbm(p + rise + vec2(5.2, 1.3) - vec2(t * 0.05, 0.0))
  );
  float f = fbm(p + rise + 0.9 * q);
  float bank = fbm(p * 0.45 + rise * 0.6 + vec2(3.7, 0.0));
  float cloud = smoothstep(0.3 - 0.06 * u_bass, 0.78, f) * (0.45 + 0.75 * smoothstep(0.3, 0.65, bank));

  float m = smoothstep(-0.1, 0.4, uv.y) * (1.0 - 0.5 * smoothstep(0.65, 1.0, uv.y));
  vec3 c = mix(u_deep.rgb, u_fog.rgb, min(cloud * m * (0.85 + 0.35 * u_bass), 1.0));

  float g = length((uv - vec2(0.5, 0.42)) * vec2(1.0, 1.25));
  c *= 1.08 - 0.45 * smoothstep(0.2, 0.85, g);

  float cell = size.y / 22.0;
  vec2 sp = (pos + vec2(0.0, t * cell * 0.9)) / cell;
  vec2 id = floor(sp);
  float h = hash(id + vec2(3.1, 7.7));
  if (h > 0.93) {
    vec2 at = vec2(hash(id + vec2(1.3, 2.9)), hash(id + vec2(8.1, 4.3))) * 0.8 + 0.1;
    float d = length((fract(sp) - at) * cell) / max(cell * 0.05, 0.7);
    float glint = 0.5 + 0.5 * sin(t * (1.5 + 2.0 * h) + h * 40.0);
    c += vec3(0.75, 0.82, 1.0) * exp(-d * d) * glint * (0.25 + 0.3 * m);
  }

  gl_FragColor = vec4(c, 1.0);
}
`;

/**
 * Creates a WebGL shader program for the Fog Backdrop.
 */
export function createFogRenderer(canvas: HTMLCanvasElement) {
  const gl = canvas.getContext("webgl", {
    alpha: false,
    antialias: false,
    depth: false,
    stencil: false,
    powerPreference: "low-power",
  });
  if (!gl) return null;

  function compileShader(type: number, source: string) {
    const shader = gl!.createShader(type);
    if (!shader) return null;
    gl!.shaderSource(shader, source);
    gl!.compileShader(shader);
    if (!gl!.getShaderParameter(shader, gl!.COMPILE_STATUS)) {
      gl!.deleteShader(shader);
      return null;
    }
    return shader;
  }

  const vert = compileShader(gl.VERTEX_SHADER, FOG_VERTEX_SHADER);
  const frag = compileShader(gl.FRAGMENT_SHADER, FOG_FRAGMENT_SHADER);
  if (!vert || !frag) {
    if (vert) gl.deleteShader(vert);
    if (frag) gl.deleteShader(frag);
    return null;
  }

  const prog = gl.createProgram();
  if (!prog) {
    gl.deleteShader(vert);
    gl.deleteShader(frag);
    return null;
  }
  gl.attachShader(prog, vert);
  gl.attachShader(prog, frag);
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
    gl.deleteProgram(prog);
    gl.deleteShader(vert);
    gl.deleteShader(frag);
    return null;
  }

  gl.useProgram(prog);

  const posLoc = gl.getAttribLocation(prog, "a_position");
  const timeLoc = gl.getUniformLocation(prog, "u_time");
  const bassLoc = gl.getUniformLocation(prog, "u_bass");
  const deepLoc = gl.getUniformLocation(prog, "u_deep");
  const fogLoc = gl.getUniformLocation(prog, "u_fog");
  const resLoc = gl.getUniformLocation(prog, "u_resolution");

  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(
    gl.ARRAY_BUFFER,
    new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
    gl.STATIC_DRAW,
  );
  gl.enableVertexAttribArray(posLoc);
  gl.vertexAttribPointer(posLoc, 2, gl.FLOAT, false, 0, 0);

  return {
    render(
      width: number,
      height: number,
      time: number,
      bass: number,
      deep: [number, number, number, number],
      fog: [number, number, number, number],
    ) {
      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
        gl.viewport(0, 0, width, height);
      }
      gl.useProgram(prog);
      gl.uniform1f(timeLoc, time);
      gl.uniform1f(bassLoc, Math.min(Math.max(bass, 0), 1));
      gl.uniform4fv(deepLoc, deep);
      gl.uniform4fv(fogLoc, fog);
      gl.uniform2f(resLoc, width, height);
      gl.drawArrays(gl.TRIANGLES, 0, 6);
    },
    destroy() {
      gl.deleteBuffer(buf);
      gl.deleteProgram(prog);
      gl.deleteShader(vert);
      gl.deleteShader(frag);
      gl.getExtension("WEBGL_lose_context")?.loseContext();
    },
  };
}

/**
 * Creates an audio-reactive bass pulse state using MyGo's barState smoothing math:
 * - Rapid attack (k = 18)
 * - Gentle decay (k = 6)
 */
export function createBassPulseState() {
  let value = 0;
  return {
    get current() {
      return value;
    },
    step(dt: number, playing: boolean, timeMs: number) {
      let target = 0;
      if (playing) {
        // Natural kick-bass pulse on ~500ms intervals (120 BPM) with sub-bass undulating
        const safeMs = Math.max(0, timeMs);
        const beatPhase = (safeMs % 500) / 500;
        const kick = Math.pow(Math.max(0, 1 - beatPhase * 3.5), 2);
        const sub = 0.15 + 0.12 * Math.sin(safeMs * 0.005);
        target = Math.min(1, Math.max(0, kick * 0.78 + sub));
      }
      const k = target > value ? 18 : 6;
      value += (target - value) * Math.min(1, k * dt);
      return value;
    },
  };
}
