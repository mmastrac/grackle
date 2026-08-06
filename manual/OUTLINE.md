# The grackle manual — outline

Status: outline only. Prose at release. `★` = not built (flag in text).
`§` → `DESIGN.md` (design authority; manual is the user-facing projection).

---

## 0. Shape of the deliverable

- Manual is a grackle site: `grackle/manual/` (own `grackle.toml`, own theme).
- Examples = engineering instruments (`field-notes/`, `minimal/`); manual = plausible site.

```
grackle/manual/
  grackle.toml
  .section                 # chapter tree → nav
  .slots/nav.md
  index.md                 # landing, claims view:chapters (mode B, §5h)
  start/*.md               # Part I
  shape/*.md               # Part II
  scale/*.md               # Part III
  reference/*.md           # Part IV
  _posts/*.md              # release notes → /news/, /atom.xml
  themes/manual/
```

### Dogfood

| feature | use |
|---|---|
| tree + pages | every chapter |
| posts + archives + feed | release notes |
| `.section` + `order:` | chapter nav, path axis (§6e) |
| `toc:` via marker | long chapters, heading axis (§6e) |
| widgets | `★` / `note` boxes (§5d) |
| row + `view:` links (strict default) | cross-refs (§6a) |
| `[sets]` / `[routes]` | `chapters` set; `/reference/`, `/news/` |
| landings mode B | `/reference/`, `/` |
| `.schema.toml` | `reference/` → `since:`, `status:` |
| CEL fields | `summary`/`toc`/`hero` on `/news/` |
| row faces | `card` on `/news/`; `row.html` for chapters |
| row `shell:` (`raw`/`html`) | one verbatim imported artifact |
| base config (`extends`) | near-empty `grackle.toml` (§4d) |
| axes | shell-axis md twin per page (q53) |
| sidecars | title/alt for non-FM files |
| profiles + `_drafts` | in-progress chapters (§4a) |
| search fold | manual search |
| root `.style.scss` | accent token override |
| themes | differences over base only |
| inspector | `/__debug/` source‖URL trees (§7c) |
| i18n | `[axes.locale]` + `.fr.md` chapter |

- No engine special-casing for the manual.
- What the manual can't express → §11 finding, not a workaround.

### Voice / pedagogy

1. Files before concepts.
2. Database framing once in ch. 1; quiet until Part III.
3. One law, three shapes (§4d): per-key merge, merge-by-source, shadow-by-name — named in ch. 4, referenced forever.
4. Show one real load-time error per mechanism chapter.
5. `★` everywhere; ch. 36 for unbuilt. No aspirational present tense.
6. No Part I → Part III forward refs.

---

## Part I — Publish something (blog + pages)

Reader: Jekyll/Hugo or none. Running site by ch. 3; no "view" before ch. 6.

### 1. What grackle is
- Site = database in git; theme = stylesheet with placement opinions.
- Pipeline map (unexplained): `file → row → query → doc model → parts → slots → CSS → URL`.
- Gets: no template logic, load-time errors, ~225ms builds.
- Not: comments, memberships, live data, dynamic anything (§7b → ch. 34).
- Part I alone ⇒ working blog.

### 2. Install and the commands
- `cargo build --release`; site = dir with `grackle.toml`.
- `build`, `serve` (watch + reload), `query`, `explain <url>`.
- `urls --against <dir>` — URL-set parity (ch. 25): missing ⇒ exit non-zero; extra ⇒ report only. Body `diff` is spot-check, not a gate.
- Build ~0.4s. Jekyll retired (say once).
- `serve`: full rebuild ~0.3s, poll reload; no SSE/TLS ★(v2).
- Write manual to `grackle explain`; today `query explain`; ★ top-level alias pre-1.0.
- ★ `grackle config --effective` — merged config + per-key provenance (beside `explain`).

### 3. Your first site: nothing but content
- Minimum `grackle.toml` is empty. Base supplies collections, `published`, `/`, `/blog/`, feed, sitemap (`examples/minimal`).
- Steps: directory → `about.md` + `_posts/2026-07-19-hello.md` → `serve` → styled blog + feed.
- First real write: `[site]` (title/url/author).
- Favicon: root `favicon.svg`/`.png`/`.ico`/`.webp`/`.gif` auto-linked; else no `<link>`. Elsewhere → named object route (ch. 10). Touch icons → `[html.head.link]` table form.
- Front matter: `title:` only; filename ⇒ `(date, slug)`.
- Framing: file → row → rule → route; rule from base. Inherit, then override.

### 4. Front matter, defaults, and the one precedence law
- Front matter wins; `permalink:` overrides every rule route (also 33d).
- Rules: `defaults = {…}` for omitted keys.
- Three shapes (§4d):
  - **scalars / settings** (`[site]`, FM, `defaults`) — per key; nearest wins (FM → tree → config).
  - **collections** — merge by source; site rules prepend → first-writer-wins for routes.
  - **registries** (`[sets]`, `[routes]`, `[markers]`, `[widgets]`, `[records]`, …) — shadow by name, whole entry (same as theme fragments, ch. 14).
