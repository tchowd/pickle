import { useEffect, useRef } from 'react';
import plasma from './artwork/plasma.webp';

const vertex = `
  attribute vec2 position;
  varying vec2 uv;
  void main() {
    uv = .5 * (position + 1.);
    gl_Position = vec4(position, 0., 1.);
  }
`;

const fragment = `
  precision highp float;
  varying vec2 uv;
  uniform sampler2D plasma;
  uniform float aspect, elapsed, transit;
  uniform vec2 texel;
  uniform float pixelHeight;
  float luminance(vec3 color) {
    return dot(color, vec3(.2126, .7152, .0722));
  }
  void main() {
    vec2 p = vec2(uv.x - .5, .5 - uv.y) * vec2(aspect, 1.);
    float radius = length(p);
    float throat = 1. - smoothstep(.055, .72, radius);
    float turn = smoothstep(0., 1., transit);
    float shear = sin(elapsed * .24) * .28 + turn * .88;
    float angle = elapsed * .12 + turn * 6.283185 + throat * shear;
    float ripple = sin(radius * 28. - elapsed * .65 - turn * 9.)
                 * sin(elapsed * .28 + turn * 2.) * .009;
    mat2 rotate = mat2(cos(angle), -sin(angle), sin(angle), cos(angle));
    vec2 samplePoint = .5 + rotate * p * (.60 + ripple) / (1. + turn * .95);
    vec3 light = texture2D(plasma, samplePoint).rgb;
    // Sharpen in luminance, keeping the colour of the painted membrane.
    vec2 stepUV = texel * 1.45;
    vec3 neighbours = .25 * (
      texture2D(plasma, samplePoint + vec2(stepUV.x, 0.)).rgb +
      texture2D(plasma, samplePoint - vec2(stepUV.x, 0.)).rgb +
      texture2D(plasma, samplePoint + vec2(0., stepUV.y)).rgb +
      texture2D(plasma, samplePoint - vec2(0., stepUV.y)).rgb);
    float value = luminance(light);
    float edge = clamp(value - luminance(neighbours), -.045, .045);
    light *= 1. + .85 * edge / max(value, .075);

    // Fine strands follow the turning sheets, breaking into highlights along
    // the bright folds. Suppress subpixel strands in the distant view.
    vec2 flow = samplePoint - .5;
    float r = max(length(flow), .014);
    float a = atan(flow.y, flow.x);
    float fold = log(r) * 53. + 3. * a + 2.4 * sin(5. * a + 18. * r)
               + .9 * sin(13. * a - 24. * r) + .5 * sin(27. * a + 37. * r);
    float filament = pow(.5 + .5 * cos(fold), 16.);
    float resolved = smoothstep(8., 22., pixelHeight * r / 6.);
    float glint = filament * resolved * smoothstep(.11, .65, value)
                * smoothstep(-.4, .8, sin(9. * a + 24. * r));
    light += vec3(.105, .12, .059) * glint;
    // Preserve luminous ridges while letting overlapping membranes occlude light.
    light *= .96 + .04 * cos(radius * 16. - elapsed * .8);
    light += light * light * vec3(.06, .12, .035) * (.4 + turn);
    gl_FragColor = vec4(light, 1.);
  }
`;

