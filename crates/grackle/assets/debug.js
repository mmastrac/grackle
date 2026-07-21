"use strict";

var DB = null;
var IX = null;
var sel = null;
var lens = "tree";
var expanded = { src: new Set(), url: new Set() };
var rowsTable = "posts";
var rowsQuery = "";
var rowsSort = { key: "url", dir: 1 };

var scrollMem = {};

var $ = function (s, r) { return (r || document).querySelector(s); };

/* `/blog/page/10/` sorts after `/blog/page/2/`. The engine's own route order
   is lexical and deliberately so (determinism), and archives look right only
   because `{month:02}` is zero-padded — pagination isn't, and shouldn't be. */
function natCmp(a, b) {
	return String(a).localeCompare(String(b), undefined, { numeric: true, sensitivity: "base" });
}

/* Panes are rebuilt wholesale on every draw, so their scroll offsets have to
   survive outside the DOM or every click jumps the reader back to the top. */
function saveScroll() {
	document.querySelectorAll("[data-scroll]").forEach(function (n) {
		scrollMem[n.dataset.scroll] = n.scrollTop;
	});
}

function restoreScroll() {
	document.querySelectorAll("[data-scroll]").forEach(function (n) {
		if (scrollMem[n.dataset.scroll] != null) n.scrollTop = scrollMem[n.dataset.scroll];
	});
}
var el = function (tag, cls, txt) {
	var n = document.createElement(tag);
	if (cls) n.className = cls;
	if (txt != null) n.textContent = txt;
	return n;
};

/* ---- indexes: everything the lenses join on ------------------------- */

function index(db) {
	var ix = {
		rows: new Map(),
		routeByUrl: new Map(),
		routeBySource: new Map(),
		pickups: new Map(),
		allRows: []
	};
	["posts", "pages", "objects"].forEach(function (t) {
		db[t].forEach(function (r) {
			ix.allRows.push(r);
			if (!ix.rows.has(r.url)) ix.rows.set(r.url, r);
		});
	});
	db.routes.forEach(function (rt) {
		ix.routeByUrl.set(rt.url, rt);
		if (rt.source) ix.routeBySource.set(rt.source, rt);
		(rt.members || []).forEach(function (u) {
			if (!ix.pickups.has(u)) ix.pickups.set(u, []);
			ix.pickups.get(u).push(rt);
		});
	});
	return ix;
}

/* A row's route: matched on source path, because a claimed row's URL is
   its landing's and would otherwise look like it owns a route it doesn't. */
function routeOf(row) {
	if (row.claimed) return null;
	var byPath = IX.routeBySource.get(row.path);
	if (byPath) return byPath;
	var byUrl = IX.routeByUrl.get(row.url);
	if (byUrl && byUrl.source) return byUrl;
	return null;
}

function flagsOf(row) {
	var f = [];
	if (row.draft) f.push(["draft", "warn"]);
	if (row.hidden) f.push(["hidden", "warn"]);
	if (row.noindex) f.push(["noindex", "warn"]);
	if (row.claimed) f.push(["claimed", ""]);
	if (row.table === "pages" && !row.rendered) f.push(["passthrough", ""]);
	if (row.shell) f.push(["shell:" + row.shell, ""]);
	if (row.locale && DB.site.locales.length && row.locale !== DB.site.default_locale)
		f.push([row.locale, ""]);
	return f;
}

function flagSpan(f) {
	var s = el("span", "flag" + (f[1] ? " " + f[1] : ""), f[0]);
	return s;
}

/* ---- trees ---------------------------------------------------------- */

function buildTree(items, keyFn) {
	var root = { name: "", kids: new Map(), n: 0, item: null };
	items.forEach(function (it) {
		var key = keyFn(it);
		if (key == null) return;
		var parts = key.split("/").filter(Boolean);
		var node = root;
		root.n++;
		parts.forEach(function (p, i) {
			if (!node.kids.has(p)) {
				node.kids.set(p, { name: p, kids: new Map(), n: 0, item: null, path: parts.slice(0, i + 1).join("/") });
			}
			node = node.kids.get(p);
			node.n++;
			if (i === parts.length - 1) node.item = it;
		});
	});
	return root;
}