- Recurs: routing, markers, overlays, slots, object refs, `extends`, theme inheritance.
- `grackle explain` → which rule wrote which key.

### 5. Routes: deciding where files land
- One token supplier: `{path} {dir} {stem} {name} {ext}` always; `{slug}` always; `{year} {month:02} {day}` if dated — any collection. Post may route anywhere (`_posts/rust/hello.md` → `/rust/hello/`).
- Extractor is rule key `file` (was `filename_formats`): `file = ["{date.year}-…-{slug}"]`; first match; partial OK (`["{slug}"]` for undated). Collection `file` = default for rules.
- Rules ordered; first writer wins; catch-all `**` last.
- Rule selects on a fact and claims matches: `front_matter = true` ⇒ identity block / pretty URL; else literal path. Glob = claimed extensions; unclaimed ≠ content. Render gate separate (ch. 19): `front_mattered || shell ∈ {html, light_html}`.
- `[[collections]]` = array of sources, no `kind`. `source` dir or `name` + rules. "objects" = asset-ext rules + `shell = "raw"` (+ `on_demand`). Base already declares posts/drafts/objects/tree — mostly read + prepend.
- Four content layers: `.gitignore`; dot/underscore skip (unless `source`); `exclude` (tree key); **position** (`grackle.toml`, `themes/` always read). Escape: `include` / `included_dir`.
- One route per row (except axes, ch. 20). Errors: two rows / one URL; one row / two URLs; dated route on undated; dead-rule warning. Legal 0: claimed landing (ch. 18) or unreferenced on-demand. `grackle urls` lists result.
- Note: `shell:` has two axes — row vs fold (ch. 19); disjoint.

### 6. Your first query: a set, then a route
- No loops: name a set. Base already has `published`, `/`, `/blog/`.
- Set = query; route = query that lands. `path`/`paths` present ⇒ route (+ landing keys: `title crumb shell paginate group_by template content intro featured`); else set. Shared: `from where match order_by limit layout variant`.

  ```toml
  [sets.published]
  from = ["posts", "drafts"]
  where = "!draft && !hidden"
  order_by = "-date"
  [routes.blog_index]
  from = "published"
  paginate = 5
  paths = ["/blog/", "/blog/page/{n}/"]
  ```

- `from` = composition (was `over`): collection, `*`, set, or route (grouped route ⇒ subdivision, ch. 8). `where` = filter (was `filter`); bare field = truthiness. `match` = separate source-path glob.
- `from` exact; unions spelled as lists. Union = collections only, same kind (else load error → `where`).
- `published` = one post-list definition for feed/tags/archives/home.
- error: `unknown field 'drafts' (did you mean 'draft'?)`.

### 7. Listings that don't ship the whole blog
- Problem: full bodies hidden by CSS.
- Computed field = CEL: `[sets.published.fields] summary = 'truncate_chars(truncate_blocks(content, 4), 700)'`. TOML string = expr; other types = literal. Same surface as `where`/`rank`/head (§5f).
- Stock: `toc = 'outline(content, 3)'`; `hero = 'cover ? cover : image ? image : images(content)[0]'`.
- Fields inherit down `from`; nearest wins.
- `summary` → card face (ch. 14); no summary ⇒ full bodies. `/blog/` 160 KB → 15.7 KB. `truncate_*` ⇒ `truncated` ⇒ `data-truncated`.
- `Content` type: `html` / `markdown` / `text`; `as_html`, `word_count`; `truncate_*`/`outline` HTML-only. Helpers: `filter_blocks`, `keep_blocks`, `links`, `images`, list index.

### 8. Tags and archives for free
- `[routes.tag_index]` `group_by = "tags"` + `path = "/blog/tags/{key}/"`.
- Any typed field groups; list fields multi-key.
- `title` / `crumb` templates over group params (`{key}`, `{year}`, `@months[{month}]`, …).
- Subdivision: grouped route `from` another grouped route; keys accumulate. No new keyword.
- Crumbs from nesting — see ch. 18 (breadcrumb sources).
- Pages group too (one row type): `date` on tree page ⇒ `group_by = "date.year"` (ch. 22/24).
- Group + paginate over every base (one materializer); objects: `group_by = "ext"`.
- Paginate inside each partition. Not q30 (paginate × subdivision still refused) — grouped+paginated stays a leaf.
- error: paginated view with single `path` (not `paths`).

### 9. Feeds, sitemap, and fold shells
- Base ships `/atom.xml`, `/sitemap.xml`. `shell` = how route serializes (`atom`/`sitemap`); full treatment ch. 19.
- Fold shells may omit `from` (whole site). `from = "*"` retired. Listing with no `from` = load error naming folds.
- Inherited-empty silent (no `_posts/` ⇒ no feed). Declared-empty still materializes.
- Footgun: fold sees every output; sitemap must `where`-filter flags itself. Profiles (ch. 24) don't rescue. ★ validator for flag-less site folds = pre-1.0.

