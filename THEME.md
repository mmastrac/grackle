# Theme: one recursive content chain, and layout as a row face

**Status: in progress (2026-07-28).** Assembly lives in
`crates/core/src/assemble/`. Byte-exactness is not required.

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

Engine part vocabulary is the static `ENGINE` table in `assemble/parts.rs`
(order for null/partial fallback still follows base fragments). A theme's
`.schema.toml` may add fields as parts on `row` (may not retype engine parts).

## 6. Replaces

| today | becomes |
|---|---|
| `document` / `summary` / `link` kinds | `row` + faces |
| `listing` / `link_list` kinds + templates | concat of row faces; furniture on wrapper `row` |
| `layout = "card"` / `"link"` / … | member face of that name (`row--{face}`) |
| `variant` | overrides `layout` as the member face |
| `listing--gallery.html` etc. | `row--gallery.html` (member face) |
| site `[[parts]]` | theme `.schema.toml` |
| `data-slot="main"` | `data-slot="content"` |

## 7. Open / deferred

- **q-theme-a:** fold `light_html` into the chain
- **q50:** deliberate vs forgotten omitted slots
- Whether `variant` stays as an override, or config collapses to one `layout`
  / `face` key once corpora no longer need both.