function renderTree(root, which, onPick) {
	var wrap = el("div");
	if (!expanded[which]) expanded[which] = new Set();
	var exp = expanded[which];
	function walk(node, depth) {
		var kids = Array.from(node.kids.values()).sort(function (a, b) {
			var ad = a.kids.size > 0, bd = b.kids.size > 0;
			if (ad !== bd) return ad ? -1 : 1;
			return natCmp(a.name, b.name);
		});
		kids.forEach(function (k) {
			var isDir = k.kids.size > 0;
			var isOpen = exp.has(k.path);
			var n = el("div", "node");
			n.dataset.node = k.path;
			n.style.paddingLeft = 12 + depth * 13 + "px";

			// A node can be both: `/blog/` is blog_index's own route AND the
			// parent of every archive under it. Conflating "has children"
			// with "is a folder" made every landing — the most interesting
			// routes on the site — unselectable, so the twisty owns
			// expansion and the label owns selection.
			var tw = el("span", "tw", isDir ? (isOpen ? "\u2212" : "+") : "");
			if (isDir) {
				tw.onclick = function (e) {
					e.stopPropagation();
					if (isOpen) exp.delete(k.path); else exp.add(k.path);
					draw();
				};
			}
			n.appendChild(tw);

			n.appendChild(el("span", "lbl", k.name + (isDir ? "/" : "")));
			if (k.item) {
				flagsOf(k.item).slice(0, 2).forEach(function (f) { n.appendChild(flagSpan(f)); });
				if (k.item.view) n.appendChild(el("span", "flag view", k.item.view));
			} else {
				n.classList.add("bare");
			}
			if (isDir) n.appendChild(el("span", "n", String(k.n)));
			if (k.item && sel && sel.kind === "row" && sel.key === k.item.url + "|" + k.item.path)
				n.dataset.sel = "1";
			if (k.item && sel && sel.kind === "route" && sel.key === k.item.url)
				n.dataset.sel = "1";

			n.onclick = function (e) {
				e.stopPropagation();
				if (k.item) { onPick(k.item); return; }
				if (isDir) {
					if (isOpen) exp.delete(k.path); else exp.add(k.path);
					draw();
				}
			};
			wrap.appendChild(n);
			if (isDir && isOpen) walk(k, depth + 1);
		});
	}
	walk(root, 0);
	return wrap;
}

function lensTree() {
	var host = el("div", "lens-host");
	var panes = el("div", "panes");

	var srcRoot = buildTree(IX.allRows, function (r) { return r.path; });
	var urlRoot = buildTree(DB.routes, function (r) { return r.url; });

	var p1 = el("div", "pane");
	var h1 = el("h2");
	h1.appendChild(el("span", null, "source"));
	h1.appendChild(el("span", null, IX.allRows.length + " rows"));
	p1.appendChild(h1);
	var b1 = el("div", "body");
	b1.dataset.scroll = "tree-src";
	b1.appendChild(renderTree(srcRoot, "src", function (row) {
		sel = { kind: "row", key: row.url + "|" + row.path, row: row };
		draw();
	}));
	p1.appendChild(b1);

	var p2 = el("div", "pane");
	var h2 = el("h2");
	h2.appendChild(el("span", null, "urls"));
	h2.appendChild(el("span", null, DB.routes.length + " routes"));
	p2.appendChild(h2);
	var b2 = el("div", "body");
	b2.dataset.scroll = "tree-url";
	b2.appendChild(renderTree(urlRoot, "url", function (rt) {
		sel = { kind: "route", key: rt.url, route: rt };
		draw();
	}));
	p2.appendChild(b2);

	panes.appendChild(p1);
	var gut = el("div", "gutter");
	gut.id = "gutter";
	panes.appendChild(gut);
	panes.appendChild(p2);

	b1.onscroll = drawGutter;
	b2.onscroll = drawGutter;
	// Wheel anywhere between the trees drives the nearer one. The handler
	// belongs on the container, not the gutter: the grid gaps flanking it
	// are 10px of no element at all, and a wheel just right of the left
	// scrollbar landed in one of them and did nothing.
	panes.onwheel = function (e) {
		if (b1.contains(e.target) || b2.contains(e.target)) return;
		var gap = function (el2) {
			var r = el2.getBoundingClientRect();
			return e.clientX < r.left ? r.left - e.clientX
				: e.clientX > r.right ? e.clientX - r.right : 0;
		};
		var target = gap(b1) <= gap(b2) ? b1 : b2;
		target.scrollTop += e.deltaY;
		e.preventDefault();
	};

	host.appendChild(panes);
	return host;
}