### 10. Images and links
- Bare name vs path: `/` or `://` ⇒ path; else name, bubble (siblings → bucket → ascend → root → error).
- `bucket = "assets"` = directory name, not path.
- `{% image [left|right|inline] ref %}`; `![]()` rewritten too.
- Width/height/thumbs free for image parts and body images (q26).
- Link sources, not URLs: `[a](carbonara.md)`, `[b](view:blog_index)`, `[c](view:tag_index/rust)`.
- Axis member: `[x](page.md?theme=ledger)`; self-pivot `[fr](.?locale=fr)`. Else canonical URL; undeclared value = load error; only declared axis names (`?utm=x` literal). → ch. 20.
- Strict links default; raw internal URL = error with correct form. `{% post_url %}` retired.

### 11. Going live
- `grackle build`; output dir contents.
- `_cache/` — content-keyed, gitignored, deletable.
- `/static/{hash}.{ext}` — `immutable` by construction.
- `urls --against <dir>`: URL set = contract; derived `/static/{hash}` exempt. Body `diff` = spot-check.

*Exit: blog + pages + tags + archives + feed + sitemap, ~60 lines config.*

---

## Part II — Make it yours (presentation)

Reader: Part I working. One idea (ch. 12), never contradicted.

### 12. One kind, many faces, and the reason there is no `{% if %}`
- One kind: `row`. Faces = fragment variants (depth in ch. 14):

  | face | fragment | is |
  |---|---|---|
  | *(default)* | `row.html` | full page |
  | `card` | `row--card.html` | listing preview |
  | `link` | `row--link.html` | bare link |
  | `figure` | `row--figure.html` | image preview |
  | `gallery`/`cards`/… | `row--{face}.html` | view-declared |

- Three words: **Layout** = face fragment into parent's `content`. **Slot** = rung (`document` vs `root`). **Shell** = whether HTML chain runs (`raw`/`html`/`light_html`, ch. 19).
- Listing = HTML concatenation of member faces; furniture on wrapper `row`. Emphasis = CSS `:first-child`. `{% view name | face %}` per embed.
- Presence-driven; `layout:` FM gone. Schema = union of parts; hole algebra deletes absent.
- Want `if` → missing fact. Want `for` → missing view. Load checker is tripwire.

### 13. Themes you don't write
- Most readers stop here. Base theme in binary; no `themes/` ⇒ real stylesheet (`examples/minimal`).
- Gallery — `cp -r` to install:

  | theme | files | shape |
  |---|---|---|
  | `vanilla` | 1 | user-agent stylesheet |
  | `ledger` | 5 | warm column, serif, dark |
  | `marginalia` | 5 | text + margin, Tufte-ish |
  | `terminal` | 5 | monospace, dark-first |
  | `atlas` | 8 | sticky section tree, cards |
  | `miroir` | 8 | fixed rail, card feed |

  ```bash
  cp -r grackle/themes/terminal themes/terminal
  ```

- Choose: `[site] theme = "terminal"`. Loads every `themes/*` (skip `_`); `theme.scss` → `/css/<name>.css`; dir `default` → `/css/main.css`.
- **Theme cascade (authoritative):** per row (§5a).
  - Row side: FM → rule `defaults` → `[site] theme` → dir named `default` → base.
  - View side (top rung): route `theme` → member unanimity (listings) / claimed row (landings) → `[site] theme`. Declaration beats unanimity. Two URLs / two looks ⇒ axis (ch. 20).
- Body renders through row's theme, not just shell.
- error: misspelled theme lists available themes.
- Subthemes: `theme: "ledger:dark"` ⇒ `data-subtheme="dark"`; compose `marginalia:dark:wide`. Works on `[site] theme`. Row's own theme carries its own tokens.
- Recolour: root `.style.scss` `:root { --accent: … }` (overlay layer). Token names = contract (ch. 15). Per-subtree `.style.scss` → ch. 28.
- Compare: `examples/theme-preview/` — same rows under each gallery theme.

### 14. Writing a theme: faces and the hole algebra
- Theme = differences over base; same-name fragment replaces; declined kinds keep base.
- Dir: `_tokens.scss`, `theme.scss`, `shell.html`, `row.html`, `row--{face}.html`. All optional. Tokens: see ch. 15.
- **Faces, in depth:** one `row` kind; one schema (union of parts); hole algebra deletes rest.
  - View picks member face via `layout` / `variant`; `{% view name | face %}` overrides. Resolve: `row--{face}` → `row` → base. Listing = concat; no `listing` fragment.
  - Missing face degrades to `row` (request, not demand). Unclaimed aggregate missing `layout` face = **build error**.
  - Worked: galleries, cards (object rows, `order_by`).
  - ★ q45/q50: missing hole silent. ★ `explain <url> --parts` lists unplaced parts.
