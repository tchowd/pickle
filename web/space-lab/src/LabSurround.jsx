import { memo } from 'react';

// The surround shares the workbench's coordinate system. y=0 is the top
// of the original wall; y=1152 is the cut through the front of the desk.
function noise(seed) {
  const n = Math.sin(seed * 127.1 + 311.7) * 43758.5453;
  return n - Math.floor(n);
}

function Drawer({ x, y, width }) {
  const middle = width / 2;
  return (
    <g transform={`translate(${x} ${y})`}>
      <rect width={width} height="170" rx="5" fill="url(#drawer-finish)" stroke="#180f09" strokeWidth="7" />
      <rect x="6" y="5" width={width - 12} height="156" rx="3" fill="url(#bench-grain)" stroke="#795332" strokeOpacity=".6" />
      <path d={`M${middle - 57} 68 q4 32 20 33 h74 q15 -1 20 -33`} fill="none" stroke="#21170f" strokeWidth="17" />
      <path d={`M${middle - 57} 65 q4 29 20 30 h74 q15 -1 20 -30`} fill="none" stroke="url(#aged-brass)" strokeWidth="10" />
      <path d={`m${middle - 39} 92 17 1m39 0 12 -1`} fill="none" stroke="#cda562" strokeWidth="2" />
      <path d="m9 7 42 1 12 -2m62 0 26 1m91 -1 35 1m103 -1 31 2M13 152l33 3m264 3 35 -1" fill="none" stroke="#946b3c" strokeWidth="2" opacity=".65" />
      {[-57, 57].map(dx => <circle key={dx} cx={middle + dx} cy="65" r="7" fill="#655237" stroke="#2b2217" strokeWidth="3" />)}
    </g>
  );
}