/* ---- the gutter: where the selection lives on both sides ------------ */

/* A tree only renders what is expanded, so a target inside a collapsed
   branch has no element. Walking up to the nearest rendered ancestor is the
   honest answer: it points at the folder to open, not at nothing. */
function nodeFor(pane, path) {
	var parts = path.split("/").filter(Boolean);
	while (parts.length) {
		var hit = pane.querySelector('[data-node="' + parts.join("/") + '"]');
		if (hit) return { el: hit, exact: parts.length === path.split("/").filter(Boolean).length };
		parts.pop();
	}
	return null;
}

function treePath(url) {
	return String(url).split("/").filter(Boolean).join("/");
}

/* Which (source, url) pairs does the current selection imply? */
function gutterPairs() {
	if (!sel) return [];
	if (sel.kind === "row") {
		var rt = routeOf(sel.row);
		return rt ? [[sel.row.path, rt.url]] : [];
	}
	if (sel.kind === "route") {
		var rt2 = sel.route;
		if (rt2.members && rt2.members.length) {
			return rt2.members.slice(0, 40).map(function (u) {
				var row = IX.rows.get(u);
				return row ? [row.path, u] : null;
			}).filter(Boolean);
		}
		if (rt2.source) return [[rt2.source, rt2.url]];
	}
	return [];
}

function drawGutter() {
	var gut = document.getElementById("gutter");
	if (!gut) return;
	gut.textContent = "";
	var panes = document.querySelectorAll(".pane .body");
	if (panes.length < 2) return;
	var pairs = gutterPairs();
	if (!pairs.length) return;

	var gb = gut.getBoundingClientRect();
	var svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
	svg.setAttribute("width", gb.width);
	svg.setAttribute("height", gb.height);

	function side(pane, path) {
		var found = nodeFor(pane, path);
		if (!found) return null;
		var r = found.el.getBoundingClientRect();
		var pb = pane.getBoundingClientRect();
		var y = r.top + r.height / 2 - gb.top;
		var off = r.bottom < pb.top ? -1 : (r.top > pb.bottom ? 1 : 0);
		return { y: Math.max(6, Math.min(gb.height - 6, y)), off: off, exact: found.exact };
	}

	pairs.slice(0, 40).forEach(function (pair) {
		var L = side(panes[0], pair[0]);
		var R = side(panes[1], treePath(pair[1]));
		if (!L || !R) return;
		var w = gb.width;
		var faint = !L.exact || !R.exact ? " faint" : "";

		var path = document.createElementNS("http://www.w3.org/2000/svg", "path");
		var mid = w / 2;
		path.setAttribute("d", "M 8 " + L.y + " C " + mid + " " + L.y + ", " + mid + " " + R.y + ", " + (w - 8) + " " + R.y);
		path.setAttribute("class", "gline" + faint);
		svg.appendChild(path);

		[[8, L, -1], [w - 8, R, 1]].forEach(function (a) {
			var x = a[0], s2 = a[1], dir = a[2];
			var tri = document.createElementNS("http://www.w3.org/2000/svg", "polygon");
			var pts;
			// An offscreen target gets an up/down head instead: the arrow
            // stops meaning "over there" and starts meaning "scroll".
			if (s2.off < 0) pts = (x) + "," + (s2.y - 5) + " " + (x - 4) + "," + (s2.y + 3) + " " + (x + 4) + "," + (s2.y + 3);
			else if (s2.off > 0) pts = (x) + "," + (s2.y + 5) + " " + (x - 4) + "," + (s2.y - 3) + " " + (x + 4) + "," + (s2.y - 3);
			else if (dir < 0) pts = (x - 5) + "," + s2.y + " " + (x + 3) + "," + (s2.y - 4) + " " + (x + 3) + "," + (s2.y + 4);
			else pts = (x + 5) + "," + s2.y + " " + (x - 3) + "," + (s2.y - 4) + " " + (x - 3) + "," + (s2.y + 4);
			tri.setAttribute("points", pts);
			tri.setAttribute("class", "ghead" + faint);
			svg.appendChild(tri);
		});
	});
	gut.appendChild(svg);
}