- `@import "tokens"|"base"|"search"|"type"|"skin"` resolves from binary; local `_<name>.scss` wins.
- Base stylesheet tiers:
  1. **reset** — always on.
  2. **type ladder** (`_type.scss`) — always on; reads tokens only.
  3. **skin** (`_skin.scss`) — decoration; opt-in via `@import "skin"` when theme has `theme.scss`; auto when tokens-only / no theme.
  - Principle: structure imposed; decoration offered.
- Four rules:
  1. `data-slot="title"` — content hole.
  2. Empty part deletes element (= every `if`).
  3. Stream maps fragment over items (= every `for`).
  4. `data-slot-href="url"` — attribute hole; absent ⇒ omit attr.
- Fallback: your fragment → base → `canonical()` (base-declined kinds only). Canonical all-or-nothing per subtree.
- **Ship a shell, own the frame:** base geometry keys on `[data-frame]` stamped by *its* `shell.html`; own shell inherits none of it.
- Part vocabulary derived at load (no `parts.toml`): base + theme fragments + schemas. Theme `.schema.toml` may *add* parts, not retype.
- Stream/map: `data-fragment` required; inline body registers child fragment (file wins if both). Same for faces.
- Rename: `data-slot="main"` → `data-slot="content"`.
- Arrangement may decline a part; canonical may not. Edge: `<img>` has only attr holes — empty content rule doesn't delete; cover on text row ⇒ broken image without exemption.
- Grouped-parts tax: rule 2 deletes part element, not your wrapper → `:not(:has(*)) { display: none }`.
- errors: unknown slot; flag-as-content; stream missing `data-fragment`; missing aggregate face.
- `<head>` = config: `[html.head.meta|property|link]` text exprs over row + `site.*`; empty ⇒ no tag. CEL ternary = deliberate `if` exception. Supersedes old `theme.toml` head-fact selection.
- ★ `head.html` theme fragment (webfonts etc.).
- ★ `theme.toml` extends chains; config `extends` built (ch. 3).
- ★ New part vocabulary needs Rust (ch. 34).

### 15. CSS does the geometry
- Contract: `[data-slot=…]`, `[data-kind=…]`, `data-<fact>`. Renderer classes = API.
- Relations: `data-relation` (was `data-axis`). Axes keep `data-axis` (ch. 29).
- Unarranged markup emits in derived schema order (ch. 14).
- `@layer reset, base, theme, overlay, post`. Overlay = root `.style.scss` (ch. 13). ★ `post` (per-post `<style>`) unbuilt. Theme beats base regardless of specificity.
- **Token contract (authoritative):** edit `_tokens.scss`; `theme.scss` = geometry, no literals. Families: palette, type, space, geometry, links/motion, components. Base binds to system colours + `ui-*`. Paste a block across themes → works. `--rule` = border shorthand.
- Breakpoints = Sass vars (foot of `_tokens.scss`); media conditions resolve before custom props.
- Baseline: nesting, `:has()`, container queries, `@layer`, subgrid, `aspect-ratio`.
- Example: footnotes → sidenotes ~4 lines grid. ★ needs notes stream (§6d B).
- `a:not([href])` = placeholder link conditional (current / nowhere).
- Style engine facts: `aria-current`, `data-relation`, `data-truncated`, `data-tree`.
- Trap (`marginalia`): flat fragment + CSS Grid = one row per child; floats for "beside", grid for "table".
- Dark mode = theme/subtheme vs `prefers-color-scheme`, not engine.
- Syntax highlight: engine emits `.k` `.s` `.c1` `.n`; theme styles or not. Languages fixed set; else plain escaped.

### 16. — *retired (faces → ch. 14)*
Deliberate gap. Close in a later renumber.

### 17. Where the site's own words live: `.slots/`
- Theme must not hold nav/copyright.
- `.slots/nav.md`; filename = slot; nearest wins; applies below.
- `.md` renders; `.html` verbatim (built behaviour).
- Block-arity: phrasing fill = exactly one block. Show error.
- Fills render per page via link resolver; one `nav.md` all locales.
- Dropdown recipe: `<details data-chrome="dropdown">` + blank line + md links + `</details>`. Localize `nav.fr.md`.
- `.slots/chrome.html` — html fragment shadowing chrome cluster (reorder/drop/insert widgets). Positional; no locale suffix; wrong spelling = load error.
- Fills = words/links, never queries. `{% view %}` does not expand in fills (→ content, ch. 6).

