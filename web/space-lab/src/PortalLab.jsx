import { memo, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from 'react';
import layers from './artwork/layers.json';
import { LabSurround } from './LabSurround.jsx';
import { PortalOptics } from './PortalOptics.jsx';
import rafters from './artwork/rafters.svg?url';
import cabinet from './artwork/bench-cabinet.svg?url';

const verticalSpace = window.matchMedia('(aspect-ratio < 16/9)');
function watchAspectRatio(onChange) {
  verticalSpace.addEventListener('change', onChange);
  return () => verticalSpace.removeEventListener('change', onChange);
}
const hasVerticalSpace = () => verticalSpace.matches;
const destination = './observatory.html';
const transitDuration = 1450;
const glass = { x: 814 / 2048, y: 224 / 1152, w: 473 / 2048, h: 363 / 1152 };

function PaintedExtension({ side, artwork, id, height }) {
  return (
    <div className={'extension extension-' + side} aria-hidden="true"
      style={{ '--paint-height': (height / 1024 * 100) + 'vw' }}>
      <svg viewBox={`0 0 1024 ${height}`} preserveAspectRatio="none" focusable="false">
        <use href={artwork + '#' + id} />
      </svg>
    </div>
  );
}

const Workbench = memo(function Workbench() {
  return (
    <svg viewBox="0 0 2048 1152" role="img" aria-labelledby="scene-title scene-desc">
      <title id="scene-title">Portal Lab</title>
      <desc id="scene-desc">
        An amber desk lamp lights a bench of model rockets. A green vortex fills an
        old CRT between a signal scope and a specimen chamber. A keyboard, tools
        and tangled cables cover the desk.
      </desc>
      <rect width="2048" height="1152" fill="#030408" />
      {layers.map(id => <g id={id} key={id}><use href={'#paint-' + id} /></g>)}
      <defs>
        <clipPath id="glass-aperture">
          <path id="glass-outline" d="M824 249Q827 231 910 231Q1100 224 1266 239Q1287 246 1280 476L1276 554Q1276 573 1262 574Q1036 587 834 569Q817 566 818 548Q814 393 824 249Z" />
        </clipPath>
        <clipPath id="monitor-viewport" clipPathUnits="objectBoundingBox">
          <use href="#glass-outline" transform="scale(.002114165 .002754821) translate(-814 -224)" />
        </clipPath>
      </defs>
    </svg>
  );
});

export function PortalLab() {
  const [running, setRunning] = useState(false);
  const [entering, setEntering] = useState(false);
  const [viewing, setViewing] = useState(() => location.hash === '#observatory');
  const entry = useRef(null);
  const camera = useRef(null);
  const scene = useRef(null);
  const navigationTimer = useRef(null);
  const showExtensions = useSyncExternalStore(watchAspectRatio, hasVerticalSpace);
  useLayoutEffect(() => {
    const stage = camera.current;
    const bench = scene.current;
    function frameMonitor() {
      const width = bench.offsetWidth * glass.w;
      const height = bench.offsetHeight * glass.h;
      if (!width || !height) return;
      const x = bench.offsetLeft + bench.offsetWidth * glass.x + width / 2;
      const y = bench.offsetTop + bench.offsetHeight * glass.y + height / 2;
      const zoom = Math.min(stage.clientWidth * .95 / width, stage.clientHeight * .91 / height);
      stage.style.transformOrigin = `${x}px ${y}px`;
      stage.style.setProperty('--camera-x', `${stage.clientWidth / 2 - x}px`);
      stage.style.setProperty('--camera-y', `${stage.clientHeight / 2 - y}px`);
      stage.style.setProperty('--zoom', zoom);
      stage.style.setProperty('--inverse-zoom', 1 / zoom);
    }
    frameMonitor();
    const observer = new ResizeObserver(frameMonitor);
    observer.observe(stage);
    observer.observe(bench);
    return () => observer.disconnect();
  }, []);
  function openObservatory() {
    window.clearTimeout(navigationTimer.current);
    navigationTimer.current = null;
    setViewing(true);
    setEntering(false);
    location.hash = 'observatory';
  }
  function enterPortal() {
    if (navigationTimer.current !== null) return;
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return openObservatory();
    setEntering(true);
    // A fallback also completes transit when browser settings suppress animation events.
    navigationTimer.current = window.setTimeout(openObservatory, transitDuration + 500);
  }
  useEffect(() => {
    const resetTransit = () => {
      window.clearTimeout(navigationTimer.current);
      navigationTimer.current = null;
      setEntering(false);
    };
    window.addEventListener('pageshow', resetTransit);
    window.addEventListener('pagehide', resetTransit);
    const changeScreen = () => {
      resetTransit();
      setViewing(location.hash === '#observatory');
      if (location.hash !== '#observatory') requestAnimationFrame(() => entry.current?.focus({ preventScroll: true }));
    };
    window.addEventListener('hashchange', changeScreen);
    const preference = matchMedia('(prefers-reduced-motion: reduce)');
    const visibilityChanged = () => { if (document.hidden) setRunning(false); };
    const motionChanged = event => { if (event.matches) setRunning(false); };
    document.addEventListener('visibilitychange', visibilityChanged);
    preference.addEventListener('change', motionChanged);
    return () => {
      window.clearTimeout(navigationTimer.current);
      window.removeEventListener('pageshow', resetTransit);
      window.removeEventListener('pagehide', resetTransit);
      window.removeEventListener('hashchange', changeScreen);
      document.removeEventListener('visibilitychange', visibilityChanged);
      preference.removeEventListener('change', motionChanged);
    };
  }, []);

  return (
    <main className="lab-viewport" aria-label="Portal Lab">
      <div className={`lab-camera${entering || viewing ? ' close-up' : ''}`} ref={camera}>
      {showExtensions
        ? <PaintedExtension side="above" artwork={rafters} id="rafters" height={627} />
        : <LabSurround />}
      <div ref={scene} className={`scene${running ? ' running' : ''}${entering ? ' entering' : ''}`}
        onAnimationEnd={event => { if (entering && event.animationName === 'enter-vortex') openObservatory(); }}
        style={{ '--transit-duration': transitDuration + 'ms' }}>
        <Workbench />
        {!viewing && <PortalOptics running={running} entering={entering} duration={transitDuration} />}
        {viewing ? (
          <div className="monitor-content">
            <iframe title="Deep-space observatory" src={destination}
              onLoad={event => event.currentTarget.focus({ preventScroll: true })} />
          </div>
        ) : (
          <button ref={entry} className="portal-entry" type="button" onClick={enterPortal}
            aria-label="Enter the deep-space observatory" title="Enter the portal" />
        )}
        <button type="button" className="power" aria-label="Animate portal"
          disabled={entering || viewing}
          aria-pressed={running} title="Animate portal" onClick={() => setRunning(value => !value)} />
      </div>
      {showExtensions && <PaintedExtension side="below" artwork={cabinet} id="bench-cabinet" height={1112} />}
      </div>
    </main>
  );
}
