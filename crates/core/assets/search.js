// grack.com search (§6b): a thin loader over /search.wasm — the SAME Rust
// core that built /search.bin, compiled for the browser. This file only
// moves bytes and pixels; every search decision (stemming, ranking,
// prefix-matching the token being typed) lives in the wasm.
//
// The I18N sentinel on the assignment below is replaced at emit with a
// per-locale string map from `[i18n.strings]` (`search` / `search_placeholder`
// / `search_empty`). Keep the token off every other line: the emitter fills
// each occurrence, and a copy in prose would land the JSON in a comment.
(function () {
  "use strict";

  var I18N = __SEARCH_I18N__;
  var wasm = null; // { mem, alloc, init, search }
  var enc = new TextEncoder();
  var dec = new TextDecoder();

  function strings() {
    var lang = document.documentElement.lang || "";
    return I18N[lang] || I18N[""] || I18N[Object.keys(I18N)[0]] || {
      label: "Search",
      placeholder: "Search posts…",
      empty: "No posts match.",
    };
  }

  function load() {
    if (wasm) return Promise.resolve(wasm);
    return Promise.all([
      fetch("/search.wasm").then(function (r) { return r.arrayBuffer(); }),
      fetch("/search.bin").then(function (r) { return r.arrayBuffer(); }),
    ]).then(function (both) {
      return WebAssembly.instantiate(both[0], {}).then(function (mod) {
        var ex = mod.instance.exports;
        var bytes = new Uint8Array(both[1]);
        var ptr = ex.alloc(bytes.length);
        new Uint8Array(ex.memory.buffer, ptr, bytes.length).set(bytes);
        if (ex.init(ptr, bytes.length) !== 0) throw new Error("bad index");
        wasm = ex;
        return ex;
      });
    });
  }

  function query(ex, q) {
    var bytes = enc.encode(q);
    var ptr = ex.alloc(bytes.length);
    new Uint8Array(ex.memory.buffer, ptr, bytes.length).set(bytes);
    var packed = ex.search(ptr, bytes.length);
    if (packed === 0n) return [];
    var outPtr = Number(packed >> 32n);
    var outLen = Number(packed & 0xffffffffn);
    return JSON.parse(dec.decode(new Uint8Array(ex.memory.buffer, outPtr, outLen)));
  }

  // ---- UI
  var overlay = null, input = null, list = null;

  function build() {
    var t = strings();
    overlay = document.createElement("div");
    overlay.className = "search-overlay";
    overlay.innerHTML =
      '<div class="search-panel" role="dialog" aria-label="' + esc(t.label) + '">' +
      '<input class="search-input" type="search" placeholder="' + esc(t.placeholder) +
      '" aria-label="' + esc(t.placeholder) + '">' +
      '<div class="search-results" role="listbox"></div>' +
      "</div>";
    document.body.appendChild(overlay);
    input = overlay.querySelector(".search-input");
    list = overlay.querySelector(".search-results");
    overlay.addEventListener("click", function (e) {
      if (e.target === overlay) close();
    });
    document.addEventListener("keydown", function (e) {
      if (e.key === "Escape") close();
    });
    input.addEventListener("input", function () {
      var q = input.value.trim();
      if (!q) { list.textContent = ""; return; }
      load().then(function (ex) { render(query(ex, q), q); });
    });
  }

  function esc(s) {
    return String(s)
      .replace(/&/g, "&amp;")
      .replace(/"/g, "&quot;")
      .replace(/</g, "&lt;");
  }

  // Index dates are xmlschema UTC midnights; keep the calendar day fixed and
  // only localize the spelling (month names, order) via the document lang.
  function fmtDate(iso) {
    if (!iso) return "";
    var d = new Date(iso);
    if (isNaN(d.getTime())) return iso;
    return d.toLocaleDateString(document.documentElement.lang || undefined, {
      year: "numeric",
      month: "long",
      day: "numeric",
      timeZone: "UTC",
    });
  }

  function render(hits, q) {
    list.textContent = "";
    if (!q) return;
    if (!hits.length) {
      var none = document.createElement("p");
      none.className = "search-none";
      none.textContent = strings().empty;
      list.appendChild(none);
      return;
    }
    hits.forEach(function (h) {
      var a = document.createElement("a");
      a.className = "search-hit";
      a.href = h[0];
      var when = document.createElement("span");
      when.className = "search-hit-date";
      when.textContent = fmtDate(h[2]);
      var title = document.createElement("span");
      title.className = "search-hit-title";
      title.textContent = h[1];
      a.appendChild(when);
      a.appendChild(title);
      list.appendChild(a);
    });
  }

  function open() {
    if (!overlay) build();
    overlay.classList.add("is-open");
    input.focus();
    load(); // warm the wasm+index while the user types
  }

  function close() {
    if (overlay) overlay.classList.remove("is-open");
  }

  window.gs = { open: open, close: close };
  open();
})();