### 18. Landings: a route owns the URL, a row may own the words
- Landing = `[routes.*]`. Tiers: bare `title` → `intro` → `content = "path.md"` (mode B). `intro` XOR `content`.
- Mode B: claimed row must place `{% view <owner> %}` or load error.
- Offer vs promise: explicit `content` = must-place; `default_content` (base `/` → `index.{md,html}`) declineable — plain `index.html` without embed owns `/`.
- Per-key intros: `[records.<field>.<id>]` (`name`, `slug`, `intro`).
- Dogfood: `/reference/` mode B.

**Breadcrumb sources (authoritative):**
1. URL nesting / mode-B landings — crumbs climbed, not declared (`trail` only for group-key chains, q46).
2. Tree ancestors — path axis / `.section` (ch. 23).
3. Grouped-route keys — nest from subdivision (ch. 8).

### 19. Shells: how much wrapper the output wears
- Two scopes, same word:
  - **Row shells** — `shell:` on row (FM or rule `defaults`): wrapper for one page.
  - **Fold shells** — `atom`/`sitemap`/`search` + `[shells.*]` on route: how whole route serializes.
  - Disjoint domains/passes.

  | tier | selected by | head | body |
  |---|---|---|---|
  | `object` | asset rule | — | bytes |
  | `raw` | `shell: raw` | — | parts verbatim |
  | `light_html` | `shell: light_html` | ~85 B | canonical, no theme |
  | `html` | `shell: html` / default | full | theme |

- Declared: rule `defaults` (base: posts/pages `html`, catch-all `raw`); FM wins. Closed vocab.
- Render gate: `front_mattered || shell ∈ {html, light_html}`. FM+`raw` = row then verbatim. Identity-less + `html` = warning ("degenerate row").
- `light_html` = tier (bypass theme), not null theme. Null theme (ch. 13) = no fragments, full head.
- `raw` = row in DB + byte-exact artifact (imported HTML with FM).
- FM `layout:` gone; `layout: default` = chrome only (`slot: root`).
- Pair imported artifacts with `hidden: true` (sitemap/search out; linkable).
- ★ `raw` ≠ transplant through theme (q50).
- Folds may omit `from`; listing may not. Set may not wear shell/route.
- Script shells: `[shells.llms] command = "…"`; route `shell = "llms"` **must** `from`. JSON stdin (`grackle-shell/0`); stdout → route; non-zero fails build.
- Gotcha: script source in tree is published unless `exclude`. Command from config, not content row.
- ★ `md` shell; `/llms.txt` via script shell today.

### 20. Axes: one row, several forms
- Route cluster: ch. 18 owns URL; ch. 19 wrapper; ch. 20 = one row, several URLs/forms.
- Relation → other rows; axis → other forms of same row. Only legal multi-route break of ch. 5.
- Declares `values` + `field` only (`url`/`match` retired):

  ```toml
  [axes.theme]
  values = ["ledger", "atlas"]   # first = CANONICAL
  field  = "theme"
  ```

- Route template spends axis: `{theme}` / `{axis:theme}` in rule `route` or `[routes.*] path`. Template list drops canonical segment; shortest that spends non-canonical wins. Canonical = `rel=canonical` / folds see **one** rendering.
- Reuse axis (`theme`): one row N ways, shared content. File axis (`locale`): per-member file; missing file ⇒ no materialize. Locale **is** a file axis.
- Built: `theme`, `shell` (md twin). Unused `field` OK (CSS-only `data-axis-theme`).
- CSS: `data-axis-theme="ledger"`; `--axis-theme: "ledger"`. Alternates via `[html.head.link] alternate = { from = "axis.<name>", … }`.
- Switcher: `data-slot="axes"`; listing views too. Self-pivot: `[fr](.?locale=fr)`.
- Composition: cartesian product; constraint on member-tuple; each axis its segment.
- ★ `light_html` head: no canonical/alternates.

*Exit: own theme, cards, nav, landing; optional same content under two looks.*

---

## Part III — Sites that get big

Reader: 100+ files, several kinds, multi-author. Database framing OK here.

### 21. The tree declares where, config declares what
- Markers: `[markers] ".draft" = { draft = true }` then `touch`.
- Inherit `.draft`/`.hidden`/`.noindex` from base. New meanings only via `[markers]`; shadow by name (ch. 4).
- Markers replace `drafts/**` rules. Rules keep: routes, cross-tree patterns (`**/*.scss`).
- Hide subtree from search: one `touch`.

### 22. Typed fields per subtree: `.schema.toml`
- `github_link = { type = "url" }`; types: `string int bool list url image date records`.
- Three scopes: positional `.schema.toml` > `[collections.<name>.schema]` > site `[schema]` (ch. 24). Same law.
- Buys: FM validation, filter types, slot check, field → part on `row` (renders). Theme `.schema.toml` adds parts only.
- Governed = strict (unknown key = load error); ungoverned = tolerant.
- Example: `recipes/` `course`, `time`; group by.
- `records` field type: typed table in FM → multi-column stream. ★ JSON-LD open.
- ≠ `[records.<field>.<id>]` enum-records (ch. 31).