function createRenderer(canvas, image) {
  const gl = canvas.getContext('webgl', { alpha: false, antialias: false, depth: false });
  if (!gl) return null;
  const program = gl.createProgram();
  for (const [type, code] of [[gl.VERTEX_SHADER, vertex], [gl.FRAGMENT_SHADER, fragment]]) {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, code);
    gl.compileShader(shader);
    gl.attachShader(program, shader);
    gl.deleteShader(shader);
  }
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    gl.deleteProgram(program);
    return null;
  }
  gl.useProgram(program);
  const triangle = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, triangle);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1, 3,-1, -1,3]), gl.STATIC_DRAW);
  const position = gl.getAttribLocation(program, 'position');
  gl.enableVertexAttribArray(position);
  gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);
  const texture = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGB, gl.RGB, gl.UNSIGNED_BYTE, image);
  const aspect = gl.getUniformLocation(program, 'aspect');
  const elapsed = gl.getUniformLocation(program, 'elapsed');
  const transit = gl.getUniformLocation(program, 'transit');
  const pixelHeight = gl.getUniformLocation(program, 'pixelHeight');
  gl.uniform2f(gl.getUniformLocation(program, 'texel'), 1 / image.naturalWidth, 1 / image.naturalHeight);
  return {
    draw(seconds, progress) {
      gl.viewport(0, 0, canvas.width, canvas.height);
      gl.uniform1f(aspect, canvas.width / canvas.height);
      gl.uniform1f(elapsed, seconds);
      gl.uniform1f(transit, progress);
      gl.uniform1f(pixelHeight, canvas.height);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    },
    dispose() {
      gl.deleteTexture(texture);
      gl.deleteBuffer(triangle);
      gl.deleteProgram(program);
    },
  };
}

export function PortalOptics({ running, entering, duration }) {
  const canvas = useRef(null);
  const redraw = useRef(null);
  const motion = useRef({ running, entering, duration });
  useEffect(() => {
    motion.current = { running, entering, duration };
    redraw.current?.();
  }, [running, entering, duration]);

  useEffect(() => {
    const surface = canvas.current;
    const image = new Image();
    const preference = matchMedia('(prefers-reduced-motion: reduce)');
    let renderer, frame, lastTime, entryStart;
    let seconds = 0;
    let disposed = false;

    function draw(now) {
      if (!renderer || document.hidden) return;
      const { entering, running, duration } = motion.current;
      const animated = !preference.matches && (running || entering);
      if (running && animated && lastTime !== undefined) seconds += Math.min(now - lastTime, 100) / 1000;
      lastTime = animated ? now : undefined;
      entryStart = entering ? entryStart ?? now : undefined;
      const progress = entering ? Math.min((now - entryStart) / duration, 1) : 0;
      renderer.draw(seconds, progress);
      if (animated && (!entering || progress < 1)) frame = requestAnimationFrame(draw);
    }
    function refresh() {
      cancelAnimationFrame(frame);
      if (!renderer) return;
      const scale = motion.current.entering
        ? Number(getComputedStyle(surface).getPropertyValue('--zoom')) || 1 : 1;
      const width = Math.min(2560, Math.max(1, Math.ceil(surface.clientWidth * devicePixelRatio * scale)));
      const height = Math.max(1, Math.round(width * 363 / 473));
      if (surface.width !== width || surface.height !== height) {
        surface.width = width;
        surface.height = height;
      }
      draw(performance.now());
    }
    function initialize() {
      if (disposed || !image.naturalWidth) return;
      renderer = createRenderer(surface, image);
      refresh();
      surface.style.opacity = renderer ? '1' : '0';
    }
    function lost(event) {
      event.preventDefault();
      cancelAnimationFrame(frame);
      renderer = null;
      surface.style.opacity = '0';
    }
    image.onload = initialize;
    image.src = plasma;
    const observer = new ResizeObserver(refresh);
    observer.observe(surface);
    preference.addEventListener('change', refresh);
    document.addEventListener('visibilitychange', refresh);
    surface.addEventListener('webglcontextlost', lost);
    surface.addEventListener('webglcontextrestored', initialize);
    redraw.current = refresh;
    return () => {
      disposed = true;
      redraw.current = null;
      cancelAnimationFrame(frame);
      observer.disconnect();
      preference.removeEventListener('change', refresh);
      document.removeEventListener('visibilitychange', refresh);
      surface.removeEventListener('webglcontextlost', lost);
      surface.removeEventListener('webglcontextrestored', initialize);
      renderer?.dispose();
    };
  }, []);
  return (
    <div className="portal-optics" aria-hidden="true">
      <span className="plasma-film" style={{ backgroundImage: `url(${plasma})` }} />
      <canvas ref={canvas} />
      <span className="crt-surface" />
    </div>
  );
}
