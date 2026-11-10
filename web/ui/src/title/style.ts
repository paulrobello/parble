// The title screen's styles (P5-T3), in the editor's Walnut Marquetry system (DESIGN.md): walnut
// veil inlay panels with maple stringing, brass for the one primary action and every chosen state,
// pale blue for focus, Alegreya Sans with small-caps labels and lining numerals. The key art fills the
// screen behind a light wordmark set in its dark title zone (assets-source/keyart/README.md).
// No backdrop-filter; every animated property is opacity or transform.

export const TITLE_CSS = `
#pt{--w:#22160f;--wp:rgba(34,22,15,.9);--r:#3a271b;--m:#d9c2a0;--b:#c49a52;--bl:#d4ab62;--p:#a8c8d8;--c:#f0e6d6;
  --line:rgba(217,194,160,.28);
  position:fixed;inset:0;z-index:40;overflow:hidden;background:#0b0d14;color:var(--c);
  font:400 14px/1.3 "Alegreya Sans",ui-sans-serif,system-ui,sans-serif;font-variant-numeric:lining-nums;font-feature-settings:"lnum" 1;
  -webkit-font-smoothing:antialiased;transition:opacity .35s ease-out}
#pt.leaving{opacity:0}
#pt *{box-sizing:border-box;margin:0}
#pt ::selection{background:var(--b);color:var(--w)}
#pt :focus-visible{outline:2px solid var(--p);outline-offset:2px}
#pt .art,#pt .art img{position:absolute;inset:0;width:100%;height:100%;object-fit:cover;object-position:50% 0}
#pt .art img{opacity:0;transition:opacity .9s cubic-bezier(.16,1,.3,1)}
#pt .art img.in{opacity:1}
#pt .shade{position:absolute;inset:0;pointer-events:none;
  background:linear-gradient(to bottom,rgba(6,7,12,.38) 0,rgba(6,7,12,0) 32%,rgba(6,7,12,0) 70%,rgba(6,7,12,.55) 100%)}
#pt .mark{position:absolute;left:50%;top:max(28px,5.5vh);transform:translateX(-50%);text-align:center;width:min(92vw,880px);transition:opacity .25s ease-out}
#pt.sheet-open .mark{opacity:.08}
#pt h1{font-weight:700;font-size:clamp(64px,9.2vw,140px);line-height:.9;letter-spacing:-.015em;color:var(--c);
  text-shadow:0 3px 24px rgba(0,0,0,.55),0 1px 2px rgba(0,0,0,.5)}
#pt .rule{display:flex;align-items:center;justify-content:center;gap:10px;margin:14px auto 12px;width:min(56vw,420px)}
#pt .rule::before,#pt .rule::after{content:"";height:1px;flex:1;background:linear-gradient(to right,rgba(217,194,160,0),rgba(217,194,160,.7))}
#pt .rule::after{background:linear-gradient(to left,rgba(217,194,160,0),rgba(217,194,160,.7))}
#pt .tag{font-size:clamp(16px,1.35vw,20px);line-height:1.35;color:var(--m);text-shadow:0 1px 10px rgba(0,0,0,.7);text-wrap:balance}

#pt .inlay{background:var(--wp);border:1px solid rgba(217,194,160,.55);border-radius:6px;position:relative;
  box-shadow:0 10px 28px rgba(0,0,0,.45),0 2px 4px rgba(0,0,0,.4)}
#pt .inlay::before{content:"";position:absolute;inset:4px;border:1px solid rgba(217,194,160,.16);border-radius:3px;pointer-events:none}
#pt .sc{font-variant-caps:all-small-caps;letter-spacing:.07em;color:var(--m)}
#pt .pin{display:inline-block;width:7px;height:7px;border-radius:50%;flex:none;
  background:radial-gradient(circle at 35% 30%,#e8c985,var(--b) 55%,#7d5f2c);box-shadow:0 1px 2px rgba(0,0,0,.6)}
#pt .pin.off{background:none;box-shadow:inset 0 0 0 1px rgba(217,194,160,.5)}
#pt .pin.bl{background:radial-gradient(circle at 35% 30%,#d6ebf5,var(--p) 55%,#5f8497)}
#pt kbd{font:500 11px/1 "Alegreya Sans",sans-serif;font-variant-numeric:lining-nums tabular-nums;color:var(--m);
  border:1px solid rgba(217,194,160,.5);border-bottom-width:2px;border-radius:3px;padding:2px 4px;background:rgba(0,0,0,.25);white-space:nowrap}
#pt button{font:inherit;color:inherit;background:none;border:1px solid transparent;border-radius:4px;cursor:pointer}
#pt button:disabled{opacity:.4;cursor:default}
#pt .q{display:flex;align-items:center;gap:8px;padding:9px 12px;border-color:var(--line);white-space:nowrap}
#pt .q:hover:not(:disabled){background:rgba(58,39,27,.95);border-color:rgba(217,194,160,.55)}
#pt .q[aria-expanded="true"]{background:var(--r);border-color:var(--b)}
#pt .go{display:flex;align-items:center;gap:10px;padding:10px 18px;background:var(--b);border-color:#e0bb78;color:#1d1209;
  font-weight:700;font-size:17px;white-space:nowrap}
#pt .go:hover{background:var(--bl)}
#pt .go kbd{color:#3a271b;border-color:rgba(34,22,15,.55);background:rgba(255,255,255,.15)}
#pt svg.i{width:18px;height:18px;fill:none;stroke:currentColor;stroke-width:1.4;stroke-linecap:round;stroke-linejoin:round;flex:none}
#pt .go svg.i{fill:currentColor;stroke:none}

#pt .bar{position:absolute;left:50%;bottom:max(20px,3.2vh);transform:translateX(-50%);display:flex;align-items:center;gap:6px;padding:10px 12px}
#pt .sep{width:1px;height:30px;background:var(--line);margin:0 6px;flex:none}
#pt .swatch{width:22px;height:22px;border-radius:50%;flex:none;box-shadow:0 1px 3px rgba(0,0,0,.6)}
#pt .gpu{display:grid;grid-template-columns:auto auto;align-items:center;column-gap:7px;row-gap:1px;padding:0 8px 0 4px;min-width:150px}
#pt .gpu .pin{grid-row:span 2}
#pt .gpu b{font-size:15px;font-weight:700}
#pt .gpu small{grid-column:2;font-size:12px;color:var(--m);max-width:220px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
#pt .gpu.busy .pin{animation:pt-blink 1.2s ease-in-out infinite}
@keyframes pt-blink{50%{opacity:.25}}

#pt .sheet{position:absolute;left:50%;bottom:calc(max(20px,3.2vh) + 76px);transform:translate(-50%,0);width:min(560px,calc(100vw - 24px));
  max-height:calc(100vh - 220px);overflow:auto;padding:20px 22px 18px;scrollbar-width:thin;scrollbar-color:var(--line) transparent;
  transition:opacity .22s ease-out,transform .22s cubic-bezier(.16,1,.3,1)}
#pt .sheet[hidden]{display:block;opacity:0;transform:translate(-50%,10px);pointer-events:none;visibility:hidden}
#pt .sheet h2{font-size:17px;font-weight:700;line-height:1.25;margin-bottom:4px}
#pt .sheet h2 .sc{font-size:15px;font-weight:500;margin-right:4px}
#pt .sheet .lede{color:var(--m);font-size:13px;line-height:1.4;margin-bottom:16px;max-width:60ch}
#pt .sheet section+section{border-top:1px solid var(--line);margin-top:16px;padding-top:14px}
#pt .sheet h3{font:400 15px/1.2 "Alegreya Sans",sans-serif;font-variant-caps:all-small-caps;letter-spacing:.07em;color:var(--m);margin-bottom:9px}
#pt .note{font-size:12px;line-height:1.4;color:var(--m);margin-top:8px}
#pt .note b{color:var(--c);font-weight:500}
#pt .foot{display:flex;justify-content:flex-end;gap:8px;margin-top:18px}

#pt .seg{display:inline-flex;border:1px solid var(--line);border-radius:4px;overflow:hidden}
#pt .seg button{display:flex;align-items:center;gap:7px;border:0;border-radius:0;padding:7px 12px;white-space:nowrap}
#pt .seg button+button{border-left:1px solid var(--line)}
#pt .seg button:hover{background:rgba(58,39,27,.95)}
#pt .seg button[aria-checked="true"]{background:var(--r);box-shadow:inset 0 0 0 1px var(--b)}
#pt .seg button .pin{display:none}
#pt .seg button[aria-checked="true"] .pin{display:inline-block}
#pt .seg small{color:var(--m);font-size:12px}

#pt .mix{display:grid;grid-template-columns:62px 1fr 3ch;align-items:center;gap:8px 12px}
#pt .mix label{font-variant-caps:all-small-caps;letter-spacing:.07em;color:var(--m);font-size:14px}
#pt .mix output{text-align:right;font-variant-numeric:lining-nums tabular-nums;font-size:13px}
#pt input[type=range]{-webkit-appearance:none;appearance:none;width:100%;height:18px;background:none;cursor:pointer}
#pt input[type=range]::-webkit-slider-runnable-track{height:3px;border-radius:2px;background:linear-gradient(to right,var(--b) var(--v,100%),rgba(217,194,160,.25) var(--v,100%))}
#pt input[type=range]::-moz-range-track{height:3px;border-radius:2px;background:rgba(217,194,160,.25)}
#pt input[type=range]::-moz-range-progress{height:3px;border-radius:2px;background:var(--b)}
#pt input[type=range]::-webkit-slider-thumb{-webkit-appearance:none;width:13px;height:13px;margin-top:-5px;border-radius:50%;border:0;
  background:radial-gradient(circle at 35% 30%,#e8c985,var(--b) 55%,#7d5f2c);box-shadow:0 1px 2px rgba(0,0,0,.6)}
#pt input[type=range]::-moz-range-thumb{width:13px;height:13px;border-radius:50%;border:0;
  background:radial-gradient(circle at 35% 30%,#e8c985,var(--b) 55%,#7d5f2c);box-shadow:0 1px 2px rgba(0,0,0,.6)}
#pt .chk{display:flex;align-items:center;gap:8px;grid-column:2;font-size:13px;cursor:pointer}
#pt input[type=checkbox]{-webkit-appearance:none;appearance:none;width:15px;height:15px;margin:0;border:1px solid rgba(217,194,160,.55);border-radius:3px;background:rgba(0,0,0,.3);display:grid;place-content:center;cursor:pointer}
#pt input[type=checkbox]:checked{border-color:var(--b)}
#pt input[type=checkbox]:checked::before{content:"";width:7px;height:7px;border-radius:50%;background:radial-gradient(circle at 35% 30%,#e8c985,var(--b) 55%,#7d5f2c)}

#pt .tiles{display:grid;grid-template-columns:repeat(auto-fill,minmax(112px,1fr));gap:10px}
#pt .tile{position:relative;display:flex;flex-direction:column;align-items:center;gap:6px;padding:10px 6px 9px;
  background:rgba(0,0,0,.22);border:1px solid var(--line);border-radius:5px;text-align:center}
#pt .tile:hover{background:rgba(58,39,27,.95)}
#pt .tile[aria-checked="true"]{background:var(--r);border-color:var(--b);box-shadow:0 4px 10px rgba(0,0,0,.4)}
#pt .tile img{width:72px;height:72px;border-radius:50%;object-fit:cover;box-shadow:0 2px 6px rgba(0,0,0,.55)}
#pt .tile b{font-size:14px;font-weight:700}
#pt .tile small{font-size:12px;color:var(--m);line-height:1.25}
#pt .tile .pin{position:absolute;top:7px;right:7px;display:none}
#pt .tile[aria-checked="true"] .pin{display:inline-block}
#pt .seg svg{width:26px;height:26px;flex:none}
#pt .seg svg circle{fill:none;stroke:currentColor;stroke-width:1.2}
#pt .detail{display:flex;align-items:baseline;gap:8px;flex-wrap:wrap;margin-top:12px;font-size:13px}
#pt .detail b{font-size:15px}
#pt .detail span{color:var(--m)}

@media (max-width:1100px){#pt .bar kbd{display:none}#pt .gpu small{max-width:140px}}
@media (prefers-reduced-motion:reduce){#pt,#pt .art img,#pt .sheet,#pt .mark{transition:none}#pt .gpu.busy .pin{animation:none}}
`;