### 23. Hierarchy: the page's tree and the tree's tree
- Two axes; one recursive part (`outline_entry`); one fragment.
- Heading: `toc: true` (+ marker). Depth: `fields.toc = 'outline(content, 3)'`. From rendered bytes. ★ depth fixed h2–h3.
- Path: bare `.section`; subtree with `current`; `aria-current` shared.
- Order: `order:` FM, else lexical — declare `order:`.
- Index-less dirs = unlinked labels.
- Rendered rows only; HTML passthrough gets nothing.
- Crumbs from tree ancestors — see ch. 18.
- Dogfood: manual nav.

### 24. Drafts, hidden, and profiles
- Flags: `hidden` = routed, unlisted; `draft` = `/drafts/{slug}/`; `noindex` → head.
- Flags = ordinary `bool`s in base `[schema]` + markers. `extends = "none"` removes them; then `where = "!draft"` = load error. Any row type.
- `_drafts/` = second source for posts: `file = ["{slug}"]`, `defaults = { draft = true, shell = "html" }`. Filtered by `!draft` in queries.
- `published` must `from = ["posts", "drafts"]` or drafts leave listings silently (invisible until `--profile drafts`).
- If declaration seems ignored → `grackle explain`. `hidden` → routes; `noindex` → `[html.head.meta]`.
- Profiles = projection: patch `where`, output address, `data-profile`. `build` → default; `serve` → `dev`.

  ```toml
  [profiles.drafts.force]
  noindex = true
  [profiles.drafts.sets.published]
  where = "!hidden"
  [profiles.drafts.routes.search]
  where = 'kind == "post" && !hidden'
  ```

- Selection = query's job; relax `published` → all consumers follow. Closed keys: `url`, `noindex`, `sets`, `routes`.
- Sitemap leak survives profiles: fold must filter itself (ch. 9).
- Settled: q10 (drafts force `noindex`). Open: listing `noindex` name-match (q33).

### 25. Bringing an existing site across
Needs: routes (5), shells (19), flags (24). Case: large tree, mostly passthrough HTML.

| the file is… | you do | it becomes |
|---|---|---|
| fine, not needed in queries | nothing | verbatim bytes, not a row |
| fine, but titled/searchable/linkable | FM or sidecar + `shell: raw` | row that emits itself |
| engine chrome, not your theme | `shell: light_html` | canonical, minimal head |
| real content | FM + markdown | ordinary page |

- Most files stay top two tiers.
- `shell: raw` = DB row + byte-exact output.
- URL parity first: `grackle urls --against <old-build>` (hard gate). Works on any built tree (incl. rsynced live). ★ redirects (q28) unsolved.
- Frozen legacy: eager object rule `match = "{code,demos,writing}/**"`, `route = "/{path}"`. Else `on_demand` for cited assets.
- Ugly imports: `hidden: true` or `.noindex` marker (ch. 21).
- Sidecars (q49): `X.toml` beside `X` → identity, not bytes. Read on declaration walk; lone `.toml` = ordinary content.
  - ★ image sidecar + `shell: html` (description page) refused pending outputs model.
  - Principle: reads what a file says; does not guess from omissions. ★ HTML `<title>` derive not yet.
- ★ Transplant (q50): extraction + chrome, unfused. Today: `raw` or rewrite.
- Order: point config at tree → URL parity → FM where queryable → shells → flags → markdown at leisure.

### 26. Widgets, and the line at control flow
- `[widgets] callout = "<callout><div>\n\n{body}\n\n</div></callout>"`.
- `{% callout %}…{% endcallout %}`; body = markdown.
- No args, no conditionals. Args ⇒ you want a template engine.
- Skin ships default `callout` rule (decoration, ch. 14).
- errors: no `{body}`; missing end tag. Unregistered paired tags verbatim.
- Dogfood: `★` / `note` boxes.

### 27. Blocks and rewrites
- Body = block sequence. Address: position (summaries, built), selector (rewrites), identity (notes).
- Body images: `width`/`height` (q26).
- Rewrite stage: resolves `a[href]` in HTML-source rows only. ★ authored `.rewrite.toml` still specced — don't teach as usable.
- ★ Notes stream / sidenotes (ch. 15). Dead footnote anchors in summaries possible today.
- Pipeline: tags → comrak → (narrow) rewrites → layout picks blocks → theme.

### 28. Per-post CSS
- ★ Specced: body `<style>`, SCSS, cached, hoisted, auto-scoped; `style_scope: false` opt-out.
- Decision: one row → per-post `<style>`; subtree → `.style.scss` (`[data-scope~="dir"]`); site → theme.
- Scoped SCSS cannot declare `:root` custom props — loud error.