/* ---- rows ----------------------------------------------------------- */

var COLS = {
	posts: ["url", "title", "date", "path", "tags", "flags"],
	pages: ["url", "title", "path", "layout", "shell", "theme", "flags"],
	objects: ["url", "path", "size"]
};

function cellText(row, col) {
	if (col === "flags") return "";
	if (col === "tags") return (row.tags || []).join(" ");
	if (col === "size") return row.size == null ? "" : String(row.size);
	return row[col] == null ? "" : String(row[col]);
}

function lensRows() {
	var host = el("div", "lens-host");
	var ctl = el("div", "controls");

	var selT = el("select");
	["posts", "pages", "objects"].forEach(function (t) {
		var o = el("option", null, t + " (" + DB[t].length + ")");
		o.value = t;
		if (t === rowsTable) o.selected = true;
		selT.appendChild(o);
	});
	selT.onchange = function () { rowsTable = selT.value; draw(); };

	var q = el("input");
	q.type = "search";
	q.placeholder = "filter url, path, title…";
	q.value = rowsQuery;
	q.oninput = function () { rowsQuery = q.value; draw(); };

	ctl.appendChild(el("label", null, "table"));
	ctl.appendChild(selT);
	ctl.appendChild(q);
	var count = el("span", "count");
	ctl.appendChild(count);
	host.appendChild(ctl);

	var data = DB[rowsTable].slice();
	if (rowsQuery.trim()) {
		var needle = rowsQuery.toLowerCase();
		data = data.filter(function (r) {
			return (r.url + " " + r.path + " " + (r.title || "")).toLowerCase().indexOf(needle) >= 0;
		});
	}
	data.sort(function (a, b) {
		return natCmp(cellText(a, rowsSort.key), cellText(b, rowsSort.key)) * rowsSort.dir;
	});

	var shown = data.slice(0, 500);
	count.textContent = shown.length < data.length
		? "showing " + shown.length + " of " + data.length
		: data.length + " rows";

	var pane = el("div", "pane");
	var body = el("div", "body");
	body.dataset.scroll = "rows-" + rowsTable;
	var tbl = el("table");
	var thead = el("thead");
	var tr = el("tr");
	COLS[rowsTable].forEach(function (c) {
		var th = el("th", null, c);
		if (rowsSort.key === c) th.appendChild(el("span", "ar", rowsSort.dir > 0 ? " ↑" : " ↓"));
		th.onclick = function () {
			if (rowsSort.key === c) rowsSort.dir *= -1;
			else { rowsSort.key = c; rowsSort.dir = 1; }
			draw();
		};
		tr.appendChild(th);
	});
	thead.appendChild(tr);
	tbl.appendChild(thead);

	var tb = el("tbody");
	shown.forEach(function (r) {
		var row = el("tr");
		if (sel && sel.kind === "row" && sel.key === r.url + "|" + r.path) row.dataset.sel = "1";
		COLS[rowsTable].forEach(function (c) {
			var td = el("td");
			if (c === "flags") flagsOf(r).forEach(function (f) { td.appendChild(flagSpan(f)); });
			else td.textContent = cellText(r, c);
			row.appendChild(td);
		});
		row.onclick = function () {
			sel = { kind: "row", key: r.url + "|" + r.path, row: r };
			draw();
		};
		tb.appendChild(row);
	});
	tbl.appendChild(tb);
	body.appendChild(tbl);
	pane.appendChild(body);
	host.appendChild(pane);
	return host;
}

