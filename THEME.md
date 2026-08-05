# Theme: one recursive content chain, and layout as a row face

**Status: built (2026-07-28).** Assembly lives in `crates/core/src/assemble/`.
Pending work for this design lives only in `TODO-1.0.md`. Byte-exactness is
not required.

## Law

**Layout** = the fragment that turns a part map into the HTML that fills the
parent's `content` hole.

**Slot** = which rung receives that HTML (`document` stack vs `root` chrome).

**Shell** = whether the HTML chain runs at all (`raw` / `html` / `light_html`).

## 1. The chain

```
root_shell (engine)              doctype/<html>/<head>/<body>
  └─ root PartMap                theme chrome
       └─ content
            └─ row PartMap       page furniture (crumbs/title/…) when present
                 └─ content      body HTML — a row's prose, or concatenated
                                 member faces, or an embed splice
```

`slot: root` skips the row furniture rung (ex-`layout: default`).
`shell: light_html` stays a separate map-shell path for now.

## 2. One kind: `row`, many faces

`document`, `summary`, and `link` collapse to **one presence-driven kind**
`row`. Faces are fragment variants:

| face | fragment | was |
|---|---|---|
| *(default)* | `row.html` | `document.html` — full page furniture |
| `card` | `row--card.html` | `summary.html` / `summary--card` |
| `link` | `row--link.html` | `link.html` inside `link_list` |
| `figure` | `row--figure.html` | `summary--figure` |
| `gallery` / `cards` / … | `row--{variant}.html` | `listing--*` / `summary--*` |

Hole algebra deletes absent parts. The schema is the union (title/url/content
plus crumbs, tags, hero, dates, image facts, intro, pagination, relations, …).

View config / `{% view name | face %}` names the **member face**. The pipe
overrides the view's default face per embed site.

## 3. A listing is HTML concatenation

There is no `listing` / `link_list` kind.

Members each render through a row face; the aggregate **content** is those
HTML strings concatenated. Page furniture lives on the **wrapper** row:

- title, crumbs, intro, pagination → set on a `row` PartMap rendered as
  `row.html` (default face)
- that map's `content` → the concatenation
- first-member emphasis (book-of-the-month, etc.) is theme CSS on
  `:first-child`, not a view flag or face

### Routes

A materialized view is a synthetic row:

- **Unclaimed** (has `layout`): wrapper `row.html` whose `content` is concat
  of members through that face. `variant` wins when the theme ships
  `row--{variant}`; otherwise `layout` (partial themes). Missing `layout`
  face is a build error.
- **Claimed** (q45): real row owns the page; body places
  `{% view <self> %}` (optional `| face`); embed is route-aware.
- **Embed-only** faces (`link`, `card`, …): concat (or one card) with **no**
  wrapper furniture — the splice *is* the content contribution.

Fold shells (`atom` / `sitemap` / `search` / script) stay serializations.

## 4. Shell + slot (unchanged from v1)

- `shell: raw` — bytes out
- `shell: html` + absent slot — full chain through `row.html`
- `shell: html` + `slot: root` — body fills theme `content` (ex-`layout: default`)
- `shell: light_html` — unchanged map shell

Row front-matter `layout:` is gone. View `layout` / face vocabulary remains
for aggregates and embeds.

## 5. Schemas

Engine part vocabulary is **derived** at theme load from base + theme
fragments plus declared field schemas (`[schema]` / theme `.schema.toml`).
Stream and map slots must declare their child with `data-fragment` (e.g.
`data-slot="crumbs" data-fragment="crumb"`). There is no handwritten
`parts.toml`. A theme's `.schema.toml` may add fields as parts on `row`
(may not retype existing parts).

### Inline fragment defaults

A stream/map hole may embed the default body of its `data-fragment` target
instead of shipping a separate file. Element children become fragment
`NAME`; whitespace/comments-only children count as empty (definition still
comes from `NAME.html` or is absent). After files are loaded:

1. If `NAME.html` (or any file-backed fragment of that name) already exists,
   inline children are **dropped** - the file wins everywhere that name is
   used.
2. Otherwise the children are **registered** as fragment `NAME` and the
   hole is cleared for render.

```html
<nav data-slot="crumbs" data-fragment="crumb" aria-label="Breadcrumbs">
	<span><a data-slot-href="url" data-slot="label"></a></span>
</nav>
```

Ship `crumb.html` in a theme to overload every `data-fragment="crumb"` site;
omit the file and keep the inline under the parent to stay one file. Same
rule for variants (`data-fragment="row--figure"` with an inline body).

A theme that replaces a parent fragment (e.g. `row.html`) still inherits
inline defaults harvested from the base parent, unless it also ships the
child file or redefines the child inline under the new parent (later inline
wins over earlier; a file always wins over an inline).

## 6. Chrome parts *(specced 2026-08-05; themes/DESIGN.md §10)*

The root part map carries capability parts — `axes` (built), `search`,
`feed`, `scheme`, `profile_notice` — each filled from a declared fact (a
route wearing a fold shell, an axis with members, a theme declaring both
schemes) and deleted by the empty-part rule when the fact is absent. The
base root groups them in a `chrome` cluster slot with an inline default
(`chrome.html` when shipped as a file); **first writer per part** when a
root also places one individually. Default fragments are built from the
chrome primitives (`data-chrome="button" | "dropdown" | "expando"`).
Fill table, stand-down law and checks: themes/DESIGN.md §10.

## 7. Replaces

| today | becomes |
|---|---|
| `document` / `summary` / `link` kinds | `row` + faces |
| `listing` / `link_list` kinds + templates | concat of row faces; furniture on wrapper `row` |
| `layout = "card"` / `"link"` / … | member face of that name (`row--{face}`) |
| `variant` | overrides `layout` as the member face |
| `listing--gallery.html` etc. | `row--gallery.html` (member face) |
| site `[[parts]]` | theme `.schema.toml` |
| `data-slot="main"` | `data-slot="content"` |