### 29. Relations: every neighbour list is a query
- Neighbour list = per-row query. Related / Earlier / Later / Linked from = same pipeline, row-relative sort.
- Declaration:

  ```toml
  [collections.relations.related]
  from     = "published"
  where    = "!(candidate in earlier) && !(candidate in later)"
  rank     = "embedding_similarity(self, candidate)"
  min_rank = 0.4
  limit    = 4
  # also: match, label
  ```

- Pipeline: `from → where → rank (+min_rank) → limit`.
- Env: `self` + `candidate` only; bare field = load error. §5f exception to "other rows ⇒ view".
- Relation name = membership value (finished limited list).
- Functions (Rust only): `embedding_similarity`, `year_gap`, `levenshtein`. Bigger wins; distance wears `-`.
- Grammar = CEL; `!(x in y)` for `not in`.
- Defaults: `earlier`, `later`, `related`, `linked_from` — override per name.
- Derived names always: `linked_from`, `ancestors`, `children`, `siblings`, … — usable in `where` or as `from`; only declared relations emit groups.
- Defaults fix: Related ≠ Earlier/Later; Linked-from ≠ breadcrumb parent (`!(candidate in ancestors)`).
- `match` glob: scopes self + names schema (e.g. `same_course` needs recipes `.schema.toml`).
- Relation vs axis (ch. 20). Stamp `data-relation`. `translations` → axes switcher; four relations remain.
- Backlinks: citing date, newest first. `{% view %}` splice skipped by backlink scanner (still seen for on-demand).
- Locale: pool default-locale; candidates pivot to `self`'s locale; missing variant dropped; dedupe by URL.
- Render order fixed: earlier, later, related, linked_from, then site-defined by name. Eval dependency-ordered; cycle = load error.
- Default pool: `[sets.published]` or collection `!draft && !hidden`. Explicit `from` verbatim — only default adds filter.
- Ties: `(rank, date desc, url)`. ★ same-day posts neither's neighbour for earlier/later.
- Embed text = title/tags/body; retitle re-embeds. `grackle query similar <url>`.
- ★ Edges: cross-kind only shared fields; `(a + b) > c` unsupported (lift to rank); model upgrade / `reindex` (q13).

### 30. Search
- Searchable set = query: `[routes.search] from = "*" shell = "search"` + `where`.
- Engine ships `search.bin`/`js`/`wasm`; themes must not commit them.
- Theme owes: trigger button + overlay CSS.
- Zero JS default; ~288 KB first click; last token live prefix.
- Index = prose not markup (`<style>`/`<script>` skipped).
- Exclude: `hidden: true` or narrow `where`.
- ★ overlay strings not localized.

### 31. More than one language
- Locale = `locale` **file axis** (ch. 20).

  ```toml
  [axes.locale]
  values = ["en", "fr"]   # first = canonical
  field  = "locale"
  [i18n]
  axis = "locale"
  ```

- Gone: `[i18n] default`/`locales`/`selector`. Segment via `{axis:locale}` in `route`/`file`.
- Translation = row pair by logical path (`dal.md` / `dal.fr.md`). Missing file ⇒ no materialize. `locale` = ordinary schema field.
- Switcher = `axes` slot (ch. 20); listings too. `rel=alternate hreflang`; `[html.html.attribute] lang`.
- Display: `[i18n.strings]` (`LocalizedStr`, `@key`, `@@`); `[i18n.tables.months]`; `site.title` localized. Precedence: inline > strings > built-in.
- Enum records: `[records.<field>.<id>]` `name`/`slug`/`intro` (≠ `records` field type, ch. 22).
- `partition` default-on (per-member views); opt out `partition = "default"`. Objects can't declare (load error).
- ★ Embedded views ignore page locale; listing surface default-locale; prefix selector unexercised.

### 32. The inspector: the database explaining itself
- Introduced ch. 2; richest after Parts I–III names land.
- `serve` reserves `/__debug/` from binary. Serve-only. Closed namespace → 404.

  | lens | shows | answers |
  |---|---|---|
  | tree | source ‖ URL | where did this file land? |
  | rows | table per origin | what does the db think? |
  | views | set/route fan-out | what does this query select? |
  | diagnose | anomalies first | why isn't this showing? |

- Two trees = route template made visible (ch. 5).
- Provenance: source → route → queries. Claimed row has no route; translated has two; `from = "*"` has members, no row.
- Diagnose bar: finding must be able to be wrong (undated draft ≠ finding; undated publishable post = finding).
- Star routes: inspector re-evals filter (`/search.bin`, `/sitemap.xml` counts).
- Under profile: "included in `drafts`, excluded from `default`" (ch. 24).
- ★ Assets in binary (rebuild to hack); route order lexical (client owns display).

*Exit: multi-section, multi-collection, searchable, typed.*

---

## Part IV — Reference

Terse; generate where possible; link back to teaching chapter.