/* ---- views ---------------------------------------------------------- */

function lensViews() {
	var host = el("div", "lens-host");
	var pane = el("div", "pane");
	var body = el("div", "body");
	body.dataset.scroll = "views";
	var tbl = el("table");
	var thead = el("thead");
	var tr = el("tr");
	["view", "over", "base", "layout", "group_by", "paginate", "filter", "routes"].forEach(function (c) {
		tr.appendChild(el("th", null, c));
	});
	thead.appendChild(tr);
	tbl.appendChild(thead);
	var tb = el("tbody");
	DB.views.slice().sort(function (a, b) {
		return b.route_count - a.route_count || natCmp(a.name, b.name);
	}).forEach(function (v) {
		var row = el("tr");
		if (sel && sel.kind === "view" && sel.key === v.name) row.dataset.sel = "1";
		[v.name, v.over || "", v.base || "", v.layout || v.shell || "", v.group_by || "",
			v.paginate == null ? "" : String(v.paginate), v.filter || "", String(v.route_count)]
			.forEach(function (t) { row.appendChild(el("td", null, t)); });
		row.onclick = function () { sel = { kind: "view", key: v.name, view: v }; draw(); };
		tb.appendChild(row);
	});
	tbl.appendChild(tb);
	body.appendChild(tbl);
	pane.appendChild(body);
	host.appendChild(pane);
	return host;
}

/* ---- diagnose: the lens that answers "why isn't this here" ----------- */

function findings() {
	var out = [];
	IX.allRows.forEach(function (r) {
		if (r.table === "objects") return;
		var rt = routeOf(r);
		if (!rt && !r.claimed) {
			out.push({ row: r, why: "no route — excluded by a rule, or its collection routes nothing" });
		}
		if (r.claimed) {
			out.push({ row: r, why: "claimed — a view owns this URL, so the row has no route of its own" });
		}
		if (r.rendered !== false && !r.title) {
			out.push({ row: r, why: "no title — listings and search will show a bare URL" });
		}
		if (r.draft) out.push({ row: r, why: "draft — routed and rendered, but every view's !draft filter excludes it" });
		// Undated is the DEFINED state for a draft (it gets its date when it
		// publishes), so only a publishable row is a finding here. For one,
		// the cost is silent: no year/month archive membership, a trail that
		// stops at the collection, and last place in every ordering.
		if (r.table === "posts" && !r.draft && !r.date)
			out.push({ row: r, why: "no date — absent from year and month archives, no date trail, sorts last" });
		if (r.hidden) out.push({ row: r, why: "hidden — served, but out of listings, sitemap and search" });
		if (r.noindex) out.push({ row: r, why: "noindex — served and listed, but asks search engines away" });
	});
	DB.routes.forEach(function (rt) {
		if (rt.view && rt.members && rt.members.length === 0 && rt.page == null) {
			out.push({ route: rt, why: "view route with no members — an empty page shipped" });
		}
	});
	return out;
}

