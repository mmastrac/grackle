# The grackle manual — outline

Status: outline only. Language is deliberately compressed; prose comes at
release. `★` marks anything not built (must be flagged in the text, not
quietly omitted). `§` refs point at `DESIGN.md`, which stays the design
authority — the manual is the *user-facing projection* of it, never a
second source of truth.

---

## 0. Shape of the deliverable

**The manual is itself a grackle site** (`grackle/manual/`, a corpus
alongside `grack.com` and the two under `grackle/examples/`). Its own
`grackle.toml`, own theme. Rationale: the examples are engineering
instruments — `field-notes/` is the kitchen sink, `minimal/` is the
falsifier that keeps the engine honest (no byte oracle) — whereas the
manual is the *plausible site*, what a real user would actually build.
Different job, different corpus.

```
grackle/manual/
  grackle.toml
  .section                 # root of the chapter tree → nav
  .slots/nav.md
  index.md                 # landing, claims view:chapters (mode B, §5h)
  start/*.md               # Part I
  shape/*.md               # Part II
  scale/*.md               # Part III
  reference/*.md           # Part IV
  _posts/*.md              # release notes → /news/, /atom.xml
  themes/manual/           # one real theme, not the null theme
```

### What the manual dogfoods (the point of writing it here)

| feature | how the manual uses it |
|---|---|
| tree collection + pages | every chapter |
| posts + archives + feed | release notes |
| `.section` + `order:` | the chapter nav, path axis (§6e) |
| `toc:` via marker | long chapters, heading axis (§6e) |
| widgets | the `★ not built` / `note` admonition boxes (§5d) |
| row links + `view:` links (strict is default) | dense cross-referencing (§6a) |
| `[sets]` / `[routes]` split | `chapters` a set; `/reference/`, `/news/` routes |
| landings mode B | `/reference/` and `/` |
| `.schema.toml` on one row type | `reference/` pages carry `since:`, `status:` (q51: pages take typed fields) |
| CEL computed fields | `summary`/`toc`/`hero` on `/news/` (`[sets.*.fields]`) |
| `row` faces | `card` face on `/news/`; `row.html` for chapters |
| row `shell:` tiers (`raw`/`html`) | one imported/demo artifact shipped verbatim |
| base config (`extends`) | the manual's `grackle.toml` is almost empty — inherits, then overrides (§4d) |
| axes (`[axes.*]`) | a `shell`-axis md twin of every page (`/x/index.md`) is the obvious dogfood (q53) |
| sidecars | title/alt for an imported image or legacy page that can't carry front matter |
| profiles + `_drafts` | chapters-in-progress drafted in the open, shipped under a `drafts` projection (§4a) |
| search fold | manual search is the obvious real need |
| root `.style.scss` | one accent-token override, no theme edit |
| themes: row faces / slots / CSS | one theme that is *only its differences* over the base |
| the inspector | a screenshot of `/__debug/`'s source‖URL trees (§7c) |
| i18n (locale axis) | now buildable — `[axes.locale]` + a `.fr.md` chapter is the next dogfood |

Two rules carried over from the example sites: **no engine special-casing
for the manual**, and **anything the manual can't express is a finding**,
logged as a §11 question rather than worked around.

### Voice and pedagogy rules

1. **Files before concepts.** Every chapter opens with something the
   reader types or creates; the model comes after.
2. **The database framing is stated once (ch. 1) and then not leaned on**
   until Part III. Newcomers do not need "virtual on-disk database" to
   publish a blog post.
3. **One law in three shapes, introduced once, referenced forever**
   (§4d's framing): per-key merge, merge-by-source, shadow-by-name. Appears
   in routing, markers, overlays, slots, object refs, config `extends`,
   theme inheritance. Name all three early (ch. 4) — teaching only "nearest
   wins" and then hitting the reader with fragment shadowing reads as a
   lie — then just point at it.
4. **Errors are a feature; show them.** Each chapter that adds a
   mechanism shows one real load-time error message from it. This is the
   single strongest differentiator vs Jekyll and it should be visible on
   nearly every page.
5. **Honesty markers everywhere.** `★` boxes for unbuilt, and a shipped
   "What isn't real yet" chapter. No aspirational present tense.
6. **No forward references in Part I.** If a Part I chapter needs a Part
   III concept, the chapter is in the wrong place.

---

## Part I — Publish something (blog + pages)

Target reader: has used Jekyll/Hugo or nothing at all. Wants a blog and
a few pages. Should reach a running site in chapter 3 and never meet the
word "view" before chapter 6.

### 1. What grackle is
- One sentence: **the site is a database that lives in git; a theme is a
  stylesheet with opinions about where things go.**
- The pipeline named once, as a map for later: `file → row → query → doc
  model → parts → slots → CSS → URL`. Not explained yet.
- What you get: no template logic, mistakes caught at load, ~225ms
  builds.
- What it isn't: comments, memberships, live data, dynamic anything
  (§7b). Point at ch. 34.
- Explicitly: you can read only Part I and have a working blog.

### 2. Install and the commands
- `cargo build --release` (a cargo workspace under `crates/`); a site is a
  directory with a `grackle.toml`.
- `grackle build`, `grackle serve` (watch + reload), `grackle query`,
  `grackle explain <url>`.
- `grackle urls --against <dir>` — URL-set parity, the migration
  instrument (ch. 25). A *missing* URL is a link that used to resolve and
  now 404s (exits non-zero); an *extra* is usually just new content
  (reported only). Note plainly: **body-level `grackle diff` is no longer
  a gate** — the URL set is what's protected, the rendered bytes are
  checked by eye.
- Build is ~0.4s (Jekyll was ~38s; **Jekyll is fully retired**, the engine
  is grackle's alone now — say this once, it changes the framing from
  "port" to "the thing").
- `serve` caveats stated up front: full rebuild per change (~0.3s),
  poll-based reload, no SSE/TLS ★(v2).
- `grackle explain` introduced here as *the* debugging tool, so it can be
  used casually in every later chapter. (Accuracy note: the command is
  `grackle query explain` today; a top-level `grackle explain` alias is a
  pre-1.0 checkbox. Write the manual to the alias, ship the alias.)
- **`grackle config --effective`** (specced, pre-1.0) — prints the merged
  config with per-key provenance (base vs your file). This is the tool
  that makes `extends` (ch. 3) *inheritance* rather than magic; worth
  introducing beside `explain` since the base config means most of a
  site's real config is inherited, not written.

### 3. Your first site: nothing but content
- **The minimum `grackle.toml` is empty** (built 2026-07-25). A base
  config compiled into the binary supplies the three collections, the
  `published` set, `/`, `/blog/`, the feed and the sitemap; `examples/
  minimal` is the proof, 27 lines → 0. So this chapter is: make a
  directory, write `about.md` and `_posts/2026-07-19-hello.md`, run
  `serve`, get a styled blog with a feed.
- The only thing a real site writes first is `[site]` (title/url/author) —
  the base's placeholders are the one part nobody keeps.
- **Favicon**: drop a `favicon.svg` (or `.png`/`.ico`/`.webp`/`.gif`) at
  the site root and it's linked automatically; no icon ⇒ no `<link>`. An
  icon that lives elsewhere is pinned with a named object route to
  `/favicon.png` (ch. 10). Touch icons and anything needing `sizes`/`type`
  are ordinary table-form `[html.head.link]` entries
  (`{ href = '…', sizes = '"180x180"' }`). First-hour question, so answer
  it here.
- Front matter is `title:` and nothing else required; the filename gives
  `(date, slug)`.
- What just happened, in four lines: file → row → rule → route — and the
  rule came from the base you didn't write. That framing (*inherit, then
  override*) carries the rest of Part I.

### 4. Front matter, defaults, and the one precedence law
- Front matter always wins — including `permalink:`, which overrides every
  rule's route outright (the front-matter end of the routing law; note it
  here and in 33d).