### 33. Reference
- **33a. `grackle.toml`** — every key. Lead with `extends` (default base; `"none"` opts out); mark base-supplied keys. Then `[site]`/`theme`, `[schema]` (flags), `[html.head.*]`, `[[collections]]`+rules, `[markers]`, `[sets.*]`/`[routes.*]` (+ `default_content`), `[sets.*.fields.*]`, `[profiles.*]`, `[widgets]`, `[records.*]`, `[collections.relations.*]`, `[collections.<name>.schema]`, `[axes.*]`, `[media_types]`, `[i18n]`, `[shells.*]`, `[cache]`, `[static]`. Gone: `[related]`, `adjacency`, `kind`, `[i18n] default/locales/selector`, required `[links]` (strict default; `policy = "loose"`). Extractor = rule `file`. Mark built/specced.
- **33b. Expression language** — grammar, ops, truthiness, functions (`truncate_*`/`outline`/`filter_blocks`/`keep_blocks`/`links`/`images`/`word_count`/`to_json`/`glob`; relation-only `embedding_similarity`/`levenshtein`), `Content` type, maps, indexing, field vocabs. `match` = separate glob.
- **33c. Row parts** — derived vocab; faces; shell parts; schema emit order. Generate from source (open Q8).
- **33d. Front matter** — one row type: `date`, `tags`, flags, `order`, `theme`, `shell` (`raw`/`html`/`light_html`), `permalink`, typed fields. `layout:` gone. Sidecars: `X.toml` beside `X`.
- **33e. Markdown tags** — `{% image %} {% view %} {% include %}` + widgets. `{% post_url %}` retired. Unknown verbatim.
- **33f. CLI** — build/serve/query (`query stats`)/explain/urls/diff; `--profile` global. Today `query explain`. ★ `config --effective`, `explain --parts`.
- **33g. Error catalogue** — every load error, meaning, fix; sorted by message.
- **33h. Glossary** — row, collection, origin, set, route, landing, claim, part, slot, fragment, kind, variant, row shell vs route shell, relation vs axis, profile, projection, marker, scope chain, computed field.

---

## Part V — Understanding grackle

Optional. Predict behaviour.

### 34. What grackle is not
- Non-goals: comments, memberships/paywalls, ratings, live/external data, stateful interactive as modeled content, template control flow, AST access, vector indexes.
- Workarounds: edge/CDN entitlements; ETL that commits; raw + per-row assets.
- Need these → use something else.

### 35. Why it's shaped this way
- Four layers, different change rates.
- Precedence law once more — all six appearances.
- Load-time errors over 404s.
- Weakness: new part vocabulary needs Rust (fragments/CSS/head don't).
- Inherit-then-override everywhere: base config + base theme = same move; merges = three rules (by-source / by-name / per-key).
- → `DESIGN.md`.

### 36. What isn't real yet
Ledger (from `TODO-1.0.md`):
- `.slots/` typed fills; authored `.rewrite.toml`; notes/footnote stream + sidenotes; per-post `<style>`; `md` shell; board kind; serve v2 (fanout built, still full rebuild); pagination × subdivision (q30); per-block facts; audio/video types; faceted filter; transclusion; profile `baseurl`; JSON-LD from `records` (q40); `light_html` into one chain; collapse `variant`/`layout` to one face key.
- Theme distribution ★: `theme.toml` extends, `head.html`, `grackle theme` subcommands + lockfile, `?theme=` override, nested `@layer`, child-on-ancestor invalidation. Today: `cp -r` + `[site] theme`.
- Pre-1.0 tooling ★: `explain` alias, `config --effective`, `explain --parts`.
- Each row: look, blocker, q number.
- Still open for readers: q48 (`type:` as row data); q50 transplant + forgotten-hole warning; q28 redirects; q30 paginate × subdivision.
- Keep current or delete. Stale worse than absent.

---

## Open questions about the manual itself

0. Resync against `master` done (row+faces, CEL, shells, routes/sidecars, locale-as-axis). Ch. 16 thin by design (gap).
1. Chapter count: 16 gap to close later. Consider merges: 3+5+6; 27+28; 34+35. Hold Part I merge until prose.
2. ~~Part I need a theme?~~ No — base in binary. Residual: ch. 3 screenshot base vs immediate gallery `cp -r`? Lean screenshot-base.
3. ★-heavy ch. 26/27 premature for v1? Hold out; ch. 36 carries until §6d B.
4. Release notes as posts ⇒ publishing cadence. OK?
5. Deploy: `grackle/manual/` → `grack.com/grackle/`? Own repo later?
6. Ch. 25 (migration) half ★ — ship loud ★s, or hold for `<title>` derive / q50 / q28?
7. Ch. 25 worked migration? `field-notes` has `demos/pane.html` (`raw`); add `light_html` row for spectrum.
8. Reference must be generated: `grackle docs` from config structs / parts / error enums. Before hand-writing 33a–d.
9. CI grep gate for retired spellings: `[views]`, `over`, `filter`, `layout:`, `adjacency`, `[related]`, …