function lensDiag() {
	var host = el("div", "lens-host");
	var f = findings();
	var wrap = el("div", "diag");
	wrap.dataset.scroll = "diag";
	if (!f.length) wrap.appendChild(el("p", "none", "Nothing unusual."));
	f.slice(0, 300).forEach(function (item) {
		var row = el("div", "row");
		var what = item.row ? (item.row.path || item.row.url) : item.route.url;
		row.appendChild(el("code", null, what));
		row.appendChild(el("span", "why", item.why));
		row.onclick = function () {
			sel = item.row
				? { kind: "row", key: item.row.url + "|" + item.row.path, row: item.row }
				: { kind: "route", key: item.route.url, route: item.route };
			lens = item.row ? "rows" : "tree";
			draw();
		};
		wrap.appendChild(row);
	});
	host.appendChild(wrap);
	return host;
}

/* ---- detail: provenance --------------------------------------------- */

function chain() {
	var c = el("div", "chain");
	for (var i = 0; i < arguments.length; i++) {
		if (i) c.appendChild(el("span", "arrow", "→"));
		var a = arguments[i];
		var b = el("span", "box" + (a[1] ? " derived" : ""), a[0]);
		c.appendChild(b);
	}
	return c;
}

function kv(pairs) {
	var d = el("dl", "kv");
	pairs.forEach(function (p) {
		if (p[1] == null || p[1] === "" || (Array.isArray(p[1]) && !p[1].length)) return;
		d.appendChild(el("dt", null, p[0]));
		var dd = el("dd");
		if (p[1] instanceof Node) dd.appendChild(p[1]); else dd.textContent = String(p[1]);
		d.appendChild(dd);
	});
	return d;
}

function openLink(url) {
	var a = el("a", null, url);
	a.href = url;
	a.target = "_blank";
	a.rel = "noreferrer";
	return a;
}

function pillList(items, onPick) {
	var w = el("div", "pickups");
	items.forEach(function (it) {
		var p = el("button", "pill", it.label);
		p.onclick = function () { onPick(it); };
		w.appendChild(p);
	});
	return w;
}

function detailRow(row) {
	var d = el("div");
	d.appendChild(el("h3", null, row.title || row.path));
	d.appendChild(el("div", "sub", row.table + " · " + row.path));

	var rt = routeOf(row);
	if (rt) d.appendChild(chain([row.path], [rt.url]));
	else if (row.claimed) d.appendChild(chain([row.path], ["claimed — no route of its own", 1]));
	else d.appendChild(chain([row.path], ["no route", 1]));

	var fl = el("div");
	flagsOf(row).forEach(function (f) { fl.appendChild(flagSpan(f)); });

	d.appendChild(kv([
		["url", row.url],
		["locale", row.locale],
		["date", row.date],
		["tags", (row.tags || []).join(", ")],
		["layout", row.layout],
		["shell", row.shell],
		["theme", row.theme],
		["size", row.size == null ? "" : row.size + " bytes"],
		["flags", flagsOf(row).length ? fl : ""],
		["open", rt || !row.claimed ? openLink(row.url) : ""]
	]));

	if (row.fields && row.fields.length) {
		var s = el("div", "sec");
		s.appendChild(el("h4", null, "schema fields"));
		s.appendChild(kv(row.fields.map(function (f) { return [f[0], f[1]]; })));
		d.appendChild(s);
	}

	var picks = IX.pickups.get(row.url) || [];
	var s2 = el("div", "sec");
	s2.appendChild(el("h4", null, "picked up by (" + picks.length + ")"));
	if (!picks.length) {
		s2.appendChild(el("p", "none", "No view lists this row."));
	} else {
		s2.appendChild(pillList(picks.map(function (rt) {
			return { label: (rt.view || "?") + " · " + rt.url, route: rt };
		}), function (it) {
			sel = { kind: "route", key: it.route.url, route: it.route };
			draw();
		}));
	}
	d.appendChild(s2);
	return d;
}