- Rules supply defaults for whatever front matter omits (`defaults = {…}`).
- **The law, in the shape §4d gives it** — not one rule but *three*, one
  per kind of thing, so a reader who later meets fragment shadowing isn't
  blindsided:
  - **scalars & settings bags** (`[site]`, front matter, `defaults`) —
    merge **per key**, nearest writer wins (front matter → tree → config).
  - **collections** — merge **by source**; a site's rules *prepend* to the
    base's, so §4's first-writer-wins gives the site the route.
  - **registries** (`[sets]`, `[routes]`, `[markers]`, `[widgets]`,
    `[records]`, …) — **shadow by name**, whole entry (the same move a
    theme fragment makes over the base's, ch. 14).
- State it in a box; the three shapes recur — routing, markers, overlays,
  slots, object refs, config `extends`, theme inheritance.
- `grackle explain` shows which rule wrote which key.

### 5. Routes: deciding where files land
- **One token supplier** (built): a rule's `route` template can use
  `{path} {dir} {stem} {name} {ext}` always, `{slug}` (extractor capture
  or the stem) always, and `{year} {month:02} {day}` wherever the row has
  a date — *regardless of collection*. So a post can route to an arbitrary
  path now (`_posts/rust/hello.md` → `/rust/hello/`), not just
  `/blog/{year}/…`. That refusal is gone.
- **The date extractor is `file`, a rule key** (was collection-level
  `filename_formats`): `file = ["{date.year}-{date.month}-{date.day}-{slug}"]`,
  tried in order, first match wins, and a format needn't name every token
  (`file = ["{slug}"]` is legal for undated drafts). A collection's `file`
  is the default its rules inherit.
- Rules are ordered; first writer wins; catch-all `**` goes last.
- **A rule selects on a fact, and claims what it matches.** `front_matter
  = true` on a rule matches only files that carry an identity block (pretty
  URL); the catch-all takes the rest at their literal path. A rule's glob
  names the extensions its scope *claims* — and **a scope owns its source**:
  what a rule doesn't claim isn't content at all (not an object, not a byte
  copy), so widening a glob is how you admit a bundle. Whether a row then
  *renders* is a separate law (ch. 19): `front_mattered || shell ∈
  {html, light_html}`.
- **`[[collections]]` is an array of *sources*, no `kind`** (that field is
  gone): each is a `source` (a dir) or a `name` with rules; "objects" is
  just a collection whose rules match asset extensions and set `shell =
  "raw"` (+ `on_demand`). The base declares posts, drafts, objects and the
  tree already, so this chapter is mostly *reading* what you inherited and
  adding a rule that **prepends** (nearest wins).
- **Four content layers** decide what's even seen: `.gitignore`
  (artifacts), the dot/underscore skip (`_layouts`, `_sass`, unless a
  scope declares that dir as its `source`), `exclude` (tracked non-content
  — tree-collection key only), and **position** (the engine reads the
  site's own `grackle.toml` and `themes/` by *where they sit*, so no site
  excludes them). `include`/`included_dir` is the escape hatch over all.
- **A row renders at exactly one route** — its errors are the ones you'll
  hit: two rows on one URL, or one row on two, are load errors naming the
  file(s). Legal counts: 0 (claimed by a landing, ch. 18, or an
  unreferenced on-demand asset), 1 (everything else), **N only along an
  axis** (ch. 20). Other errors: dated route on an undated row; dead-rule
  warning. `grackle urls` lists the result.

### 6. Your first query: a set, then a route
- The pitch: you never write a loop, you name a set — and **`published`,
  `/`, `/blog/` already exist** (base config). This chapter teaches the
  model by reading them, then overriding one.
- **The one sentence: a *set* is a query; a *route* is a query that
  lands.** Two config tables; `path`/`paths` present ⇒ route (and it may
  carry the landing keys — `title crumb shell paginate group_by template
  content intro featured`); absent ⇒ set. Keys on both: `from where match
  order_by limit layout variant`.

  ```toml
  [sets.published]        # inherited; shown to explain, not to type
  from = ["posts", "drafts"]   # a union: two sources, one corpus, said aloud
  where = "!draft && !hidden"
  order_by = "-date"
  [routes.blog_index]
  from = "published"
  paginate = 5
  paths = ["/blog/", "/blog/page/{n}/"]
  ```

- **`from` is the one composition keyword** (replaced `over`): names a
  collection, `*`, a set, or a route — and what it names decides the
  meaning (over a grouped route = subdivision, ch. 8). **`where`** is the
  filter (replaced `filter`); bare field = truthiness. `match` is a
  *separate* source-path glob, so the filter stays typed-fields-only.
- **`from` scopes to *exactly* what it names, and unions are spelled out**
  (built 2026-07-25). Naming a collection no longer quietly means "every
  collection of that kind" — so "several sources, one corpus" (ch. 5) is
  said in the config with a **list**: `from = ["posts", "drafts"]`. A
  union may name only *collections*, and they must *share a kind*
  (unioning sets, or across kinds, is a load error pointing at `where`
  instead). This is a real behaviour change — a `published` that named one
  of two posts collections silently dropped the other's rows.
- Why `published` is a named set: one definition of "a post list," reused
  by feed/tags/archives/home. The real story sells it — five hand-written
  Jekyll guards had drifted into three answers and the feed shipped
  drafts; the base now names it once so a site can't re-fork it.
- Error, shown: `unknown field 'drafts' (did you mean 'draft'?)`.

### 7. Listings that don't ship the whole blog
- The problem: full bodies hidden by CSS.
- **A computed field is a CEL expression** (the `truncate = {…}` struct
  stopgap is gone): `[sets.published.fields] summary =
  'truncate_chars(truncate_blocks(content, 4), 700)'`. A TOML *string* is
  an expression; any other TOML type is a literal. Same expression surface
  as `where`/`rank`/head (§5f) — this is the reader's first real sighting.
- The `fields` table also carries the manual's other stock derivations:
  `toc = 'outline(content, 3)'` (feeds a `toc: true` row) and
  `hero = 'cover ? cover : image ? image : images(content)[0]'`.
- Computed fields inherit down `from` chains; nearest declaration wins.
- The `summary` field feeds the **card face** (ch. 14) by convention; no
  summary field ⇒ full bodies (intended, not a bug). Measured: `/blog/`
  160 KB → 15.7 KB. `truncate_*` stamps the `truncated` fact →
  `data-truncated` (first "a fact becomes an attribute").
- **The `Content` type** (what `content` is): three kinds — `html` /
  `markdown` / `text` — with coercion (`as_html`) and `word_count`;
  `truncate_*`/`outline` are HTML-only. Block helpers: `filter_blocks`
  (list of matching blocks, for indexing — `filter_blocks(content, "p")[0]`
  = lede), `keep_blocks` (Content with only those tags), both multi-tag;
  plus `links`/`images` extractors and list indexing. Keep this a
  reference-flavored list; the point is *the surface exists*, not memorize.

### 8. Tags and archives for free
- `[routes.tag_index]` with `group_by = "tags"` + `path =
  "/blog/tags/{key}/"`.
- Any typed field groups; list fields multi-key.
- `title` / `crumb` are templates over group params (`{key}`, `{year}`,
  `@months[{month}]`, `{key}`, …).
- **Subdivision**: a grouped route whose `from` names another grouped
  route refines the partition — `yearly_archive` → `monthly_archive`, keys
  accumulate down the chain. No new keyword; the engine sees `from` points
  at a grouped route and subdivides.
- Breadcrumbs fall out of the nesting; nothing declares them (§5h/q46).
- **Pages can be grouped too now** (q51 — one row type): a `date` on a
  tree page means `group_by = "date.year"` works over it. Mention lightly;
  the payoff is ch. 22/24.
- **Grouping and pagination work over *every* base now** (built 2026-07-25
  — one materializer). Objects included: `group_by = "ext"` puts all the
  jpegs at one route and the pngs at another, because `ext` was always a
  column of the object vocabulary. Galleries stop being a special case.
- **A grouped view can paginate too** — pagination happens *inside each
  partition* (the partition says which rows, `paginate` says how many per
  page). This was silently ignored before. It is **not** q30: pagination ×
  *subdivision* (a pageable parent whose children subdivide the same URL
  space) is still refused, so a grouped-and-paginated view stays a leaf.
- New error worth showing: a paginated view given a single `path` (not
  `paths`) — used to collide page 2 onto page 1, or emit nothing.

### 9. Feeds, sitemap, and fold shells
- **You already have `/atom.xml` and `/sitemap.xml`** — both inherited
  from the base (which ships exactly the URLs whose *absence* would be a
  bug anywhere; tag pages and search aren't among them, plenty of sites
  want neither). So this chapter *explains* them, and introduces `shell`
  in one sentence — *how a route is serialized* (`atom`/`sitemap`), full
  treatment in ch. 19.
- **Fold shells read every output** — a fold (`atom`/`sitemap`/`search`)
  may omit `from`; it folds over the whole materialized site. (`from =
  "*"` is retired, a hard cutoff.) A *listing* route with no `from`, by
  contrast, is a load error naming the fold shells.
- **Inherited-empty is silent; declared-empty is not.** A base route with
  no members (no `_posts/` ⇒ no feed) simply doesn't materialize, so a
  blogless site pays nothing for the inherited feed. But a route *you*
  declare materializes even when empty — the difference answers a real
  "why did/didn't this page appear."
- **The footgun**: a fold sees *every output*, and a routed row is routed
  whatever its flags say — so the sitemap must filter for itself
  (`where = '!draft && !hidden && …'`, which the base's does). Profiles
  (ch. 24) don't rescue this. ★ A validator to refuse a site fold whose
  `where` omits the flags is a pre-1.0 checkbox.

### 10. Images and links
- Bare name vs path: contains `/` or `://` → path; otherwise → name,
  bubbled up from the referencing file (siblings → bucket → ascend →
  root → error).
- `bucket = "assets"` is a directory *name*, not a path.
- `{% image [left|right|inline] ref %}`; plain markdown `![]()` is
  rewritten too.
- Width/height/thumbnails come free where images are parts. ★ post
  *bodies* still ship without dimensions (q26).
- **Link to sources, not URLs**: `[a](carbonara.md)`,
  `[b](view:blog_index)`, `[c](view:tag_index/rust)`.
- **Naming an axis member**: `[x](page.md?theme=ledger)` (and
  `view:notes_index?theme=ledger`), plus the **self-pivot** `[fr](.?locale=fr)`
  — path `.` is this page, the selector pivots it onto that axis (how you
  hand-write a switcher entry). A link otherwise resolves to a row's
  *canonical* URL; an undeclared value is a load error, and only declared
  axis names read this way (`?utm=x` stays literal). Forward-ref to ch. 20.
- **Strict links are the default now** (not opt-in) — a raw internal URL
  is an error answering with the correct source-or-`view:` form. Show it.
  Note the corpus "earned" this: `{% post_url %}` is retired, replaced by
  ordinary file-relative links.

### 11. Going live
- `grackle build`, output dir, what's in it.
- `_cache/` — content-keyed, gitignored, never published, always safe to
  delete.
- `/static/{hash}.{ext}` and why `immutable` is correct by construction.
- `grackle urls --against <dir>` for migrations — the URL set is the
  contract (20 years of inbound links); derived assets are exempt (the
  `/static/{hash}` scheme changes URLs on purpose). Body `diff` exists but
  is for spot-checking, not a gate.

*Exit check for Part I: a blog with pages, tags, archives, a feed, and a
sitemap, in ~60 lines of config.*

---

## Part II — Make it yours (presentation)

Target reader: has Part I working, wants it to look like something. The
whole part rests on one idea, delivered in ch. 12 and never contradicted.

### 12. One kind, many faces, and the reason there is no `{% if %}`
**Rewritten to the 2026-07-28 theme model (THEME.md).** The old
document/listing/summary/link *kinds* collapsed into one — this is the new
conceptual headline of Part II, so it leads.
- **One kind: `row`.** Every rendered thing is a `row` part map; there are
  no separate `document`/`summary`/`link`/`listing` kinds anymore. A row
  wears a **face** — a fragment variant — for the job at hand:

  | face | fragment | is |
  |---|---|---|
  | *(default)* | `row.html` | the full page (ex-`document`) |
  | `card` | `row--card.html` | a listing preview (ex-`summary`) |
  | `link` | `row--link.html` | a bare link in a list |
  | `figure` | `row--figure.html` | an image preview |
  | `gallery`/`cards`/… | `row--{face}.html` | any view-declared face |

- **The three orthogonal words** (state once, the spine of the part):
  **Layout** = the fragment that turns a part map into the HTML filling the
  parent's `content` hole (i.e. the face). **Slot** = which rung receives
  it (`document` furniture stack vs `root` chrome). **Shell** = whether the
  HTML chain runs at all (`raw`/`html`/`light_html`, ch. 19).
- **A listing is HTML concatenation, not a kind.** Members each render
  through a face; the aggregate `content` is those HTML strings
  concatenated, with title/crumbs/intro/pagination set on a **wrapper
  `row`**. First-member emphasis (book-of-the-month) is CSS `:first-child`,
  not a flag. `{% view name | face %}` picks the member face per embed.
- **The kind is presence-driven, never declared.** Absent front-matter
  `layout:` (gone) just means a full row; the schema is the *union* of all
  part fields, and the hole algebra deletes the ones a given row lacks.
- **The rule**: want an `if` → you're missing a fact. Want a `for` →
  you're missing a view. Both are design bugs; the load checker is the
  tripwire. (Evidence: of ~60 Liquid constructs on grack.com, 3 were
  genuine display iteration.)

### 13. Themes you don't write
**New chapter (base theme landed 2026-07-24).** Comes before "writing a
theme" because most readers should stop here. Answers the outline's old
open question "does Part I need a theme?" — no: there is now a real one in
the binary.

- **The base theme ships in the engine.** A site with **no `themes/`
  directory at all** renders as semantic HTML with a real stylesheet —
  `examples/minimal` is the proof. State plainly what changed: the null
  theme used to be *complete but unusable* (a `url` part rendered as
  `<a href="/x/">/x/</a>` — a link whose text is its own URL); the base
  fixes that with one obvious fragment per label+link kind.
- **The gallery — six themes, `cp -r` to install:**

  | theme | files | shape |
  |---|---|---|
  | `vanilla` | 1 | the user-agent stylesheet, 2026 edition |
  | `ledger` | 5 | one warm column, serif, dark mode |
  | `marginalia` | 5 | text + margin column, Tufte-ish |
  | `terminal` | 5 | monospace, dark-first, nothing rounded |
  | `atlas` | 8 | sticky section tree, cards, gallery |
  | `miroir` | 8 | fixed sidebar rail, card feed, accent chrome |

  ```bash
  cp -r grackle/themes/terminal themes/terminal
  ```
  That's the whole install — the base comes with the binary, so a copied
  theme has no companion directory to remember.
- **Choosing one**: `[site] theme = "terminal"` (built 2026-07-25) — the
  second line of the install, after `cp -r`. Engine loads every directory
  under `themes/` (skipping `_`-prefixed), compiles each `theme.scss` to
  `/css/<name>.css`; the one named `default` keeps `/css/main.css`.
- **It is only the root of the cascade**, which is the point to teach: theme
  is **per row** (§5a), so front matter opts a row out and a rule default
  cascades another theme over a subtree (`match = "recipes/**"`,
  `defaults = { theme = "ledger" }`). Same law, fifth appearance — front
  matter → rule → `[site] theme` → a directory named `default` → the base.
- **A route can name a theme too** (built 2026-07-25): `[routes.x] theme =
  "ledger:dark"` makes the look a property of the *route*, not inferred
  from its content — the view side of the cascade gains a rung at the top
  (**view declaration → member unanimity for listings / claimed row for
  landings → `[site] theme`**). The declaration wins over unanimity
  because unanimity is only an inference. What it does *not* solve — one
  query rendered under two *URLs* and two looks — is the axis (ch. 20);
  this is the half that needs no axis.
- A misspelled `[site] theme` (or `[routes.x] theme`) is a load error
  listing the themes you have. Good place for the errors-are-a-feature
  beat, because the alternative (silently rendering the default) is what
  every other SSG does.
- Nice consequence of per-row themes, worth one line: a row's **body** is
  rendered through its own theme, not just its shell — so a `recipes/`
  subtree under `terminal` is terminal all the way down.
- **Subthemes ride after a colon**: `theme: "ledger:dark"` stamps
  `data-subtheme="dark"` on `<html>`; CSS subselects `[data-subtheme~=…]`.
  They compose — `marginalia:dark:wide`. **And they work on `[site] theme`**,
  so site-wide dark mode is one config line and no theme file — worth showing,
  because it is the whole ladder (rung 0 reaching rung 2) in one line. A row
  naming its own theme states its own tokens; the site's do not follow it.
- **Recolour without touching a theme**: a site-owned root `.style.scss`
  setting `:root { --accent: … }` sits in a layer above theme CSS, and
  because the token names are a cross-theme contract it survives theme
  *switches*, not just updates. Cheapest real customization there is. ★
  (`.style.scss` overlays themselves are still specced — ch. 27.)
- Dogfood/tooling callout: `examples/theme-preview/` is a site of structurally
  identical content under each gallery theme, so `/ledger/blog/` and
  `/miroir/blog/` are the same rows in the same shapes — compare in two
  tabs. `grackle --config grackle/examples/theme-preview/grackle.toml serve`.

### 14. Writing a theme: faces and the hole algebra
- **A theme is only its differences.** It inherits the base; a fragment
  replaces the base's of the same name, and every kind you decline keeps
  the base arrangement. Three of the six gallery themes are four files.
- A theme is a directory of data: `_tokens.scss`, `theme.scss`,
  `shell.html`, `row.html`, and face variants `row--card.html` /
  `row--figure.html` / `row--{face}.html`. All optional.
- **Faces, in depth** (merged from the old faces chapter): a face is a
  fragment variant of the one `row` kind, and one schema — the union of
  all row parts — backs every face. `row.html` is the full page; a card
  wears title/date/summary, a figure wears the image parts, and the hole
  algebra deletes the rest. This is the cleanest illustration of "one
  kind, many faces."
  - **A view picks the member face** with `layout` (or `variant`) on the
    route; `{% view name | face %}` overrides per embed. Resolution:
    `row--{face}` → `row` → base. A **listing is the concatenation** of
    its members through that face, furniture on the wrapper `row` (ch. 12)
    — there is no `listing`/`link_list` fragment to write.
  - **A face a theme doesn't ship degrades**: a view asking for
    `row--cards` under a theme without it falls back to the default `row`
    face — *requests, not demands*, which is what lets any site render
    under any theme. (An *unclaimed* aggregate whose `layout` face is
    missing is a **build error**, though — it has to render as something.)
    `examples/theme-preview/` shows this side by side.
  - Galleries and cards are the worked examples (object rows, `order_by`).
  - ★ q45/q50 leftover: a face missing a hole drops that part silently
    (a deliberate omission is byte-identical to a forgotten one).
    Workaround: **`grackle explain <url> --parts`** (specced, pre-1.0)
    lists which parts nothing placed; until then, diff against the base
    face.
- **You can `@import "tokens" | "base" | "search" | "type" | "skin"` and
  the base's partial resolves out of the binary** — your own `_<name>.scss`
  wins if you ship one. So a theme reuses the reset, the search overlay, or
  the content skins without vendoring any of them.
- **The base stylesheet is three tiers, and the split is the teachable
  idea** (measured, not guessed):
  1. **reset** — box model, links, media, the placeholder-link
     convention. Always on.
  2. **type ladder** (`_type.scss`) — the heading scale and block rhythm.
     **Always on too.** Safe to impose because it reads *only tokens*: a
     theme retunes the whole hierarchy through `--size`/`--scale` without
     restating a rule, and under a theme with its own type sheet the
     ladder measured *inert* (the theme's reset zeroes what it sets).
  3. **skin** (`_skin.scss`) — the *decoration*: blockquote border, code
     panel, table borders, the callout wrapper. **Opt-in**, because it is
     *not* inert — on grack.com's listing the skins move a paragraph 19px
     and the page 61px, so a theme with its own opinions would fight them.
  - **How you get skins**: a theme with a real `theme.scss` gets reset +
    ladder free and pulls the decoration in with `@import "skin";` (into
    its own layer, so its overrides win) — all five styled gallery themes
    do exactly that. A tokens-only theme, or a site with no theme at all,
    gets skins automatically, because nobody else will supply them.
  - The principle to state once: **structure the base imposes, decoration
    it offers.** Same line it draws against `vanilla`, one level deeper.
- **Four rules, the whole language:**
  1. `data-slot="title"` — a content hole.
  2. An empty part **deletes its element** (this is every `{% if %}`).
  3. A stream **maps a fragment over items** (this is every `{% for %}`).
  4. `data-slot-href="url"` — attribute hole; absent ⇒ attribute omitted.
- **The fallback chain is now three deep**: your fragment → the base's →
  `canonical()`. Canonical fires only for kinds *the base* declines, so
  most authors never see it. (It's still the completeness guarantee —
  no part's bytes can vanish.)
- Gotcha, still true and now rarer: **canonical is all-or-nothing per
  subtree** — it never consults fragments for child kinds. The base makes
  this mostly moot, which is worth saying so readers don't fear it.
- **Ship a shell, own the frame.** The base's page geometry keys on
  `[data-frame]`, an attribute *its own* `shell.html` stamps — so writing
  your own shell inherits none of it. That's deliberate: a sticky
  full-bleed bar (`atlas`) or a fixed rail (`miroir`) would be actively
  wrong inside a centred `--measure` column. Best example in the manual of
  "opting out is a decision, not an accident."
- **The part vocabulary is *derived*, not a file** (2026-07-28): there is
  no handwritten `parts.toml` anymore. At theme load the engine derives the
  `row` vocabulary from the base + theme fragments plus the declared field
  schemas (`[schema]` / a theme's `.schema.toml`). A theme's `.schema.toml`
  may *add* fields as parts on `row` (it may not retype existing ones).
- **Stream/map slots must name their child with `data-fragment`** —
  `<nav data-slot="crumbs" data-fragment="crumb">`. And a stream/map hole
  may **embed the default body inline** instead of shipping a separate
  file: put the child markup inside the hole and it registers as fragment
  `crumb`; ship `crumb.html` and the file wins everywhere. Same rule for
  faces (`data-fragment="row--figure"` with an inline body). Lets a small
  theme stay one file.
- **`data-slot="main"` is now `data-slot="content"`** (the row's body
  hole) — flag it, since every existing shell used `main`.
- **An arrangement may decline a part; the canonical fallback may not.**
  Completeness is the *parts layer's* obligation, not the theme's —
  `terminal` drops tags from its card on purpose, a card is a jacket with
  no prose. The base's own exemptions are declared with reasons. Worth one
  worked example because it teaches rule 2's exact edge: the row's image
  part is exempt because rule 2 deletes an element with an empty **content**
  slot, and an `<img>` has only *attribute* holes — so a plain card trying
  to show a cover would emit a broken image on every text row.
- **The grouped-parts tax**: rule 2 deletes an empty part's element, not a
  wrapper *your fragment* invented. Group two parts in a meta bar and you
  pay `:not(:has(*)) { display: none }` — direct-child-scoped.
- Errors, shown: unknown slot (lists the row parts), flag-as-content slot,
  a stream/map slot missing `data-fragment`, a missing `layout` face on an
  aggregate (build error).
- **The `<head>` is config, not (mostly) a theme concern.** It's declared
  in `[html.head.meta|property|link]` (three tables = the three elements)
  as **text expressions over the row plus `site.*`** — with `+`
  concatenation (`canonical = 'site.url + url'`) and a CEL ternary
  (`robots = 'noindex ? "noindex,follow" : ""'`); an **empty result emits
  no tag** (§5e's rule 2, one level up). The ternary is worth calling out:
  it's the one place §5d's no-`if` rule is deliberately relaxed, because
  "which string does this meta take" has no fact-shaped spelling. This
  *supersedes* the old `theme.toml` head-fact selection.
- ★ But a theme *can* still need head content it can't get from config —
  a webfont link. The answer is a pending **`head.html`** theme fragment
  (specced), appended after the computed facts. Name it, so "how do I add
  a font" has an answer that isn't "you can't."
- `theme.toml` (theme-level `extends` chains) is still ★ specced;
  config-level `extends` (ch. 3) is built.
- ★ honest weakness: a *new* theme is data, but the part vocabulary is
  Rust — see ch. 34.

### 15. CSS does the geometry
- Slot names are the styling contract: `[data-slot=…]`, `[data-kind=…]`,
  `data-<fact>`. The renderer's classes are API, not implementation.
- Note the one recent rename: a relation group is keyed by
  **`data-relation`** (was `data-axis`), so per-relation CSS
  (`.relation[data-relation="related"]`) targets that. Translations, an
  *axis*, keep `data-axis`. (ch. 29 for the split.)
- **Unarranged markup emits in the *derived* schema order** — the reading
  order a screen reader or the base sees. The part vocabulary is derived
  from fragments + field schemas at load (no `parts.toml`, ch. 14), and
  that order is enforced, not the producer's incidental one.
- **The cascade order is declared in full: `@layer reset, base, theme,
  overlay, post`.** `reset`/`base`/`theme` carry content, and **`overlay`
  is now filled** by the root `.style.scss` (ch. 13); only `post`
  (per-post `<style>`) stays ★ unbuilt. The point that matters: a theme
  rule beats the base's regardless of selector specificity — no arms race,
  and it's why a four-file theme can restyle a page it didn't arrange.
- **The token contract** — the file you actually edit is `_tokens.scss`;
  `theme.scss` holds the geometry and *no literals*. Name the families
  (palette, type, space, geometry, links/motion, components) and give the
  rule that makes them work: the base binds them to CSS **system colours**
  and `ui-*` platform faces, so a theme that overrides nothing still has a
  complete, dark-mode-aware, accessible value set underneath.
  - The payoff to state explicitly: **take a block from one theme, paste
    it into another, and it works.** That's what makes tokens a contract
    rather than a convention.
  - Nice detail: `--rule` is a whole border shorthand, which is why no
    rule below a token file ever names a colour to draw a line.
- **The one thing that cannot be a token**: a media query's condition
  resolves before custom properties do, so **breakpoints are Sass
  variables** — declared at the foot of `_tokens.scss` so they're still
  edited in one place. Two gallery themes have no breakpoint at all.
- Baseline: nesting, `:has()`, container queries, `@layer`, subgrid,
  `aspect-ratio`.
- Worked example: footnotes → sidenotes in ~4 lines of grid CSS, with no
  layer above CSS consulted. ★ (needs the notes stream, §6d stage B).
- The `a:not([href])` idiom: inert crumb tail, current page, disabled
  arrow, index-less tree node. Frame it as **"a placeholder link is a
  conditional"** — the engine's way of saying "current page" or "nowhere
  to go", so no fragment ever branches on it.
- **Style engine vocabulary, don't reinvent it**: `aria-current`,
  `data-relation`, and the flags (`data-truncated`, `data-tree`) come from
  the engine. A read-more fade is a rule on a fact.
- **A flat fragment plus CSS Grid means one row per child** — the trap
  worth showing, from `marginalia`: a margin column built with
  `grid-template-columns` puts every part in its own row, so four margin
  items grow four empty rows opposite them and the prose starts *below*
  its own marginalia. Floats out of a padding inset express "beside";
  grid expresses "table."
- Dark mode as a theme concern, not an engine one — every gallery theme
  forces it with a subtheme token against `prefers-color-scheme`.
- **Syntax highlighting is a theme obligation** (highlighter built, no
  config at all — no on/off, no language registration). The engine emits
  four token classes — `.k` keyword, `.s` string, `.c1` comment, `.n`
  name — inside the usual `<pre><code>` wrappers; a theme that wants
  coloured code ships rules for those four. No rules ⇒ spans render
  uncoloured, which is exactly what two gallery themes do. Languages are a
  fixed set (rust, c, java, nasm/asm, bash/sh, yaml, json); anything else
  falls through to plain escaped text, deliberately.

### 16. — *retired (faces merged into ch. 14)*
Number left as a deliberate gap for now, not reused, so later chapter
references and the reader's mental index don't shift. The row-and-faces
material lives in ch. 14 ("Faces, in depth"). Close the gap in a later
renumbering pass.

### 17. Where the site's own words live: `.slots/`
- Problem: no theme file should contain your nav or your copyright line.
- `.slots/nav.md` beside the tree; filename = slot name; nearest wins;
  applies to everything below.
- `.md` renders; `.html` is verbatim (document the built behaviour, not
  the spec's).
- Block-arity rule: a fill in a phrasing element must be exactly one
  block. Show the error.
- Fills render per consuming page, through the link resolver — one
  `nav.md` serves every locale.
- **Your own dropdown is one recipe, not a feature** (worked example):
  a `<details data-chrome="dropdown">` in a slot fill, a blank line, then
  your markdown links, then `</details>`. The blank line drops CommonMark
  back into markdown, so the links go through the resolver (`view:` names
  are validated at load); the `data-chrome` primitive makes it render
  native under every theme. Localize with `nav.fr.md` as usual.
- **`.slots/chrome.html` is the widget row itself** (built 2026-08-05):
  one root-level html file shadowing the chrome cluster across every
  theme — reorder the engine's widgets, drop one, or put your own markup
  between them. A fragment, not a fill: html only, site root only, no
  locale suffix (the holes fill with localized parts already), and the
  wrong spelling is a load error naming the right one. This is how a
  site author mints chrome without touching a theme.
- The line to state: fills are words and links, never queries — `{% view %}`
  does not expand in a fill. A box of rows is a view embedded in content
  (ch. 6), not chrome.

### 18. Landings: a route owns the URL, a row may own the words
- A landing is a `[routes.*]` entry. Three tiers: bare (`title` only) →
  `intro = "…"` → `content = "path.md"` (mode B claim). `intro` XOR
  `content`.
- Mode B: the claimed row must place `{% view <owner> %}`, or the rows
  are unreachable — load error. (`{% view %}` still names the query; the
  keyword didn't change with the sets/routes split.)
- **Offer vs promise** (`default_content`): an *explicit* `content` is a
  promise the must-place check enforces; a *defaulted* claim (the base's
  `/` offers `default_content = "index.{md,html}"`) can be *declined* — a
  hand-built `index.html` that doesn't embed `{% view home %}` just owns
  `/` as an ordinary page. That exemption is why the base can offer a
  homepage without forcing every site to embed the feed.
- Per-key intros via `[records.<field>.<id>]` (`name`, `slug`, `intro`).
- The chain: URL nesting *is* parent derivation. Crumbs are climbed, not
  declared. `trail` remains only for group-key chains (q46).
- Dogfood callout: `/reference/` in this manual is a mode B landing.

### 19. Shells: how much wrapper the output wears
- Two scopes, same word, and the chapter must separate them in its first
  paragraph:
  - **Row shells** (the *map* family) — `shell:` on a row (front matter,
    or a rule `defaults`): how much wrapper *one page* wears.
  - **Fold shells** — `shell = "atom" | "sitemap" | "search"` plus
    `[shells.*]` script shells on a `[routes.*]`: how a whole route is
    serialized.
  - Disjoint value domains, disjoint passes — a row never meets a fold
    shell. Say so once.
- **The row tiers**, the chapter's centrepiece. **The set is `raw` /
  `light_html` / `html`** (renamed 2026-07 from the old `none`/`light`;
  `none`→`raw`, `light`→`light_html`, hard cutoffs). Show `object` too so
  the head-size jumps read:

  | tier | selected by | head | body |
  |---|---|---|---|
  | `object` | an asset rule (extension glob) | — | bytes off disk |
  | `raw` | `shell: raw` | — | rendered parts, emitted verbatim |
  | `light_html` | `shell: light_html` | minimal (~85 B) | canonical parts, no theme |
  | `html` | `shell: html` / default | full | theme fragments |

- Where it's declared: **rule `defaults`** carry the site's shells (the
  base gives posts/pages `html`, the tree/objects catch-all `raw`); front
  matter `shell:` beats them. Closed vocabulary, checked at load.
- **The render gate**, worth stating plainly: a row renders iff
  **`front_mattered || shell ∈ {html, light_html}`**. So a front-mattered
  file with `shell: raw` renders and then ships verbatim; an
  identity-less file sent through `html` renders anyway (a *warning*, the
  "degenerate row," title derived from the slug).
- **`light_html` is a *tier*, not the null theme**: it bypasses the theme
  registry for a minimal computed head — that's *why* it's `light_html`,
  not `light` (there is no `themes/light/`). The null theme (ch. 13) is a
  theme with no fragments — full head, everything but body chrome.
- **`raw` is a capability, not a spelling**, and the chapter's worked
  example: an imported artifact can carry front matter *and* still emit
  itself byte-exact. Before, front matter nested the whole `<!doctype
  html>` in a second document, so shipping it verbatim meant no front
  matter — which meant it wasn't a row at all: no title, no metadata,
  invisible to every query. `raw` makes it a row the database sees *and* a
  byte-exact artifact.
- Row front-matter `layout:` is **gone** (dissolved into shell + inferred
  face); `layout: default` survives only as the "chrome, no row furniture"
  escape (`slot: root`).
- Pair it with `hidden: true` — the honest way to keep an imported
  artifact out of the sitemap and the search index while keeping it
  linkable by source path (ties to ch. 10 and ch. 24).
- ★ What `raw` does *not* do: lift the meat out of an imported page and
  render it through the theme. That's q50, two operations (extraction,
  then chrome), deliberately not fused.
- **Fold shells** (`atom`/`sitemap`/`search`) may omit `from` — they read
  every output; a *listing* view with no `from` is a load error naming
  them. (`from = "*"` is retired, a hard cutoff.) A set may not wear a
  shell or route.
- Script shells: `[shells.llms] command = "python3 shells/llms.py"`; the
  view opts in with `shell = "llms"` and **must name a `from`** (a
  `from`-less script shell was silently fed empty rows — now a load
  error). Rows arrive as JSON on stdin (`"schema":"grackle-shell/0"`,
  provisional), stdout is written at the route, non-zero exit fails the
  build.
- **Gotcha with a real scar**: a script shell's source lives in your
  tree (`examples/field-notes/shells/llms.py`), so it's routed and
  *published* unless excluded. Add `shells/**` to `exclude`; `/llms.txt`
  still builds because the command comes from config, not a content row.
- ★ `md` shell specced; `/llms.txt` currently ships via a script shell.

### 20. Axes: one row, several forms
**New chapter (built 2026-07-25).** The third member of the route cluster:
ch. 18 a route owns a URL, ch. 19 how much wrapper a row wears, and now —
**one row published at several URLs, each a different *form* of it.**
- **The distinction (ties to relations, ch. 29)**: a *relation* points at
  *other rows*; an *axis* points at *other forms of the same row*. An axis
  is the **only** mechanism allowed to break "a row renders at exactly one
  route" (ch. 5) — everything else producing a second route on one row is
  a load error.
- **The axis declares only its values and its field** — *not* where
  members land (the `url`/`match` keys retired):

  ```toml
  [axes.theme]
  values = ["ledger", "atlas"]   # members; first is CANONICAL
  field  = "theme"               # the row field each member sets
  ```

- **The route template *spends* the axis, and the rule that spends it
  opts the rows in** — a `{theme}` (or `{axis:theme}`) segment in a
  collection rule's `route` or a `[routes.*] path`. A **list of templates**
  lets the canonical member drop its segment: `route = ["/{theme}/{axis:
  locale}/", "/{theme}/", "/"]` — engine picks the shortest that still
  spends every non-canonical axis. Canonical is a *declaration*: which
  member `rel="canonical"`/`og:url` name and which one a fold (sitemap,
  search) sees — **canonical only**, so a crawler doesn't read N
  renderings as N documents.
- **Two kinds of axis — the frame master uses**: a **reuse axis** (`field
  = "theme"`) renders one row N ways, members share the canonical row's
  content, none is ever missing; a **file axis** (`locale`) gives each
  member its own file (`index.fr.md`), and a member with no file just
  doesn't materialize. So **locale IS an axis** (a file axis), exposed
  through this interface — the reversal to state plainly.
- **Built uses**: `field = "theme"` (one corpus, several looks — what
  `theme-preview/`'s duplicate trees faked); `field = "shell"` (q44's md
  twin). A `field` the engine doesn't consume is *fine* — a CSS-only axis
  keying `data-axis-theme` is legitimate, not a footgun.
- **Members are readable from CSS, two ways**: `data-axis-theme="ledger"`
  (selecting) and `--axis-theme: "ledger"` (reading via `var()`; `attr()`
  can't reach the root). **Members emit `rel="alternate"`** — config
  `[html.head.link] alternate = { from = "axis.<name>", … }`; locale
  carries `hreflang`, a different-format form (md twin) carries `type`
  (from `[media_types]`), a same-format restyle carries neither.
- **The switcher is the `axes` shell slot** (built, closes q47):
  `data-slot="axes"` in the shell, rendered as a `<details>` dropdown by
  the base; works for **listing views too** (`/fr/blog/` → `/blog/`);
  superseded the `translations` relation. Self-pivot links pick a member
  by hand: `[fr](.?locale=fr)`, `[dark](.?theme=ledger)`.
- **Composition is built**: multiple axes over one row → the cartesian
  product, the constraint keyed on the member-*tuple* (`axis = ["palette",
  "flavor"]` → `/plain/sweet/…`); canonical is the tuple of first-declared
  members; each axis needs its own segment.
- ★ Honest edge, now nearly the only one: the `light_html` tier's minimal
  head carries no canonical and no alternates, so an alternate at that
  tier advertises nothing.

*Exit check for Part II: a theme of your own, with cards, a nav, a landing
page — and, if you want it, the same content under two looks.*

---

## Part III — Sites that get big

Target reader: 100+ files, several kinds of content, more than one
author. Here the database framing pays off and can finally be used.

### 21. The tree declares where, config declares what
- Marker files: `[markers] ".draft" = { draft = true }`, then `touch`.
- **You inherit `.draft`/`.hidden`/`.noindex` already** — the base config
  declares those three markers (and the bools behind them, ch. 24), so a
  `touch _posts/wip/.draft` works in an empty-config site. You write a
  `[markers]` entry only to name a *new* meaning; the registry shadows by
  name (ch. 4), so redeclaring one of the three overrides it.
- Same law as ch. 4 (registries shadow by name).
- What markers replace: `drafts/**` rules. What rules keep: routes, and
  patterns that cut *across* the tree (`**/*.scss`).
- Practical: hide a subtree from search with one `touch`.

### 22. Typed fields per subtree: `.schema.toml`
- `github_link = { type = "url" }`; types `string int bool list url image
  date records`.
- **Three scopes, one precedence** — teach as a unit: positional
  `.schema.toml` (a subtree), `[collections.<name>.schema]` (a whole
  collection), and site-wide `[schema]` (ch. 24 — where the base puts the
  flags). Nearest wins: positional > collection > site-wide. Same law.
- Buys **four** things: front-matter validation, filter type-checking,
  slot checking, and — a declared field **becomes a part on `row`**
  (ch. 12/14), so it *renders*. A theme's `.schema.toml` may add fields as
  parts (not retype existing ones). A typed field is presentation, not
  just a guard.
- Governed rows are strict (unknown key = load error naming the file);
  ungoverned rows stay tolerant.
- Worked example: `recipes/` with `course`, `time`; then group by it.
- **`records` — a list-of-records field** (q40 first slice, built): a
  typed table in front matter. `ingredients = { type = "records", fields
  = { amount = "string", name = "string", note = "string" } }`; the row's
  front matter carries a YAML list of maps, and the field fills a
  multi-column stream the theme renders as a table (`field-notes` dogfoods
  `ingredients` this way). ★ JSON-LD Recipe emission still open.
- **Don't confuse it with `[records.<field>.<id>]`** (ch. 31) — that's the
  *enum-records* table naming a grouped field's value domain (tag/course
  slug/name/intro). Same word, unrelated mechanism; worth one sentence so
  a reader isn't tripped.

### 23. Hierarchy: the page's tree and the tree's tree
- Two axes, one recursive part kind (`outline_entry`), one fragment.
- **Heading axis**: `toc: true` (cascade it with a marker). Depth is
  `fields.toc = 'outline(content, 3)'`. Extracted
  from rendered bytes, so link and target can't desync. ★ depth fixed
  h2–h3.
- **Path axis**: drop a bare `.section` file. Every row beneath gets the
  subtree with `current` marked; `aria-current` and styling share one
  part.
- Ordering: `order:` front matter, else lexical. Say plainly: **declare
  `order:`** — lexical is only right by zero-padding luck.
- Index-less directories render as unlinked labels.
- Constraint: rendered rows only; static HTML passthrough gets nothing.
- Dogfood callout: this is the manual's own nav.

### 24. Drafts, hidden, and profiles
- The three flags, where they come from, what each means.
- `hidden` = routed but unlisted; `draft` = routed to `/drafts/{slug}/`.
- **The flags are not engine vocabulary** (2026-07-25) — they're ordinary
  `bool` fields the *base config* declares in `[schema]`, plus the three
  markers that set them. Nice thing to state, because it's the model
  paying off: `extends = "none"` genuinely removes them, and then
  `where = "!draft"` is a load error naming the knowns, not a filter that
  silently matches everything. Also why flags work on *any* row (a page
  hides exactly as a post does) — they're row properties, not a post
  feature.
- **Drafts live in `_drafts/`** — a *second source* for the posts table,
  not a second table: `[[collections]] source = "_drafts"` with a rule
  `file = ["{slug}"]` (a draft has no date until it publishes) and
  `defaults = { draft = true, shell = "html" }`. Ordinary rows otherwise —
  routed, in the link graph, visible to the inspector — kept out of feeds
  and listings by the `!draft` filter the queries already carry.
- **The union that makes this work** (post-`from`-scoping, ch. 6): the
  `published` set must name *both* sources — `from = ["posts", "drafts"]`
  — or the drafts silently leave every listing. This is the sharp edge of
  the change: the default projection hides drafts by `where` anyway, so the
  bug is invisible until `--profile drafts` relaxes the filter to surface
  them. A URL-set check can't catch it (a draft is routed either way);
  only a full render under the drafts profile can. Good, concrete lesson in
  "parity has two profiles, and the interesting rows live in one of them."
- Cautionary example worth keeping: a page's `noindex: true` was once
  accepted and silently dropped — the exact failure this system exists to
  prevent. Teach "if a declaration seems ignored, `grackle explain` it."
  `hidden` reaches the route (star views filter it); `noindex` reaches the
  head via `[html.head.meta]` (ch. 14).
- **Profiles are built** (the outline's loudest ★ closed 2026-07-19) — a
  projection, not a different database. A profile changes three things:
  which rows the queries admit (by patching a set's or a route's `where`),
  the output address, and a `data-profile` marker. `build` uses the
  default projection; `serve` defaults to `dev`. Full config below.

  ```toml
  [profiles.drafts.force]               # forced facts for the projection
  noindex = true
  [profiles.drafts.sets.published]      # patch a set → patch a query
  where = "!hidden"
  [profiles.drafts.routes.search]       # patch a route → patch a landing
  where = 'kind == "post" && !hidden'
  ```

- **Selection stays the query's job.** Relax `published` and every
  listing, archive and feed follows, because they all read it. The key set
  is closed (`url`, `noindex`, `sets`, `routes`) on purpose — a profile
  that can override anything is a config merge, and config merges drift.
- **Keep the sitemap-leak story** — best cautionary tale in the manual,
  and it's why flags live on routes. It survives profiles: a `from = "*"`
  route sees routes, so it must filter for itself (ch. 9).
- Settled since the last draft: q10 (the drafts profile forces `noindex`
  site-wide). Still open: listing `noindex` is an engine name-match (q33).

### 25. Bringing an existing site across

The migration chapter. Placed here because it needs routes (ch. 5),
shells (ch. 19) and flags (ch. 24) and nothing later. Written against
the real case: a 27-year tree where **187 of 227 page rows are
passthrough HTML** — hand-built demos, imported artifacts, pages older
than the tags they use.

- **Frame it as a spectrum, not a conversion.** Four tiers, and picking
  one per file *is* the migration:

  | the file is… | you do | it becomes |
  |---|---|---|
  | fine as-is, and you don't need it in queries | nothing | verbatim bytes, not a row |
  | fine as-is, but should be titled/searchable/linkable | front matter (or a sidecar) + `shell: raw` | a row that emits itself |
  | worth engine chrome but not your theme | `shell: light_html` | canonical parts, minimal head |
  | real content | front matter + markdown | an ordinary page |

  Most files stay in the top two rows. Say that plainly — a migration
  that demands rewriting 187 files is a migration nobody finishes.

- **The move that unlocks the rest**: `shell: raw` means a file can be a
  database row *and* byte-exact output. Before, those were mutually
  exclusive — front matter nested the artifact inside a second `<html>`,
  and skipping front matter meant no title, no metadata, invisible to
  every query. Recover the whole tree's addressability without touching
  its bytes.
- **URL parity first, prettiness later.** Get the existing URL set
  reproduced, then change routes deliberately. `grackle urls --against
  <old-build>` is the instrument, and it's a hard gate: a missing URL
  exits non-zero. Works against any directory of built output — including
  a tree rsynced down from the live server, which is what lets it outlive
  the build that first produced it. ★ Redirects for restructured trees are
  unsolved (q28) — state it early, because it decides whether someone can
  move at all.
- **Frozen legacy subtrees publish wholesale.** For hand-written trees
  that cite assets in markup the scanner will never read (`<body
  background>`, imagemaps, framesets), an eager object rule (`match =
  "{code,demos,writing}/**"`, `route = "/{path}"`) publishes the subtree
  verbatim rather than teaching the resolver twenty years of dead HTML.
  This is the pragmatic escape hatch, and the manual should name it — the
  alternative (reference-driven `on_demand` publishing) is for assets the
  engine *can* see cited.
- **What to do about the ugly parts of an import**: `hidden: true` keeps
  an artifact linkable but out of the sitemap and the search index; a
  `.noindex` marker does a whole subtree at once (ch. 21). This is the
  honest answer for demos and legacy trees, and it's one `touch`.
- **Sidecars: metadata for a file that can't carry front matter** (q49,
  **built**). Drop `X.toml` beside `X` — `kite.png.toml` beside `kite.png`
  — a TOML front-matter block (`title`, `alt`, `date`, any declared field).
  It grants **identity, not bytes**: the row becomes `front_mattered`,
  governed, titled, linkable, in the graph — but not a document (an image
  stays its bytes). Read on the declaration walk beside markers, so an
  `exclude = ["*.toml"]` can't unspeak it; a `.toml` with no file beside it
  (`Cargo.toml`, `.schema.toml`) is ordinary content, no exception list.
  - ★ Still open: an image sidecar wearing `shell: html` (a *description
    page* — a second HTML output from an asset) is refused-with-reason,
    pending the outputs model.
  - The principle to state, because it explains otherwise-arbitrary
    behaviour: **grackle reads what a file says; it does not guess from
    what a file omits.** A raw HTML file's real `<title>` is derivation the
    engine could do (★ not yet — 39 user-facing rows are titleless today);
    inferring "fragment vs page" from a *missing* `<title>` is guessing —
    a 1996 page can be complete without `<html>`, a real demo can have no
    title — and it fails toward not rendering something.
- ★ **Transplanting** — keeping an imported page's content but rendering
  it through your theme — is q50, and doesn't exist yet. Two operations,
  deliberately unfused: *extraction* (where's the meat: `<body>`'s
  children, or a selector) and *how much chrome the result wears*
  (`shell:`, which does exist). Today: `raw`, or rewrite by hand.
- **Order of operations**, the chapter's takeaway checklist: point
  `grackle.toml` at the tree → get the URL set to parity → add front
  matter only where a row must be queryable → pick shells → flag what
  shouldn't be indexed → *then* start converting to markdown, at
  whatever pace, forever.

### 26. Widgets, and the line at control flow
- `[widgets] callout = "<callout><div>\n\n{body}\n\n</div></callout>"`.
- Usage: `{% callout %}` … `{% endcallout %}`; body is ordinary markdown
  (no `markdown="1"`).
- No arguments, no conditionals — by design. An argumentful widget is the
  tripwire that says you want a template engine, and you don't.
- **Styling the wrapper**: the base's `skin` tier ships a default
  `callout` rule, so a theme-less site (or one that `@import "skin"`s)
  gets a styled callout for free; a theme restyles the element via its own
  selector. (This is why callout lives in the *skin*, not the reset — it's
  decoration, ch. 14.)
- Errors: template with no `{body}`; missing end tag. Unregistered paired
  tags stay verbatim.
- Dogfood callout: the `★` and `note` boxes in this manual are widgets.

### 27. Blocks and rewrites
- Why the body is a block sequence, not a string. Three addressing modes:
  **position** (summaries — built), **selector** (rewrites), **identity**
  (notes).
- **Body images carry their dimensions now** (q26, built) — a bare
  `![](foo.png)` in a post ships `width`/`height`, so no layout shift.
  This was ★ in earlier drafts; move it to the "works" column.
- **The rewrite stage exists, but narrowly** (stage B, partial): today it
  does exactly one job — resolving `a[href]` links in rows whose *source*
  is HTML (raw pages, HTML slot fills, raw landings), the one thing the
  markdown AST pass structurally can't. ★ The *authored* `.rewrite.toml`
  rule table (`[[rule]] match = "table" wrap = …`) is still specced — "it
  waits for its second consumer." Don't teach it as usable.
- ★ Notes as a second stream, and the sidenote payoff (ties back to
  ch. 15) — still unbuilt. One post's summary can ship a dead footnote
  anchor today; say so.
- Pipeline order, one diagram: tags → comrak → (narrow) rewrites → layout
  picks blocks → theme.

### 28. Per-post CSS
- ★ Entirely specced: a `<style>` block in the body, SCSS, compiled,
  cached, hoisted, auto-scoped, `style_scope: false` to opt out.
- Where CSS belongs, decision table: one row → per-post `<style>`; a
  subtree → `.style.scss` ★; the whole site → theme.
- Gotcha to document now because the failure is invisible: **scoped SCSS
  cannot declare `:root` custom properties**.

### 29. Relations: every neighbour list is a query
**Rewritten — this landed built 2026-07-23 (q52/q53), and it's the moment
the expression language (§5f) became real.** Formerly "related posts +
`[related]`"; both `[related]` and the collection-level `adjacency` key are
gone. A big chapter, and a highlight of the manual: the one place a reader
writes a real expression.

- **The idea first**: a *relation* is a neighbour list, and a neighbour
  list is a per-row query. "Related", "Earlier"/"Later", "Linked from" are
  the same pipeline with a **row-relative** sort (a different order per
  post — Later/Earlier is literally pagination with a window of one).
- **The declaration**, per collection:

  ```toml
  [collections.relations.related]
  from     = "published"        # candidate pool: a set, or a derived relation
  where    = "!(candidate in earlier) && !(candidate in later)"
  rank     = "embedding_similarity(self, candidate)"   # double, bigger wins
  min_rank = 0.4
  limit    = 4
  # also: match (glob — scopes self AND names the schema), label ("@ref")
  ```

  Pipeline per row: **`from → where → rank (+min_rank) → limit`**. (`from`
  is the same reach keyword sets and routes use; the base ships the four
  defaults in `base.toml` now, not Rust.)
- **The environment is exactly two rows**: `self` (the row being rendered)
  and `candidate`. Field access must be qualified — a bare `tags` is a load
  error (ambiguous). This is §5f's one sanctioned exception to "a function
  wanting other rows means you want a view."
- **Every relation name is a value** — a set you can test membership
  against, meaning its *finished, limited list*: `!(candidate in earlier)`
  = "not already shown as Earlier." Change Earlier and this can't desync,
  because it refers to the *name*, not a restated definition. (For a raw
  threshold instead, call the function: `embedding_similarity(self,
  candidate) > 0.5`.)
- **Functions are registered in Rust, never defined in config**:
  `embedding_similarity(row,row)`, `year_gap(row,row)`,
  `levenshtein(string,string)`. That's the whole list — naming anything
  else is a load error. Worth one sentence on *why* the list is short: a
  registered-but-unwired function would type-check and then silently yield
  an empty group, so one was deliberately un-registered rather than left
  as a trap.
  **Bigger always wins** — a distance wears a minus sign (`rank =
  "-levenshtein(self.title, candidate.title)"`), same house style as
  `order_by = "-date"`. No asc/desc knob.
- **Grammar is CEL** (§5f's contract: valid CEL, never a dialect), so
  `not in` is spelled `!(x in y)`. Good place to teach the whole point of
  the contract — it keeps a real CEL crate swappable.
- **Four defaults** ship if you declare nothing: `earlier`, `later`,
  `related`, `linked_from`. Override **per name**, not wholesale.
- **Graph and path families are *derived names*** the engine always
  provides — `linked_from`, `ancestors`, `children`, `siblings`, … — usable
  two ways: in `where` (`!(candidate in ancestors)`) or as the pool
  (`from = "linked_from"`). Only a *declared* relation emits a group; a
  derived name can exist unrendered.
- **The defaults fix real bugs, so output moves** (eye-check, not byte
  diff): Related used to re-show the Earlier/Later neighbours; "Linked
  from: its own breadcrumb parent" is killed by `!(candidate in
  ancestors)`, and the scoping is *data, not an `if`* — a blog post has no
  page ancestors, so the clause does nothing there. Great illustration of
  the whole philosophy.
- **`match` is why relations carry a glob** — worked example, the tree
  collection's `same_course` (recipes in the same course): `self.course`
  only type-checks against the recipes subtree's `.schema.toml`, so the
  glob both scopes which rows get the relation *and* names the schema to
  check. Answers "why doesn't the manual show Same course?" with structure.
- **q53 terminology, now load-bearing**: a *relation* points at other
  rows; an *axis* points at other **forms of the same row** (ch. 20).
  Site-defined relations now stamp **`data-relation`** (the old
  `data-axis`, renamed in this change) — a deliberate theme-contract
  change. **`translations` is no longer a relation at all** — it became
  the `locale` group of the `axes` switcher slot (ch. 20/31), so the
  relation set is now four: `earlier`, `later`, `related`, `linked_from`.
- **Backlinks carry the citing row's date**, newest first; `linked_from`
  is the honest *mixed* example (a citing page has no date). One real fix
  rides here: the homepage's recent-posts arrangement no longer counts as
  a citation ("Linked from: Home" is gone) — the `{% view %}` splice is
  marked so the backlink scanner skips it, while on-demand publishing still
  sees it (or an arrangement-only image would silently unpublish).
- **Locales are decided, not an edge any more** (review fix): a pool is
  default-locale by construction, so each candidate **pivots through the
  logical-path index into `self`'s locale**, and a candidate with no
  variant there is *dropped*. Results dedupe by URL, since two pool
  members can pivot to the same variant. So a French note relates to
  French notes. (The first cut shipped empty relations on every translated
  page — good, short cautionary example of why "untested" edges get
  written down.)
- **Render order is fixed, not declaration order**: `earlier`, `later`,
  `related`, `linked_from`, then site-defined relations by name. Only
  *evaluation* is dependency-ordered (because `related` reads `earlier`);
  a reference cycle is a load error, never a render surprise.
- **The default pool won't leak drafts**: when `from` is omitted, the
  fallback is `[sets.published]` if you have one, else the collection
  filtered `!draft && !hidden`. State the rule crisply — **an explicit
  `from` is taken verbatim; only the default's fallback adds the filter**
  — because it's the kind of implicit behaviour that's otherwise
  surprising in both directions.
- Determinism: ties break `(rank, date desc, url)`. ★ Small known edge:
  `earlier`/`later` compare a day-granular ordinal with a strict `<`, so
  **two posts on the same day are neither's neighbour**. Zero such pairs
  in the corpus today; worth documenting rather than discovering.
- Embedding text is title/tags/body ⇒ retitling re-embeds. Nothing
  embedded is ever published. `grackle query similar <url>`.
- ★ Honest edges that remain: cross-kind pools may only compare fields
  every candidate carries; a parenthesised expression on the left of a
  comparison (`(a + b) > c`) is valid CEL but unsupported — the error says
  to lift it into a rank term; model upgrade / `reindex` undecided (q13).

### 30. Search
- The searchable set is a **query**, not a setting: `[routes.search] from
  = "*" shell = "search"` with a `where`.
- Engine assets (`search.bin`, `search.js`, `search.wasm`) ship
  automatically; **themes must not commit them**.
- A theme owes exactly two things: a trigger button and overlay CSS.
- Zero JS on the default page; ~288 KB on first click; last token is a
  live prefix.
- **The index holds prose, not markup**: raw-text elements (`<style>`,
  `<script>`) are skipped, so a styled post doesn't make `rgba` and
  `ffffff` searchable — while `margin` stays findable when a post
  actually discusses it. Relevant to anyone shipping `shell: raw` rows
  or per-post CSS.
- Keeping things out of the index: `hidden: true` on the row, or narrow
  the route's `where`. The searchable set is a query, so this is the
  same lever, not a second one.
- ★ overlay strings not localized.

### 31. More than one language
**Reworked to master's locale-as-axis model.** Locale is no longer a
bespoke mechanism — it's the `locale` **file axis** (ch. 20), which is the
frame to lead with.
- **Turn on French in one line**: add it to the axis's values.

  ```toml
  [axes.locale]
  values = ["en", "fr"]   # first is canonical
  field  = "locale"
  [i18n]
  axis = "locale"          # which axis does the pairing (default "locale")
  ```

  Gone: `[i18n] default`, `[i18n] locales`, `[i18n] selector` — the locale
  *set* is `[axes.locale] values`, canonical is `values[0]`, and *where*
  the segment lands is a `{axis:locale}` token in a rule's `route`/`file`
  (suffix `{stem}.{axis:locale}` or prefix `{axis:locale}/{stem}`), not a
  selector key.
- **A translation is a row, not a site copy** — `dal.md` and `dal.fr.md`
  are two rows paired by logical path. As a *file* axis (ch. 20) each
  member owns its file, and a member with no file just doesn't
  materialize. `locale` is an ordinary declared `[schema]` field; filters
  see it like any field.
- **The switcher is the `axes` shell slot** (ch. 20), which is what closes
  the old q47 — it works for **listing views too** (`/fr/blog/` links back
  to `/blog/`), because a view's members are read off its own routes.
  Members emit `<link rel="alternate" hreflang>` in the head (config:
  `[html.head.link] alternate = { from = "axis.locale", hreflang = 'locale',
  href = 'site.url + url' }`), and `[html.html.attribute] lang = 'locale'`.
- **Display strings and dates are `[i18n]` data**: `[i18n.strings]` (a
  `LocalizedStr` map, `@key` refs, `@@` escapes) holds search-overlay
  strings and date *templates* (`medium_date = "{day} @months[{month}]
  {year}"`); `[i18n.tables.months]` (decimal-keyed `LocalizedStr`) is the
  month names — referenced anywhere with `@months[{month}]`. `site.title`
  is a `LocalizedStr` too. Precedence for a name: inline > `[i18n.strings]`
  > engine built-in.
- **Enum records** name a grouped field's value domain:
  `[records.<field>.<id>]` with `name` (displays, `LocalizedStr`), `slug`
  (routes, locale-independent), `intro` (the value's landing prose). Id
  stays the key. (Distinct from the `records` *field type*, ch. 22.)
- **`partition`** (was `View.locales`) controls per-member materialization
  — **default-on**, so every row-query view gets its `/fr/` parallel; opt
  out with `partition = "default"`. A member with no rows materializes
  nothing. Object views can't declare it (load error).
- ★ Honest edges: embedded views don't follow their page's locale yet;
  listing-surface resolution is default-locale today; prefix selector
  unexercised by a real corpus.

### 32. The inspector: the database explaining itself
Placed last in Part III because it *displays* everything Parts I–III
taught — claimed rows, star routes, locales, flags, profiles — and is far
richer once those names mean something. Introduced back in ch. 2 so a
reader can use it from day one.
- `grackle serve` reserves `/__debug/`, answered from the binary.
  Serve-only; a build emits none of it. Closed namespace — a miss 404s.
- **Four lenses**, cardinality picks the form (deliberately no node-graph
  — 1575 routes as a hairball teaches nothing):

  | lens | shows | the question it answers |
  |---|---|---|
  | tree | source tree ‖ URL tree, side by side | where did this file land? |
  | rows | a table per origin, typed columns, flags | what does the db think this row is? |
  | views | every set/route and its fan-out | what does this query actually select? |
  | diagnose | anomalies first | **why isn't this page showing up?** |

- **The two trees are the teaching device**: source and URL are one corpus
  in two shapes, and the difference between them *is* the route template
  (ch. 5 made visible). The best screenshot in the manual.
- **The provenance strip**: source → route → the queries that picked it
  up. A generic db viewer can't show it — the row and the URL aren't the
  same object: a claimed row has no route (ch. 18), a translated row has
  two (ch. 31), a `from = "*"` route has 66 members and no row.
- **The diagnose lens earns the most space** — every finding is an
  exception (no route, claimed, draft, hidden, noindex, no title, undated
  post, route with no members). State the bar, because it's a transferable
  idea: **a finding must be able to be wrong.** An undated *draft* isn't
  one (undated is what a draft is); an undated *publishable post* is
  (silent triple cost — no archive membership, a truncated trail, last in
  every ordering). It's the difference between a linter people read and one
  they mute.
- **Star routes have members here.** `from = "*"` carries no member list
  (it ranges over routes), so the inspector re-evaluates the filter rather
  than showing empty — `/search.bin` 327, `/sitemap.xml` 589 (matching the
  emitted sitemap exactly).
- Under a profile it says "included in `drafts`, excluded from `default`"
  — the best demonstration of what a projection *is* (ch. 24).
- ★ Honest edges: assets embedded in the binary (hacking the inspector
  needs a rebuild); route order is lexical, so the client owns display
  order (`/blog/page/10/` before `/blog/page/2/` otherwise).

*Exit check for Part III: a multi-section, multi-collection, searchable
site with typed content.*

---

## Part IV — Reference

Terse, generated where possible, no teaching. Each entry links back to
the chapter that teaches it.

### 33. Reference
- **33a. `grackle.toml`** — every key. Open with **`extends`** (defaults
  to the base config; `"none"` opts out — and the whole reference should
  mark which keys the *base* already supplies, since a real site overrides
  more than it writes). Then `[site]` (incl. **`theme`**), `[schema]`
  (where the `draft`/`hidden`/`noindex` bools now live),
  **`[html.head.meta|property|link]`** (the head as text expressions —
  empty ⇒ no tag), `[[collections]]` + `[[collections.rules]]`
  (array-of-tables; name from source dir), `[markers]`, **`[sets.*]`** and
  **`[routes.*]`** (the split; route-only keys incl. **`default_content`**
  vs shared keys), `[sets.*.fields.*]`, `[profiles.*]` (+ nested
  `.sets.*`/`.routes.*`, `.force`), `[widgets]`, `[records.*]` (enum
  records), **`[collections.relations.*]`** (`from`/`where`/`rank`/
  `min_rank`/`limit`/`match`/`label`), **`[collections.<name>.schema]`**
  (the middle of the three schema scopes, ch. 22), **`[axes.*]`**
  (`values`/`field`) + route `axis =`, **`[media_types]`**, `[i18n]`
  (`axis`/`names`/`strings`/`tables`), `[shells.*]`, `[cache]`, `[static]`.
  Gone: **`[related]`**, collection **`adjacency`**, collection **`kind`**,
  **`[i18n] default`/`locales`/`selector`**, required **`[links]`** (strict
  default; `policy = "loose"` opts out). Route extractor is now the rule
  key **`file`** (was `filename_formats`). Mark built/specced per key.
- **33b. Filter/expression (`where` and `fields`) language** — grammar,
  operators, truthiness, the function registry (`truncate_blocks`/
  `truncate_chars`/`outline`/`filter_blocks`/`keep_blocks`/`links`/
  `images`/`word_count`/`to_json`/`glob`; relation-only
  `embedding_similarity`/`levenshtein`), the `Content` type
  (html/markdown/text + coercion), map literals, list indexing, and the
  field vocabularies (row / object / route). `match` is a *separate*
  source-path glob.
- **33c. Row parts** — the derived vocabulary (no `parts.toml`): the union
  of `row` part fields from base + theme fragments + declared schemas.
  State it's **engine-derived** (a theme's `.schema.toml` adds parts, can't
  retype), lists faces (`row`/`row--card`/`row--figure`/…) and the shell
  parts (`content`, `axes`, …), and that unarranged parts emit in schema
  order. Generate this from the source, not by hand (open question 8).
- **33d. Front matter** — reserved keys. One row type (q51): `date`,
  `tags`, flags, `order`, `theme`, `shell`, `permalink`, typed fields —
  on *any* row. `shell:` is **`raw`/`html`/`light_html`**; `layout:` is
  **gone** (a row no longer declares its face). Sidecars: an `X.toml`
  beside `X` carries the same front-matter keys for a file that can't.
- **33e. Tags in markdown** — `{% image %} {% view %} {% include %}`
  (parameterless) + widgets. `{% post_url %}` is **retired** — use an
  ordinary file-relative link. Unrecognised tags emit verbatim.
- **33f. CLI** — build / serve / query (incl. `query stats` — one count
  per declared bool field) / explain / **urls** / diff, all flags.
  `--profile` is global. Note the real spelling `grackle query explain`
  and the pending top-level alias. ★ pre-1.0, name them anyway:
  **`config --effective`** (merged config + provenance) and **`explain
  <url> --parts`** (part map, incl. parts nothing placed).
- **33g. Error catalogue** — every load-time error, what it means, the
  fix. Sorted by message. High value: this is the page people land on
  from a search engine.
- **33h. Glossary** — row, collection, origin, **set**, **route**,
  landing, claim, part, slot, fragment, kind, variant, **row shell vs
  route shell**, **relation vs axis**, profile, projection, marker, scope
  chain, computed field.

---

## Part V — Understanding grackle

Optional reading. Explains the shape so users can predict behaviour
rather than memorize it.

### 34. What grackle is not
- Confirmed non-goals with reasons: comments, memberships/paywalls,
  ratings, live/external data, stateful interactive widgets as *modeled*
  content, control flow in templates, AST access, vector indexes.
- The honest workaround for each (edge/CDN for entitlements; ETL that
  commits data; raw passthrough + per-row assets).
- Says clearly: if you need these, use something else. That's fine.

### 35. Why it's shaped this way
- The four layers and their different rates of change.
- The recurring law, one more time, with all six of its appearances.
- Why load-time errors instead of 404s.
- The one honest weakness left: a new *part vocabulary* needs Rust
  (fragments and CSS don't; the head is now config-declared, ch. 14).
- Worth a section of its own: **inherit-then-override, everywhere.** The
  base config and the base theme are the same move — a binary-embedded
  default a site extends and cannot forget to copy (the part vocabulary is
  now *derived* rather than a shipped `parts.toml`, but the principle is
  identical) — and config merges decompose into the *same three rules*
  (by-source / by-name / per-key) the engine already had. State it once.
- Pointer to `DESIGN.md` for anyone who wants the full argument.

### 36. What isn't real yet
- The ledger, in one table (sourced from `TODO-1.0.md`, master):
  **subtree/positional `.style.scss`** (the *root* one is built, ch. 13),
  **`.slots/` typed fills**, **authored `.rewrite.toml` rules**, **the
  notes/footnote stream + sidenotes**, **per-post `<style>`**, **the `md`
  shell**, **board kind**, **serve v2** (incremental invalidation; the
  fanout graph is built but serve still rebuilds), **pagination ×
  subdivision** (q30), per-block facts, audio/video field types, faceted
  filtering, transclusion, profile `baseurl`, JSON-LD from `records` (q40),
  **`light_html` folding into the one chain**, **collapse `variant`/
  `layout` to one face key**.
- **Theme distribution** is the big specced block: theme-level `theme.toml`
  + `extends` chains, `head.html`, per-theme head-fact selection, the
  `grackle theme` subcommand family
  (`add`/`update`/`list`/`new`/`derive`/`check`/`try`) + `themes/.lock.toml`,
  the `?theme=` dev override, nested `@layer` down the chain, child-on-
  ancestor invalidation. Today: install is `cp -r` + `[site] theme`, no
  update path.
- **Pre-1.0 tooling the manual leans on**: the top-level `grackle explain`
  alias (it's `query explain`), `config --effective` (merged-config
  provenance), `explain --parts`. Keep ★ until they land.
- Each row: what it would look like, what blocks it, the q number.
- **Landed since the site-theme drafts — moved out of the ledger** into the
  chapters named (this is the big resync): the **new theme model** (ch. 12
  — one `row` kind + faces, listing = concatenation, `parts.toml` gone,
  `main`→`content`); **CEL computed fields** (ch. 7 — `truncate_*`/
  `outline`/`hero`/`filter_blocks`, the `Content` type, replacing the
  `truncate={}` struct); **`records` field type** (ch. 22); **shell rename**
  to `raw`/`html`/`light_html` (ch. 19); **route-token one supplier** — a
  post routes anywhere now (ch. 5), retiring q51's remainder; **sidecars /
  q49** (ch. 25); **four content layers + positional** (ch. 5);
  **collections lost `kind`; `filename_formats`→`file`** (ch. 5); **`from`
  unions + one materializer** (ch. 6/8); **the base config** (ch. 3);
  **flags as declared bools** (ch. 24); **axes fully landed** (ch. 20 —
  switcher slot closes q47, composition, member `rel="alternate"`, locale
  reclassified **AS** a file axis); **`[media_types]`** (ch. 20/33);
  **`[site] theme` + root `.style.scss`** (ch. 13); **i18n as the locale
  axis** (ch. 31 — `[i18n] axis`, `[i18n.tables]`, `site.title` localized);
  **the base theme + gallery** (ch. 13, now 8 themes); **the inspector**
  (ch. 32); **profiles + `_drafts`** (ch. 24); **relations / §5f** (ch. 29).
- **Still-open arrivals a reader would notice:**
  - q48 — `type:` as row data. Held until something other than the
    renderer consumes it; today the answer is subtree position +
    `.schema.toml`.
  - q50 — transplanting an imported page (extraction + chrome), and the
    blocked forgotten-hole warning under it (ch. 14). Sidecars (q49) built.
  - q28 — redirects for restructured URL trees; q26 — body-image
    dimensions; q30 — pagination × subdivision.
- Kept current or deleted. A stale version of this page is worse than no
  page.

---

## Open questions about the manual itself

0. **This pass was a resync against `master`, not the `site-theme` branch
   the outline grew up on.** Big model changes folded in: the row+faces
   theme model (ch. 12–16), CEL fields (ch. 7/22), the shell rename
   (ch. 19), route-token unification + four content layers + sidecars
   (ch. 5/25), and i18n-as-locale-axis (ch. 31). A couple of chapters are
   now *thinner* than their neighbours (16 especially) — see merges below.
1. **Chapter count.** **16 (faces) has been merged into 14** — its number
   is now a deliberate gap (not reused), to be closed in a later
   renumbering pass. Remaining merges to consider: **3+5+6** into one
   "inherit-then-override" chapter (the empty base config makes them one
   story); **27+28** (blocks + per-post CSS, both largely ★); **34+35**
   (non-goals + rationale). Hold the Part I merge until prose.
2. ~~**Does Part I need a theme?**~~ **Answered by the engine, 2026-07-24:
   no.** The base theme ships in the binary, so ch. 3's first `serve`
   already looks like a real site with zero configuration. That was the
   single biggest structural worry in this outline and it's gone. Residual
   question: **should ch. 3 screenshot the base, or `cp -r` a gallery
   theme immediately?** Leaning screenshot-the-base — "you already have a
   site" is a stronger opening than "now install something."
3. **★-heavy chapters (26, 27) may be premature.** Option: hold them out
   of v1 and let ch. 36 carry them until §6d stage B fully lands.
4. **Release notes as posts** is the natural dogfood, but it means the
   manual has a publishing cadence. Acceptable?
5. **Where does the manual site live and deploy?** `grackle/manual/` in
   this repo, served at `grack.com/grackle/`? Own repo later?
6. **Ch. 23 (migration) is half ★ — does it ship in v1?** The built half
   stands on its own (the four tiers, `shell: raw`, flags, `grackle
   urls` parity, wholesale legacy publishing). The unbuilt half is the
   ending: q49 metadata derivation, q50 transplant, q28 redirects — the
   questions a migrator asks first. Ship with the ★s loud, or hold until
   q49's cheap derive half (reading a `<title>` already there) lands.
7. **Does ch. 25 want a worked migration?** A before/after on a small
   imported tree would carry it better than prose. `field-notes` now has
   `demos/pane.html` (`shell: raw`); a second row at the `light_html` tier
   would make the spectrum visible.
8. **Reference sections must be generated — a `grackle docs` subcommand.**
   32a (config keys), 32b (`where` functions), 32c (part kinds), 32d
   (front-matter keys) each churned under a refactor the manual didn't see
   coming; hand-written, they rot in a *week*. The fix is a `grackle docs`
   subcommand that emits them from the single sources of truth — the config
   structs, `assets/parts.toml`, and the error enums — so the reference
   can't drift from the binary. The strongest feature request to fall out
   of writing the manual, and it should exist before the reference is
   written by hand even once.
9. **Retired-spelling grep gate in CI.** The vocabulary keeps churning:
   `[views]`→`[sets]`/`[routes]`, `over`→`from`, `filter`→`where`, and now
   row `layout:`→`shell:`/inferred and `adjacency`/`[related]`→
   `[collections.relations.*]`. Every retired spelling reads as plausible,
   so a grep gate over the manual corpus (fail the build on a banned token)
   is cheap insurance the prose can't quietly regress.