export const LabSurround = memo(function LabSurround() {
  return (
    <svg className="lab-surround" viewBox="0 0 2048 1152" aria-hidden="true" focusable="false">
      <defs>
        <linearGradient id="shop-wall" x2=".45" y2="1">
          <stop stopColor="#090f16" /><stop offset=".64" stopColor="#05080d" /><stop offset="1" stopColor="#020206" />
        </linearGradient>
        <linearGradient id="desk-front" x2=".1" y2="1">
          <stop stopColor="#4b2b13" /><stop offset=".11" stopColor="#593417" /><stop offset="1" stopColor="#21140e" />
        </linearGradient>
        <linearGradient id="drawer-finish" x2=".85" y2="1">
          <stop stopColor="#52351e" /><stop offset=".52" stopColor="#372619" /><stop offset="1" stopColor="#241a12" />
        </linearGradient>
        <linearGradient id="aged-brass" x2=".2" y2="1">
          <stop stopColor="#c8a568" /><stop offset=".3" stopColor="#786449" /><stop offset=".65" stopColor="#413d2f" /><stop offset="1" stopColor="#826d49" />
        </linearGradient>
        <radialGradient id="knee-space" cx=".45" cy=".85" r=".9">
          <stop stopColor="#28251e" /><stop offset=".63" stopColor="#191815" /><stop offset="1" stopColor="#090b0b" />
        </radialGradient>
        <radialGradient id="lamp-spill">
          <stop stopColor="#fdab4a" stopOpacity=".22" /><stop offset=".55" stopColor="#b1672d" stopOpacity=".12" /><stop offset="1" stopColor="#9a5424" stopOpacity="0" />
        </radialGradient>
        <radialGradient id="phosphor-spill">
          <stop stopColor="#82ac65" stopOpacity=".14" /><stop offset="1" stopColor="#5b7148" stopOpacity="0" />
        </radialGradient>
        <linearGradient id="lip-shadow" x2="0" y2="1">
          <stop stopColor="#0c0804" stopOpacity=".87" /><stop offset="1" stopColor="#0c0804" stopOpacity="0" />
        </linearGradient>
        <linearGradient id="edge-fade" x2="0" y2="1">
          <stop stopColor="white" /><stop offset=".35" stopColor="white" /><stop offset="1" stopColor="black" />
        </linearGradient>
        <mask id="desk-edge-blend" maskUnits="userSpaceOnUse" x="0" y="1151" width="2048" height="68">
          <rect x="0" y="1151" width="2048" height="35" fill="url(#edge-fade)" />
          {/* Let cut cables and sheets end at physical contours on the wood. */}
          <path d="m0 1151 96 0 -39 28 -57 17Z m379 0 195 0 -56 30 -59 10 -88 -8Z m194 0 242 0 -46 28 -215 -13Z m236 0 155 0 -39 23 -79 13 -34 -5Z m155 0 313 0 -4 37 -325 29 -29 -13Z m425 0 174 0 -29 19 -75 13 -57 -3Z" fill="white" />
        </mask>
        <clipPath id="curtain-edge"><path d="M0 1v-244h82l11 9 14 -4 15 3 20 -3 16 4 18 -5 17 4 22 -2 13 238Z" /></clipPath>
        <clipPath id="poster-edge"><path d="m318 1 3 -25 242 6 4 20Z" /></clipPath>
        <clipPath id="shelf-edge"><path d="M1725 1v-5h156l12 -9 19 -6 69 -1 15 5 7 7 45 1v10Z" /></clipPath>
        <pattern id="wood-fibres" patternUnits="userSpaceOnUse" width="2048" height="256">
          <use href="#wood-swatch" /><use href="#wood-swatch" transform="translate(2048 0) scale(-1 1)" />
          <use href="#wood-swatch" transform="translate(0 256) scale(1 -1)" /><use href="#wood-swatch" transform="translate(2048 256) scale(-1)" />
        </pattern>
        <pattern id="etched-wear" width="319" height="211" patternUnits="userSpaceOnUse">
          {Array.from({ length: 290 }, (_, i) => {
            const x = noise(i + 51) * 319, y = noise(i + 180) * 211;
            const length = i % 13 === 0 ? 6 + noise(i + 8) * 27 : 1 + noise(i + 76) * 4;
            return <path key={i} d={`m${x} ${y} ${length} ${noise(i + 7) * 3 - 1.5} -2 1.8Z`}
              fill={i % 4 ? '#100c09' : '#ac8551'} opacity={.24 + noise(i + 20) * .34} />;
          })}
        </pattern>
        <pattern id="bench-grain" width="531" height="123" patternUnits="userSpaceOnUse">
          {Array.from({ length: 72 }, (_, i) => {
            const x = noise(i + 4) * 531;
            const y = noise(i + 110) * 123;
            const width = 14 + noise(i + 56) * 198;
            return <path key={i} d={`M${x} ${y} q${width * .31} -4 ${width} -2`}
              fill="none" stroke={i % 4 ? '#140d07' : '#b3814b'}
              opacity={.12 + noise(i + 79) * .25} strokeWidth={1 + noise(i + 99) * 2} />;
          })}
        </pattern>
        <pattern id="star-plaster" width="1171" height="743" patternUnits="userSpaceOnUse">
          {Array.from({ length: 91 }, (_, i) => {
            const x = noise(i + 9) * 1171;
            const y = noise(i + 121) * 743;
            const fill = ['#b7db9c', '#6ac8c3', '#77aabc', '#d5d598'][i % 4];
            return i % 6 === 0
              ? <use key={i} href={`#painted-star-${(i / 6) % 3}`} style={{ mixBlendMode: 'screen' }}
                transform={`translate(${x} ${y}) rotate(${i * 19}) scale(${.23 + noise(i + 54) * .66})`} />
              : <circle key={i} cx={x} cy={y} r={.7 + noise(i + 44) * 2.3} fill={fill} opacity={.35 + noise(i + 72) * .6} />;
          })}
        </pattern>
        <pattern id="parts-cabinet" width="490" height="204" patternUnits="userSpaceOnUse">
          <rect width="490" height="204" fill="#24180f" />
          <Drawer x={10} y={11} width={470} />
          <path d="M0 191h490" stroke="#634426" strokeWidth="5" />
        </pattern>
      </defs>
      <rect x="-10000" y="-10000" width="22048" height="10672" fill="#030409" />
      <rect x="-10000" y="-2100" width="22048" height="2100" fill="url(#shop-wall)" />
      <rect x="-10000" y="-10000" width="22048" height="10672" fill="url(#star-plaster)" />
      {/* Curtain rod, folds and the top edge of the Space Fleet poster. */}
      <path d="M-260 -252H216V4H-260Z" fill="#08172a" stroke="#191814" strokeWidth="12" />
      <path d="M92 6 86 -235q12 -12 23 0 12 -12 22 0 15 -14 30 0 13 -13 26 -1 17 -12 33 0l7 242Z" fill="#10283c" stroke="#070e17" strokeWidth="4" />
      <path d="m100 -232 4 236m19 -235 -8 236m40 -239 -11 240m38 -239 -14 240m46 -240 -18 240" fill="none" stroke="#061322" strokeWidth="5" />
      <path d="M-290 -244H246" stroke="#3f3928" strokeWidth="12" /><circle cx="248" cy="-244" r="10" fill="#62563d" />
      <path d="m321 5 2 -32 243 8 -1 25" fill="#111311" stroke="#39372c" strokeWidth="5" />
      <path d="m1880 2 13 -11 83 -1 14 12" fill="#293723" stroke="#131a10" strokeWidth="5" />
      <use href="#curtain-continuation" clipPath="url(#curtain-edge)" />
      <use href="#poster-continuation" clipPath="url(#poster-edge)" />
      <use href="#shelf-continuation" clipPath="url(#shelf-edge)" />
      {/* The bench continues past the papers, then turns down into a drawer bank. */}
      <rect x="-10000" y="672" width="22048" height="500" fill="#4d2c17" />
      <rect x="-10000" y="672" width="22048" height="570" fill="url(#bench-grain)" />
      <rect x="-10000" y="1152" width="22048" height="95" fill="#3c210f" />
      <rect x="-10000" y="1152" width="22048" height="95" fill="url(#bench-grain)" />
      <rect x="-3000" y="1152" width="8048" height="96" fill="url(#wood-fibres)" opacity=".86" />
      <rect x="-10000" y="1246" width="22048" height="10000" fill="#211d18" />
      <rect x="-10000" y="2048" width="22048" height="9000" fill="url(#bench-grain)" opacity=".65" />
      <rect x="-10000" y="1246" width="22048" height="852" fill="url(#parts-cabinet)" />
      <path d="M-2000 2101H4096" stroke="#110e0a" strokeWidth="16" />
      <rect x="557" y="1298" width="936" height="806" fill="url(#knee-space)" />
      <path d="m714 1300 9 174q-1 131 105 133 83 -2 89 -106l6 -194m62 0q-1 184 69 209t99 -211"
        fill="none" stroke="#10110e" strokeWidth="14" />
      <path d="m710 1300 9 174q-1 131 105 133 83 -2 89 -106l6 -194" fill="none" stroke="#493927" strokeWidth="3" />
      <path d="m548 1298 64 2 -12 873 -58 -3Z m889 -1 64 1 13 871 -58 -5Z" fill="url(#drawer-finish)" stroke="#1d160f" strokeWidth="7" />
      <path d="m612 1921 832 -3 0 43 -833 4Z" fill="#3c2b1b" stroke="#20190f" strokeWidth="6" />
      <path d="m598 2181 -774 111m1683 -112 984 147m-1022 -115 -438 1608m-1082 -1618 -193 242"
        fill="none" stroke="#15120f" strokeWidth="6" opacity=".72" />
      <rect x="-10000" y="1235" width="22048" height="73" fill="url(#desk-front)" stroke="#24150e" strokeWidth="6" />
      <rect x="-10000" y="1237" width="22048" height="59" fill="url(#bench-grain)" />
      <path d="M-10000 1241h22048" stroke="#96704a" opacity=".6" strokeWidth="4" />
      <rect x="-3000" y="1244" width="8048" height="63" fill="url(#wood-fibres)" opacity=".58" />
      <rect x="-3000" y="1308" width="8048" height="46" fill="url(#lip-shadow)" />
      <ellipse cx="378" cy="1114" rx="830" ry="435" fill="url(#lamp-spill)" />
      <ellipse cx="1388" cy="1115" rx="410" ry="185" fill="url(#phosphor-spill)" />
      <rect x="-3000" y="1152" width="8048" height="1800" fill="url(#etched-wear)" />
      <use href="#desk-continuation" mask="url(#desk-edge-blend)" />
    </svg>
  );
});