function detailRoute(rt) {
	var d = el("div");
	d.appendChild(el("h3", null, rt.url));
	d.appendChild(el("div", "sub", rt.kind + (rt.view ? " · view " + rt.view : "")));

	if (rt.source) d.appendChild(chain([rt.source], [rt.url]));
	else if (rt.view) d.appendChild(chain(["view " + rt.view, 1], [rt.url]));

	d.appendChild(kv([
		["kind", rt.kind],
		["view", rt.view],
		["key", rt.key],
		["page", rt.page],
		["locale", rt.locale],
		["params", (rt.params || []).map(function (p) { return p[0] + "=" + p[1]; }).join(" ")],
		["source", rt.source],
		["open", openLink(rt.url)]
	]));

	var mem = rt.members || [];
	if (rt.view) {
		var s = el("div", "sec");
		s.appendChild(el("h4", null, "members (" + mem.length + ")"));
		if (!mem.length) s.appendChild(el("p", "none", "No members."));
		else s.appendChild(pillList(mem.slice().sort(natCmp).slice(0, 120).map(function (u) {
			return { label: u, url: u };
		}), function (it) {
			var row = IX.rows.get(it.url);
			if (row) { sel = { kind: "row", key: row.url + "|" + row.path, row: row }; draw(); }
		}));
		d.appendChild(s);
	}

	var srcRow = rt.source ? IX.allRows.find(function (r) { return r.path === rt.source; }) : null;
	if (srcRow) {
		var s2 = el("div", "sec");
		s2.appendChild(el("h4", null, "row"));
		s2.appendChild(pillList([{ label: srcRow.path, row: srcRow }], function (it) {
			sel = { kind: "row", key: it.row.url + "|" + it.row.path, row: it.row };
			draw();
		}));
		d.appendChild(s2);
	}
	return d;
}

function detailView(v) {
	var d = el("div");
	d.appendChild(el("h3", null, v.name));
	d.appendChild(el("div", "sub", "view · " + v.route_count + " routes"));
	d.appendChild(chain(
		[v.base || "?"],
		[(v.filter ? "filter " + v.filter : "no filter"), 1],
		[(v.group_by ? "group by " + v.group_by : "ungrouped"), 1],
		[v.route_count + " routes"]
	));
	d.appendChild(kv([
		["over", v.over],
		["base table", v.base],
		["layout", v.layout],
		["shell", v.shell],
		["filter", v.filter],
		["group_by", v.group_by],
		["paginate", v.paginate]
	]));
	if (v.routes.length) {
		var s = el("div", "sec");
		s.appendChild(el("h4", null, "routes (" + v.route_count + ")"));
		var routes = v.routes.slice().sort(natCmp).map(function (u) {
			return IX.routeByUrl.get(u) || { url: u };
		});
		var tree = buildTree(routes, function (r) { return r.url; });
		// A detail tree is already a filtered answer, so open it: collapsed
		// to a single `blog/` node it hides the very thing that was asked for.
		var key = "view:" + v.name;
		if (!expanded[key]) {
			expanded[key] = new Set();
			(function seed(node) {
				node.kids.forEach(function (k) {
					if (k.kids.size) { expanded[key].add(k.path); seed(k); }
				});
			})(tree);
		}
		s.appendChild(renderTree(tree, key, function (rt) {
			var real = IX.routeByUrl.get(rt.url);
			if (real) { sel = { kind: "route", key: real.url, route: real }; draw(); }
		}));
		d.appendChild(s);
	}
	return d;
}

/* ---- shell ---------------------------------------------------------- */

var LEGEND = {
	tree: "The same corpus in its two shapes: the filesystem's on the left, the URL space's on the right. " +
		"A row and its route are rarely the same shape — that difference is the route template.",
	rows: "Every row of a table, with the flags that decide where it can appear.",
	views: "Every query the site declares, and how far each one fans out. A handful of views account for most of the URL space.",
	diagnose: "Everything the database can tell you is unusual. Most entries are deliberate — the list is for recognising the one that isn't."
};

