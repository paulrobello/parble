// Walnut Marquetry (.impeccable/surfaces/web-ui-src-editor.md): one dark walnut panel inlaid with
// maple stringing, brass pins for state. Injected as one <style> (the page CSP allows inline styles).
// No backdrop-filter over the canvas; every animated property is opacity or transform.

/** A site path with its content-hash version (window.__PARBLE_HASHES), like audio/boot.ts. */
export function versioned(win: Window, site: string): string {
  const v = win.__PARBLE_HASHES?.[site];
  return v ? `${site}?v=${v}` : site;
}

export const FONT_DIR = "web/ui/fonts/";
export const THUMB_DIR = "web/ui/thumbs/";
const WEIGHTS = [400, 500, 700] as const;

export function fontFaces(win: Window): string {
  return WEIGHTS.map(
    (w) =>
      `@font-face{font-family:"Alegreya Sans";font-style:normal;font-weight:${w};font-display:swap;` +
      `src:url("${versioned(win, `${FONT_DIR}alegreya-sans-latin-${w}-normal.woff2`)}") format("woff2")}`,
  ).join("\n");
}

export const CSS = `
#pe{--w:#22160f;--wp:rgba(34,22,15,.9);--r:#3a271b;--m:#d9c2a0;--b:#c49a52;--p:#a8c8d8;--c:#f0e6d6;--f:#e0705a;
  --line:rgba(217,194,160,.28);--tray-h:174px;
  position:fixed;inset:0;pointer-events:none;z-index:20;font:400 13px/1.25 "Alegreya Sans",ui-sans-serif,system-ui,sans-serif;color:var(--c);
  -webkit-font-smoothing:antialiased;font-variant-numeric:lining-nums;font-feature-settings:"lnum" 1}
#pe *{box-sizing:border-box;margin:0}
#pe ::selection{background:var(--b);color:var(--w)}
#pe input{caret-color:var(--b)}
#pe .sec,#pe-sheet .list{scrollbar-width:thin;scrollbar-color:var(--line) transparent}
#pe.tray-closed{--tray-h:36px}
#pe button{font:inherit;color:inherit;background:none;border:1px solid transparent;border-radius:4px;cursor:pointer}
#pe button:hover:not(:disabled){background:rgba(58,39,27,.9);border-color:var(--line)}
#pe button:disabled{opacity:.4;cursor:default}
#pe :focus-visible{outline:2px solid var(--p);outline-offset:2px}
#pe .inlay{position:absolute;pointer-events:auto;background:var(--wp);border:1px solid rgba(217,194,160,.55);border-radius:6px;
  box-shadow:0 10px 28px rgba(0,0,0,.45),0 2px 4px rgba(0,0,0,.4);transition:opacity .4s ease-out}
#pe .inlay::before{content:"";position:absolute;inset:4px;border:1px solid rgba(217,194,160,.16);border-radius:3px;pointer-events:none}
#pe .sc{font-variant-caps:all-small-caps;letter-spacing:.07em;color:var(--m)}
#pe .pin{display:inline-block;width:7px;height:7px;border-radius:50%;flex:none;
  background:radial-gradient(circle at 35% 30%,#e8c985,var(--b) 55%,#7d5f2c);box-shadow:0 1px 2px rgba(0,0,0,.6)}
#pe .pin.off{background:none;box-shadow:inset 0 0 0 1px rgba(217,194,160,.5)}
#pe .pin.bl{background:radial-gradient(circle at 35% 30%,#d6ebf5,var(--p) 55%,#5f8497)}
#pe .pin.rd{background:radial-gradient(circle at 35% 30%,#f2b5a6,var(--f) 55%,#8a3b2c)}
#pe .travel{position:absolute;left:0;top:0;z-index:2;pointer-events:none;transition:transform .18s ease-out,opacity .18s ease-out}
#pe .travel.hide{opacity:0}
#pe kbd{font:500 11px/1 "Alegreya Sans",sans-serif;font-variant-numeric:lining-nums tabular-nums;color:var(--m);border:1px solid rgba(217,194,160,.5);border-bottom-width:2px;
  border-radius:3px;padding:2px 3px;background:rgba(0,0,0,.25);font-variant-numeric:tabular-nums;white-space:nowrap}
#pe svg.i{width:18px;height:18px;fill:none;stroke:currentColor;stroke-width:1.4;stroke-linecap:round;stroke-linejoin:round;flex:none}

#pe-top{left:12px;right:12px;top:12px;height:52px;display:flex;align-items:center;padding:0 12px;gap:2px}
#pe .sep{width:1px;height:28px;background:var(--line);margin:0 4px;flex:none}
#pe .brand{font-size:22px;font-weight:700;letter-spacing:.01em;margin-right:6px}
#pe .track{flex:none;color:var(--m);margin-right:2px;white-space:nowrap;max-width:180px;overflow:hidden;text-overflow:ellipsis;padding:6px 6px}
#pe .track-in{font:inherit;color:var(--c);background:rgba(0,0,0,.3);border:1px solid var(--b);border-radius:4px;padding:5px 6px;width:170px}
#pe .grp{position:relative;display:flex;align-items:center;gap:1px;flex:none}
#pe .t{position:relative;display:flex;flex-direction:column;align-items:center;gap:2px;padding:4px 4px;min-width:46px;white-space:nowrap}
#pe .t span{display:flex;gap:3px;align-items:center;font-size:12px}
#pe .t.on{background:var(--r);border-color:var(--b)}
#pe .lib{flex-direction:row;gap:7px;padding:8px 10px;font-size:13px}
#pe .sl{flex:none;white-space:nowrap;display:flex;align-items:center;gap:2px}
#pe .sl .v{font-size:15px;font-weight:700;padding:0 4px;font-variant-numeric:lining-nums tabular-nums;min-width:48px;text-align:center}
#pe .sl button{display:flex;align-items:center;gap:3px;padding:6px 5px}
#pe .sl .cl{margin-left:6px;border:1px solid transparent}
#pe .sl .cl.on{background:var(--r);border-color:var(--b)}
#pe .sp{flex:1}
#pe .build{transition:opacity .2s}
#pe.running .build{opacity:.45}
#pe.running .build:hover,#pe.running .build:focus-within{opacity:1}
#pe .mb{flex:none;white-space:nowrap;display:flex;flex-direction:column;gap:4px;margin:0 6px;align-items:flex-start}
#pe .mb b{font-size:15px;font-variant-numeric:tabular-nums;margin-left:4px;color:var(--c);font-variant-caps:normal;letter-spacing:0}
#pe .mb .row{display:flex;gap:0}
#pe .mb .row button{padding:2px 1.5px;border:0;display:flex}
#pe .mb .row button:hover{background:none}
#pe .mb .row button:hover .pin{box-shadow:0 0 0 2px rgba(196,154,82,.45)}
#pe .run{display:flex;align-items:center;gap:8px;background:var(--b);color:#1d1209;font-weight:700;font-size:15px;padding:8px 14px;border-color:#e0bb78;flex:none}
#pe .run:hover:not(:disabled){background:#d4ab62;border-color:#e0bb78}
#pe .run kbd{color:#3a271b;border-color:rgba(34,22,15,.55);background:rgba(255,255,255,.15)}
#pe .menu{position:absolute;top:58px;display:none;flex-direction:column;gap:2px;padding:8px;min-width:120px}
#pe .menu.open{display:flex}
#pe .menu button{display:flex;align-items:center;gap:8px;padding:6px 10px;text-align:left}

#pe-side{right:12px;top:76px;width:320px;bottom:calc(var(--tray-h) + 72px);display:flex;flex-direction:column;overflow:hidden}
#pe.side-closed #pe-side{width:28px}
#pe.side-closed #pe-side .sec{display:none}
#pe .spine{position:absolute;right:10px;top:12px;z-index:3;padding:3px 4px;display:flex;align-items:center}
#pe.side-closed .spine{right:auto;left:2px}
#pe .rk{display:none}
#pe.running .rk{display:inline}
#pe .acts-seg{margin-top:14px}
#pe .acts-seg button{padding:6px 6px;display:flex;align-items:center;justify-content:center;gap:5px}
#pe .sec{padding:16px 18px 14px;position:relative}
#pe .sec+.sec{border-top:1px solid var(--line);flex:1;overflow-y:auto;min-height:0}
#pe h2{font-size:17px;font-weight:700;display:flex;align-items:center;gap:8px;margin-bottom:12px}
#pe h2 .sc{font-weight:500;font-size:15px}
#pe .kv{display:grid;grid-template-columns:84px 1fr;row-gap:11px;align-items:center}
#pe .seg{display:flex;border:1px solid var(--line);border-radius:4px;overflow:hidden;position:relative}
#pe .seg button{flex:1;border-radius:0;padding:5px 4px 5px 14px;font-size:12px;border:0;border-right:1px solid var(--line)}
#pe .seg button:last-child{border-right:0}
#pe .seg .on{background:var(--r);color:var(--c)}
#pe .acts{display:flex;gap:6px;margin-top:14px;flex-wrap:wrap}
#pe .acts button,#pe .btn{display:flex;align-items:center;gap:6px;padding:6px 10px;border-color:var(--line)}
#pe .btn.warn{border-color:var(--f);color:#f6d2c8}
#pe .steps{list-style:none;padding:0;display:flex;flex-direction:column;gap:10px;counter-reset:s}
#pe .steps li{display:grid;grid-template-columns:22px 1fr;gap:8px;align-items:start;color:var(--c);line-height:1.35}
#pe .steps li::before{counter-increment:s;content:counter(s) ".";font-weight:700;color:var(--c);font-size:15px;line-height:1.2}
#pe .steps li.done{color:var(--m)}
#pe .steps li.done::before{content:"";width:7px;height:7px;margin:6px 0 0 2px;border-radius:50%;
  background:radial-gradient(circle at 35% 30%,#e8c985,var(--b) 55%,#7d5f2c);box-shadow:0 1px 2px rgba(0,0,0,.6)}
#pe .iss{display:grid;grid-template-columns:10px 1fr auto;gap:2px 9px;align-items:start;padding:9px 0;border-bottom:1px solid rgba(217,194,160,.14)}
#pe .iss:last-of-type{border-bottom:0}
#pe .iss .pin{margin-top:5px;grid-row:1/3}
#pe .iss i{font-style:normal;font-variant-numeric:tabular-nums;color:var(--p);font-weight:500}
#pe .iss span{grid-column:2;color:var(--c)}
#pe .iss button{grid-column:3;grid-row:1/3;align-self:center;color:var(--p);text-decoration:underline;text-underline-offset:3px;font-weight:500;padding:4px 4px}
#pe .note{margin-top:8px;color:var(--m);font-size:12px;line-height:1.35}
#pe .ok{display:flex;align-items:center;gap:8px;color:var(--c)}
#pe .ck{margin-top:14px;padding-top:12px;border-top:1px solid rgba(217,194,160,.14)}
#pe .ck h3{font-size:15px;font-weight:500;margin-bottom:10px}
#pe .ckb{display:flex;gap:8px;align-items:center}
#pe .ckb .seg{flex:1}
#pe .ckb .seg button{display:flex;align-items:center;justify-content:center;gap:6px;padding:5px 8px}
#pe .ckb .seg button .pin{margin-left:-2px}
#pe .ck.stale .iss{opacity:.55}
#pe .ck .ok{margin-top:10px}

#pe-tray{left:12px;right:12px;bottom:12px;height:var(--tray-h);overflow:hidden;transition:opacity .4s ease-out}
#pe .tabs{position:relative;display:flex;height:36px;border-bottom:1px solid var(--line);padding:0 14px;gap:2px;align-items:flex-end}
#pe .tabs button{position:relative;border-radius:4px 4px 0 0;padding:6px 16px 7px 22px;font-size:14px;border-bottom:0;color:var(--m)}
#pe .tabs .fit{margin-left:10px}
#pe .fit-note{margin:auto 0;padding:0 8px;color:var(--m);font-size:14px}
#pe .tabs .on{background:var(--r);border-color:var(--line);color:var(--c);font-weight:700}
#pe .tabs .collapse{margin-left:auto;align-self:center;display:flex;gap:6px;align-items:center;padding:4px 8px;font-size:12px;border-radius:4px}
#pe .body{display:flex;height:138px;padding:9px 18px 0;gap:14px}
#pe .tiles.more-r{mask-image:linear-gradient(90deg,#000 calc(100% - 48px),transparent)}
#pe .tiles.more-l{mask-image:linear-gradient(90deg,transparent,#000 48px)}
#pe .tiles.more-l.more-r{mask-image:linear-gradient(90deg,transparent,#000 48px,#000 calc(100% - 48px),transparent)}
#pe .tiles{position:relative;display:flex;gap:14px;overflow-x:auto;overflow-y:hidden;flex:1;min-width:0;padding-bottom:6px;scrollbar-width:thin;scrollbar-color:var(--line) transparent}
#pe .pc{width:96px;flex:none;display:flex;flex-direction:column;gap:5px;text-align:center;font-size:12.5px;color:var(--c)}
#pe .tile{position:relative;width:96px;height:96px;padding:0;background:rgba(0,0,0,.22);border:1px solid var(--line);border-radius:5px;display:flex;align-items:center;justify-content:center}
#pe .tile img{width:80px;height:80px;pointer-events:none}
#pe .tile:hover{background:var(--r)}
#pe .tile.on{background:var(--r);border-color:var(--b);box-shadow:0 4px 10px rgba(0,0,0,.4)}
#pe .pc .nm{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
#pe .pc[data-g]{display:grid;grid-template-columns:96px;row-gap:5px}
#pe .pc[data-g]>.tile{grid-area:1/1}
#pe .pc[data-g]>.nm{grid-area:2/1}
#pe .hts{grid-area:1/1;align-self:end;justify-self:start;margin:0 0 5px 5px;z-index:1;display:flex;flex-direction:column-reverse;border:1px solid var(--line);border-radius:4px;overflow:hidden;background:rgba(20,14,10,.72)}
#pe .hts .ht{width:20px;height:19px;padding:0;border:0;border-bottom:1px solid var(--line);border-radius:0;font:500 11px/1 "Alegreya Sans",sans-serif;font-variant-numeric:lining-nums tabular-nums;color:var(--m)}
#pe .hts .ht:first-child{border-bottom:0}
#pe .hts .ht:hover{background:var(--r);color:var(--c)}
#pe .hts .ht.on{background:var(--b);color:#1b130d}
#pe .hint{flex:none;width:300px;border-left:1px solid var(--line);padding:2px 0 0 20px;display:flex;flex-direction:column;gap:9px;color:var(--m)}
#pe .hint p{line-height:1.35}
#pe .hint .b{display:flex;gap:8px;flex-wrap:wrap}
#pe .hint button{display:flex;align-items:center;gap:7px;padding:6px 9px;border-color:var(--line);color:var(--c)}

#pe-tip{left:0;top:0;display:none;align-items:center;gap:8px;padding:7px 12px;white-space:nowrap;font-variant-numeric:tabular-nums;pointer-events:none;transition:none}
#pe-tip.show{display:flex}
#pe-tip.bad{border-color:var(--f)}
#pe-marq{position:absolute;left:0;top:0;width:1px;height:1px;transform-origin:0 0;display:none;border:1px solid var(--p);background:rgba(168,200,216,.12)}
#pe-marq.show{display:block}
#pe-mlabel{left:0;top:0;display:none;padding:5px 10px;pointer-events:none;color:var(--p);font-variant-numeric:tabular-nums}
#pe-mlabel.show{display:block}

#pe-cap b{font-weight:700;font-variant-numeric:lining-nums tabular-nums}
#pe-cap{left:50%;top:12px;transform:translateX(-50%);display:flex;align-items:center;gap:12px;padding:6px 14px;opacity:0;pointer-events:none;font-variant-numeric:tabular-nums}
#pe .hair{position:absolute;background:var(--m);opacity:0;transition:opacity .4s ease-out;pointer-events:none}
#pe .hair.h-top{left:12px;right:12px;top:12px;height:1px}
#pe .hair.h-bottom{left:12px;right:12px;bottom:12px;height:1px}
#pe .hair.h-right{right:12px;top:12px;bottom:12px;width:1px}
#pe.faded .hair{opacity:.35}
#pe.faded #pe-cap{opacity:1}
#pe.faded .inlay.fz{opacity:0;pointer-events:none}
#pe.faded.show-top #pe-top,#pe.faded.show-bottom #pe-tray,#pe.faded.show-bottom #pe-snd,#pe.faded.show-right #pe-side{opacity:1;pointer-events:auto}
#pe.faded.show-top #pe-cap{opacity:0}
#pe.building #pe-cap,#pe.building.faded.show-top #pe-cap{opacity:1}

#pe-sheet{right:12px;top:76px;bottom:12px;width:min(640px,calc(50vw - 12px));display:none;flex-direction:column;z-index:5}
#pe-sheet.open{display:flex}
#pe-sheet header{display:flex;align-items:center;gap:8px;padding:16px 18px;border-bottom:1px solid var(--line)}
#pe-sheet header h2{margin:0;flex:1}
#pe-sheet .list{flex:1;overflow-y:auto;padding:6px 18px}
#pe-sheet .row{display:grid;grid-template-columns:64px 1fr auto auto;gap:12px;align-items:center;padding:10px 0;border-bottom:1px solid rgba(217,194,160,.14)}
#pe-sheet .thumb{width:64px;height:48px;border:1px solid var(--line);border-radius:4px;background:rgba(0,0,0,.25);display:flex;align-items:center;justify-content:center;
  font-weight:700;font-size:20px;color:var(--b)}
#pe-sheet .meta{color:var(--m);font-size:12px;font-variant-numeric:tabular-nums}
#pe-sheet .ask{margin:0 18px 14px;padding:10px 12px;border:1px solid var(--b);border-radius:4px;display:none;gap:8px;align-items:center}
#pe-sheet .ask.open{display:flex}
#pe-sheet .ask span{flex:1}
#pe-sheet .status{padding:0 18px 12px;color:var(--m);font-size:12px;min-height:1em}
#pe-sheet .status.bad{color:#f6d2c8}

body.parble-editor #parble-mixer{right:12px!important;bottom:calc(var(--pe-tray-h,174px) + 24px)!important;padding:6px 12px!important;
  background:rgba(34,22,15,.9)!important;border:1px solid rgba(217,194,160,.55)!important;border-radius:6px!important;
  box-shadow:0 10px 28px rgba(0,0,0,.45),0 2px 4px rgba(0,0,0,.4);color:#f0e6d6!important;
  font:400 13px/1.25 "Alegreya Sans",ui-sans-serif,system-ui,sans-serif!important;z-index:21!important;transition:opacity .4s ease-out}
body.parble-editor #parble-mixer button{font:inherit;color:inherit;background:none;border:1px solid transparent;border-radius:4px;padding:2px 6px 2px 26px;cursor:pointer;
  background-image:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 20 20' fill='none' stroke='%23f0e6d6' stroke-width='1.4' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M3 8v4h3l4 3V5L6 8z M13 7.5a3.5 3.5 0 0 1 0 5 M15.5 5a7 7 0 0 1 0 10'/%3E%3C/svg%3E");
  background-repeat:no-repeat;background-size:18px;background-position:3px center}
body.parble-editor #parble-mixer button:hover{background-color:rgba(58,39,27,.9);border-color:rgba(217,194,160,.28)}
body.parble-editor #parble-mixer input{accent-color:#c49a52}
body.parble-editor #parble-mixer label{color:#d9c2a0;font-variant-caps:all-small-caps;letter-spacing:.07em}
body.parble-editor.pe-faded #parble-mixer{opacity:0;pointer-events:none}
body.parble-editor.pe-faded.pe-show-bottom #parble-mixer{opacity:1;pointer-events:auto}
body.parble-editor #parble-hud,body.parble-editor #parble-controls{display:none}

#pe-ghost{position:absolute;inset:0;width:100%;height:100%;pointer-events:none;overflow:visible}
#pe-ghost path{fill:none;stroke:var(--b);stroke-width:1.4;stroke-dasharray:5 4;stroke-linecap:round}
#pe-ghost path.bad{stroke:var(--f)}
#pe-ghost path.run{stroke:rgba(196,154,82,.7);stroke-width:1;stroke-dasharray:3 4}
#pe-nz{position:absolute;inset:0;width:100%;height:100%;pointer-events:none;overflow:visible}
#pe-nz path{fill:none;stroke-linecap:round}
#pe-nz path.nz{stroke:var(--p);stroke-width:1;stroke-dasharray:2 4;opacity:.75}
#pe-nz path.nza{stroke:var(--p);stroke-width:1.6;opacity:.85}
#pe-nz path.sel{stroke:var(--b);opacity:1}
#pe-nz path.nz.sel{stroke-width:1.4;stroke-dasharray:6 3}
#pe .kv .seg.nzs button{padding:5px 2px;text-align:center}
#pe .nz .iss.on i{color:var(--b)}
#pe .tile .ct{position:absolute;top:5px;right:6px;font:500 11px/1 "Alegreya Sans",sans-serif;font-variant-numeric:lining-nums tabular-nums;color:var(--m)}
#pe .kv .seg.opt button{display:flex;align-items:center;justify-content:center;gap:6px;padding:5px 8px}
#pe .kv .seg.opt button .pin{margin-left:-2px}
#pe-sheet .thumb svg{display:block}
#pe-compass{left:24px;bottom:calc(var(--tray-h) + 24px);width:66px;height:66px;padding:0;border-radius:50%;pointer-events:none}
#pe-compass::before{border-radius:50%}
#pe-compass .ring{fill:none;stroke:var(--line)}
#pe-compass .needle{stroke:var(--b);stroke-width:2;stroke-linecap:round}
#pe-compass text{fill:var(--m);font:700 11px "Alegreya Sans",ui-sans-serif,sans-serif;text-anchor:middle;dominant-baseline:central}
#pe-compass [data-c="N"]{fill:var(--b)}
#pe-tt{left:0;top:0;display:none;align-items:center;gap:7px;padding:5px 10px;white-space:nowrap;pointer-events:none;z-index:6;transition:none}
#pe-tt.show{display:flex}
#pe.faded #pe-tt{display:none}

#pe.running .build kbd{display:none}
@media (max-width:1279px){
  #pe .rc span{display:none}
  #pe .rc{min-width:34px}
  #pe .hint{display:none}
  #pe .build kbd,#pe .run kbd{display:none}
  #pe .track{max-width:110px}
  #pe .sl>.sc{display:none}
  #pe .sl .cl{margin-left:2px}
}
@media (prefers-reduced-motion:reduce){
  #pe .travel,#pe .inlay,#pe .hair,#pe .build,body.parble-editor #parble-mixer{transition:none}
}
`;
