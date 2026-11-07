// WGSL compatibility shim for browsers without the `texture_and_sampler_let` language extension
// (Safari 27). Bevy 0.20's PBR shaders bind texture and sampler handles with `let`, which such
// browsers reject ("'texture_2d<f32>' cannot be used as the type of a 'let'"), and Bevy then quits.
// The rewrite inlines each such `let` alias inside its enclosing block, which is semantically
// identical WGSL. `?wgsl_compat=1` forces it on, `?wgsl_compat=0` turns it off.
(function () {
  "use strict";
  if (typeof GPUDevice === "undefined" || !navigator.gpu) return;
  var mode = new URLSearchParams(location.search).get("wgsl_compat");
  var feats = navigator.gpu.wgslLanguageFeatures;
  var native = !!(feats && feats.has("texture_and_sampler_let"));
  // perModule: the alias count of each rewritten module, in creation order (e2e/smoke.mjs checks each).
  var stats = { active: false, native: native, modules: 0, rewritten: 0, aliases: 0, perModule: [], unhandled: 0, ms: 0 };
  window.__wgslCompat = stats;
  if (mode === "0" || (native && mode !== "1")) return;
  stats.active = true;

  // Bindless `let t = arr[i];` aliases are counted and warned about, never inlined: moving the index
  // expression to each use site could change when it is evaluated.
  var HANDLE_VAR = /\bvar\s+([A-Za-z_]\w*)\s*:\s*(?:binding_array\s*<\s*)?(?:texture_\w+|sampler_comparison|sampler)\b/g;
  var HANDLE_TYPE = /^\s*(?:binding_array\s*<\s*)?(?:texture_\w+|sampler_comparison|sampler)\b/;
  var LET = /^let\s+([A-Za-z_]\w*)\s*(:\s*[^=;]+)?=\s*([^;]+);/;
  var DECL = /^(?:let|var|const)\s+([A-Za-z_]\w*)/;
  var IDENT = /^[A-Za-z_]\w*/;

  function rewrite(code) {
    var handles = Object.create(null), m;
    HANDLE_VAR.lastIndex = 0;
    while ((m = HANDLE_VAR.exec(code))) handles[m[1]] = true;
    var scopes = [Object.create(null)], out = "", i = 0, n = code.length, aliases = 0, unhandled = 0, last = "";
    function lookup(name) {
      for (var s = scopes.length - 1; s >= 0; s--) if (name in scopes[s]) return scopes[s][name];
      return name;
    }
    while (i < n) {
      var c = code[i];
      if (c === "/" && code[i + 1] === "/") {
        var e = code.indexOf("\n", i);
        e = e < 0 ? n : e;
        out += code.slice(i, e);
        i = e;
      } else if (c === "/" && code[i + 1] === "*") {
        var b = code.indexOf("*/", i + 2);
        b = b < 0 ? n : b + 2;
        out += code.slice(i, b);
        i = b;
      } else if (c === "{") {
        scopes.push(Object.create(null));
        out += c;
        i++;
        last = c;
      } else if (c === "}") {
        if (scopes.length > 1) scopes.pop();
        out += c;
        i++;
        last = c;
      } else if (/[0-9]/.test(c)) {
        var num = /^[0-9][0-9A-Za-z_.]*/.exec(code.slice(i, i + 64))[0];
        out += num;
        i += num.length;
        last = "0";
      } else if (/[A-Za-z_]/.test(c)) {
        var rest = code.slice(i, i + 512);
        var word = IDENT.exec(rest)[0];
        var prev = last;
        last = "a";
        if (word === "let" && scopes.length > 1) {
          var lm = LET.exec(rest);
          if (lm) {
            var rhs = lm[3].trim();
            var typedHandle = lm[2] && HANDLE_TYPE.test(lm[2].slice(1));
            if (/^[A-Za-z_]\w*$/.test(rhs) && handles[lookup(rhs)]) {
              scopes[scopes.length - 1][lm[1]] = lookup(rhs);
              aliases++;
              i += lm[0].length;
              continue;
            }
            if (typedHandle || /^[A-Za-z_]\w*\s*\[/.test(rhs) && handles[rhs.split("[")[0].trim()]) unhandled++;
          }
        }
        // Limitation: a declaration in a `for (...)` header lands in the enclosing scope, so it can hide an
        // alias of the same name for the rest of that block. That leaves a `let` un-inlined (a loud compile
        // error in Safari), never a wrong substitution.
        var dm = DECL.exec(rest);
        if (dm && scopes.length > 1) {
          scopes[scopes.length - 1][dm[1]] = dm[1];
          out += dm[0];
          i += dm[0].length;
          continue;
        }
        out += prev === "." ? word : lookup(word);
        i += word.length;
      } else {
        out += c;
        i++;
        if (!/\s/.test(c)) last = c;
      }
    }
    return { code: out, aliases: aliases, unhandled: unhandled };
  }
  window.__wgslCompatRewrite = rewrite;

  var create = GPUDevice.prototype.createShaderModule;
  GPUDevice.prototype.createShaderModule = function (desc) {
    stats.modules++;
    if (desc && typeof desc.code === "string" && /\blet\s/.test(desc.code)) {
      var t0 = performance.now();
      var r = rewrite(desc.code);
      stats.ms += performance.now() - t0;
      if (r.unhandled) {
        stats.unhandled += r.unhandled;
        console.warn("wgsl-compat: " + r.unhandled + " handle let(s) left unrewritten (non-alias form)");
      }
      if (r.aliases) {
        stats.rewritten++;
        stats.aliases += r.aliases;
        stats.perModule.push(r.aliases);
        var copy = {};
        for (var k in desc) copy[k] = desc[k];
        copy.code = r.code;
        return create.call(this, copy);
      }
    }
    return create.apply(this, arguments);
  };
})();