var LENSES = [
	["tree", "tree", function () { return null; }],
	["rows", "rows", function () { return DB.posts.length + DB.pages.length + DB.objects.length; }],
	["views", "views", function () { return DB.views.length; }],
	["diagnose", "diagnose", function () { return findings().length; }]
];

function draw() {
	saveScroll();
	$("#site-title").textContent = DB.site.title + " · " + DB.site.url;

	var c = $("#counts");
	c.textContent = "";
	[["posts", DB.stats.posts], ["pages", DB.stats.pages], ["objects", DB.stats.objects],
		["routes", DB.stats.routes]].forEach(function (p) {
		var s = el("span");
		s.appendChild(el("b", null, String(p[1])));
		s.appendChild(document.createTextNode(" " + p[0]));
		c.appendChild(s);
	});

	var tabs = $("#lenses");
	tabs.textContent = "";
	LENSES.forEach(function (L) {
		var b = el("button", "lens", L[1]);
		b.setAttribute("role", "tab");
		b.setAttribute("aria-selected", lens === L[0] ? "true" : "false");
		var n = L[2]();
		if (n != null) b.appendChild(el("span", "n", String(n)));
		b.onclick = function () { lens = L[0]; draw(); };
		tabs.appendChild(b);
	});

	var host = $("#view");
	host.textContent = "";
	host.appendChild(
		lens === "tree" ? lensTree() :
		lens === "rows" ? lensRows() :
		lens === "views" ? lensViews() : lensDiag()
	);

	$("#legend").textContent = LEGEND[lens] || "";

	var det = $("#detail");
	det.dataset.scroll = "detail";
	det.textContent = "";
	if (!sel) det.appendChild(el("p", "empty", "Select a row, route or view."));
	else if (sel.kind === "row") det.appendChild(detailRow(sel.row));
	else if (sel.kind === "route") det.appendChild(detailRoute(sel.route));
	else det.appendChild(detailView(sel.view));

	restoreScroll();
	drawGutter();
}

/* Re-resolve the selection against a fresh payload: the object identity is
   gone after a rebuild, but the key survives, so the pane keeps its place
   through an edit. */
function reselect() {
	if (!sel) return;
	if (sel.kind === "row") {
		var r = IX.allRows.find(function (x) { return x.url + "|" + x.path === sel.key; });
		sel = r ? { kind: "row", key: sel.key, row: r } : null;
	} else if (sel.kind === "route") {
		var rt = IX.routeByUrl.get(sel.key);
		sel = rt ? { kind: "route", key: sel.key, route: rt } : null;
	} else {
		var v = DB.views.find(function (x) { return x.name === sel.key; });
		sel = v ? { kind: "view", key: sel.key, view: v } : null;
	}
}

function load(first) {
	return fetch("/__debug/site.json", { cache: "no-store" })
		.then(function (r) { return r.json(); })
		.then(function (db) {
			DB = db;
			IX = index(db);
			if (first) {
				["_posts", "recipes", "books", "demos"].forEach(function (d) { expanded.src.add(d); });
				DB.routes.slice(0, 400).forEach(function (r) {
					var top = r.url.split("/").filter(Boolean)[0];
					if (top) expanded.url.add(top);
				});
			} else {
				reselect();
			}
			draw();
		});
}

/* The inspector rides the same version the live-reload script polls, so it
   refreshes with the site instead of going stale beside it. */
function watch() {
	var last = null;
	setInterval(function () {
		fetch("/__grackle/version", { cache: "no-store" })
			.then(function (r) { return r.text(); })
			.then(function (v) {
				if (last === null) { last = v; return; }
				if (v !== last) { last = v; load(false); }
			})
			.catch(function () {});
	}, 1000);
}

load(true).then(watch);
