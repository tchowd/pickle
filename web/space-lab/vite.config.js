import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

const artwork = new URL('./src/artwork/', import.meta.url);

// Deliver the static paint paths once as native SVG. React places the objects
// by ID, so toggling the portal never reconciles the 42,630 individual paths.
function sceneArtwork() {
  return {
    name: 'portal-lab-artwork',
    transformIndexHtml() {
      const layers = JSON.parse(readFileSync(new URL('layers.json', artwork), 'utf8'));
      const shapes = layers.map(id => {
        const svg = readFileSync(new URL(id + '.svg', artwork), 'utf8');
        const inner = svg.trim().replace(/^<svg[^>]*>|<\/svg>$/g, '');
        return '<g id="paint-' + id + '">' + inner + '</g>';
      });
      const materials = readFileSync(new URL('surround-materials.svg', artwork), 'utf8');
      shapes.push(materials.trim().replace(/^<svg[^>]*>|<\/svg>$/g, ''));
      return [{
        tag: 'svg',
        attrs: { width: '0', height: '0', 'aria-hidden': 'true', style: 'position:absolute' },
        children: '<defs>' + shapes.join('\n') + '</defs>',
        injectTo: 'body-prepend',
      }];
    },
    handleHotUpdate({ file, server }) {
      if (file.startsWith(fileURLToPath(artwork))) {
        server.ws.send({ type: 'full-reload' });
        return [];
      }
    },
  };
}

export default defineConfig({
  base: './',
  plugins: [react(), sceneArtwork()],
});
