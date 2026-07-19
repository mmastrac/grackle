# The grackle manual — outline

Status: outline only. Language is deliberately compressed; prose comes at
release. `★` marks anything not built (must be flagged in the text, not
quietly omitted). `§` refs point at `DESIGN.md`, which stays the design
authority — the manual is the *user-facing projection* of it, never a
second source of truth.

---

## 0. Shape of the deliverable

**The manual is itself a grackle site** (`grackle/manual/`, third corpus
after `grack.com` and `example/`). Not a subdirectory of the example —
its own `grackle.toml`, own theme. Rationale: the example site is the
*falsifier* (weird on purpose, no byte oracle); the manual is the
*plausible site* — what a real user would actually build. Different job,
different corpus.

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
| row links + `view:` links, `policy = "strict"` | dense cross-referencing (§6a) |
| landings mode B | `/reference/` and `/` |
| `.schema.toml` | `reference/` rows carry `since:`, `status:` |
| computed `summary` | `/news/` listing |
| row `shell:` tiers | one imported/demo artifact shipped verbatim (§5g) |
| search shell | manual search is the obvious real need |
| themes: parts/slots/CSS | one honest theme, documented as it is built |
| i18n ★ | *not* enabled v1; named as the next dogfood |

Two rules carried over from the example site: **no engine special-casing
for the manual**, and **anything the manual can't express is a finding**,
logged as a §11 question rather than worked around.

### Voice and pedagogy rules

1. **Files before concepts.** Every chapter opens with something the
   reader types or creates; the model comes after.
2. **The database framing is stated once (ch. 1) and then not leaned on**
   until Part III. Newcomers do not need "virtual on-disk database" to
   publish a blog post.
3. **One law, introduced once, referenced forever**: *nearest wins, first
   writer per key; front matter → tree → config*. Appears in routing,
   markers, overlays, slots, object refs. Name it early (ch. 4), then
   just point at it.
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
  (§7b). Point at ch. 31.
- Explicitly: you can read only Part I and have a working blog.

### 2. Install and the four commands
- `cargo build --release`; a site is a directory with a `grackle.toml`.
- `grackle build`, `grackle serve` (watch + reload), `grackle query`,
  `grackle explain <url>`.
- `serve` caveats stated up front: full rebuild per change (~0.3s),
  poll-based reload, no SSE/TLS ★(v2).
- `grackle explain` introduced here as *the* debugging tool, so it can be
  used casually in every later chapter.

### 3. Your first site: one page, one post
- Minimum viable `grackle.toml`: `[site]`, one `posts` collection, one
  `tree` collection, one `objects` collection, catch-all rules.
- Write `about.md`, write `_posts/2026-07-19-hello.md`, run `serve`.
- Front matter is: `title:` and nothing else required.
- Filename gives `(date, slug)` — `filename_formats`.
- What just happened, in four lines: file → row → rule → route.

### 4. Front matter, defaults, and the one precedence law
- Front matter always wins.
- Rules supply defaults for whatever front matter omits (`defaults = {…}`).
- **The law**: nearest wins, first writer per key. Front matter → tree →
  config. State it in a box; it recurs five more times.
- `grackle explain` shows which rule wrote which key.

### 5. Routes: deciding where files land
- Route templates and tokens: `{year} {month:02} {day} {slug} {dir}
  {stem} {path}`.
- Rules are ordered; first writer wins; catch-all `**` goes last.
- Pretty URLs vs literal passthrough: `front_matter = true` as the switch
  (the Jekyll behaviour, made explicit).
- Related but distinct, so name the difference here and defer: front
  matter *presence* decides whether a file is a row at all; `shell:`
  decides how much wrapper a row wears (ch. 18). A file with no front
  matter ships verbatim and is invisible to every query.
- Errors, shown: two rows on one URL; dated route on an undated row; dead
  rule warning.
- `grackle query urls`.

### 6. Your first view: making `/blog/` exist
- The pitch: you never write a loop. You name a set.
- `[views.published]` (query only) → `[views.blog_index]` (route +
  `paginate`).
- **Three shapes**, the table: named query / embeddable / materialized —
  the difference is just whether `route` and `layout` are present.
- Why `published` is separate: one definition of "a post list", reused by
  the feed, tags, archives, home. (Tell the real story: five hand-written
  Jekyll guards had drifted into three different answers and the feed was
  shipping drafts.)
- Filters: `filter = "!draft && !hidden"`; bare field = truthiness.
- Error, shown: `unknown field 'drafts' (did you mean 'draft'?)`.

### 7. Listings that don't ship the whole blog
- The problem: full bodies hidden by CSS.
- `[views.published.fields.summary] truncate = { max_blocks = 4,
  max_chars = 700 }`.
- Computed fields inherit down `over` chains; nearest declaration wins.
- `summary` is the one preview kind, by convention. No summary field ⇒
  full bodies (intended, not a bug).
- Measured: `/blog/` 160 KB → 15.7 KB.
- `truncated` becomes `data-truncated`, which the theme styles. First
  sighting of "a fact becomes an attribute".
- ★ note: `truncate = {…}` is a stopgap shape; it becomes an expression
  (§5f, q31). Don't build config that depends on the struct form.

### 8. Tags and archives for free
- `group_by = "tags"` + `route = "/blog/tags/{key}/"`.
- Any typed field groups; list fields multi-key.
- `title` / `crumb` are templates over group params.
- **Subdivision**: `over` a grouped view refines the partition —
  `yearly_archive` → `monthly_archive`, keys accumulate.
- Breadcrumbs fall out of the nesting; nothing declares them (§5h/q46).
- Limit, stated plainly: **pagination × subdivision is refused** (q30) —
  and the config error says so.

### 9. Feeds, sitemap, and `over = "*"`
- `[views.feed]` + `shell = "atom"`; `[views.sitemap]` + `shell =
  "sitemap"`.
- Introduce `shell` in one sentence: *how the result is serialized*.
  Full treatment in ch. 18.
- `over = "*"` = every routable row; runs in a second pass.
- **The footgun, called out loudly**: any new `over = "*"` view must
  repeat `!draft && !hidden` or it leaks. Nothing enforces it ★(profiles,
  §4a).

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
- `[links] policy = "strict"` — recommend it, show the error, note the
  manual itself runs strict.

### 11. Going live
- `grackle build`, output dir, what's in it.
- `_cache/` — content-keyed, gitignored, never published, always safe to
  delete.
- `/static/{hash}.{ext}` and why `immutable` is correct by construction.
- `grackle diff --against <dir>` for migrations. ★ caveats (q21/q22).

*Exit check for Part I: a blog with pages, tags, archives, a feed, and a
sitemap, in ~60 lines of config.*

---

## Part II — Make it yours (presentation)

Target reader: has Part I working, wants it to look like something. The
whole part rests on one idea, delivered in ch. 12 and never contradicted.

### 12. Parts, slots, and the reason there is no `{% if %}`
- **Layout kind = which parts. Theme = which arrangement. Shell = which
  serialization.** Three independent axes; state once, repeat as needed.
- A layout kind emits a *part map*, not a page: `document`, `listing`,
  `feed`, `raw`, plus `gallery`, `summary`, …
- The kind is inferred from what a row is; you rarely declare it.
- ★ Do **not** teach `layout:` as the way to pick furniture. It is a
  surviving Jekyll word now scheduled to dissolve into `shell:`
  (q33(f)): `page`/`post` are one value, and the `_layouts/*.html` it
  names have been unread since §5e. Teach `shell:` (ch. 18); mention
  `layout:` only in the migration note and the reference.
- **The rule**: want an `if` → you're missing a fact. Want a `for` →
  you're missing a view. Both are design bugs. The load checker is the
  tripwire.
- Evidence, briefly: of ~60 Liquid constructs on grack.com, 3 were
  genuine display iteration.

### 13. Writing a theme: the hole algebra
- A theme is a directory of data: `shell.html`, `<kind>.html`,
  `<kind>--<variant>.html`, `theme.scss`. All optional.
- **Four rules, the whole language:**
  1. `data-slot="title"` — a content hole.
  2. An empty part **deletes its element** (this is every `{% if %}`).
  3. A stream **maps a fragment over items** (this is every `{% for %}`).
  4. `data-slot-href="url"` — attribute hole; absent ⇒ attribute omitted.
- `shell.html` is body chrome only; the engine owns doctype/head/body
  (§5g). Slots: `nav`, `site_title`, `main`, `copyright`.
- Themes are **partial**: any kind you don't arrange falls back to
  canonical markup.
- Gotcha, prominent: **canonical fallback is all-or-nothing per subtree**
  — no `document.html` means your `crumb.html` is never consulted.
- Errors, shown: unknown slot (lists the kind's parts), flag-as-content
  slot, `data-fragment` on a scalar.
- ★ `theme.toml` (per-theme head-fact selection) is specced, absent; the
  engine renders all head facts.
- ★ honest weakness: a *new* theme is data, but the shell/part vocabulary
  is Rust — see ch. 32.

### 14. CSS does the geometry
- Slot names are the styling contract: `[data-slot=…]`, `[data-kind=…]`,
  `data-<fact>`. The renderer's classes are API, not implementation.
- Baseline: nesting, `:has()`, container queries, `@layer`, subgrid,
  `aspect-ratio`.
- Worked example: footnotes → sidenotes in ~4 lines of grid CSS, with no
  layer above CSS consulted. ★ (needs the notes stream, §6d stage B).
- The `a:not([href])` idiom: inert crumb tail, current page, disabled
  arrow.
- Dark mode as a theme concern, not an engine one.

### 15. Variants: one kind, several looks
- `variant = "cards"` on a view → `summary--card.html`; resolution is
  `{kind}--{variant}` → `{kind}` → canonical.
- `data-fragment` as an explicit override on a stream.
- Galleries as the worked example (object views, `order_by` required).
- ★ known silent failure: a variant fragment missing a hole drops that
  part with no warning (q45 leftover). Now understood as blocked, not
  merely unbuilt — a *deliberate* omission is byte-identical to a
  forgotten one, so the warning can't exist until a theme can say "I
  don't place this part" (q50). Document the symptom and the workaround
  (diff against canonical), not a promise.

### 16. Where the site's own words live: `.slots/`
- Problem: no theme file should contain your nav or your copyright line.
- `.slots/nav.md` beside the tree; filename = slot name; nearest wins;
  applies to everything below.
- `.md` renders; `.html` is verbatim (document the built behaviour, not
  the spec's).
- Block-arity rule: a fill in a phrasing element must be exactly one
  block. Show the error.
- Fills render per consuming page, through the link resolver — one
  `nav.md` serves every locale.

### 17. Landings: a view owns the URL, a row may own the words
- Three tiers: bare (`title` only) → `intro = "…"` → `content =
  "path.md"` (mode B claim). `intro` XOR `content`.
- Mode B: the claimed row must place `{% view <owner> %}`, or the rows
  are unreachable — load error.
- Per-key intros via `[records.<field>.<id>]` (`name`, `slug`, `intro`).
- The chain: URL nesting *is* parent derivation. Crumbs are climbed, not
  declared. `trail` remains only for group-key chains (q46).
- Dogfood callout: `/reference/` in this manual is a mode B landing.

### 18. Shells: how much wrapper the output wears
- Two scopes, same word, and the chapter must separate them in its first
  paragraph:
  - **View shells** — `shell = "html" | "atom" | "sitemap" | "search"`
    on a `[views.*]`: how a whole route is serialized.
  - **Row shells** — `shell:` in a row's front matter (built
    2026-07-19): how much wrapper *one page* wears.
- **The three row tiers**, the chapter's centrepiece:

  | `shell:` | what you get |
  |---|---|
  | `none` | the body IS the output — no skeleton, no theme |
  | `light` | engine skeleton, canonical parts, no theme chrome |
  | `html` | the theme (the default for ordinary pages) |

- Closed vocabulary, checked at load and named with the file — a typo'd
  shell would otherwise render the wrong tier silently.
- **`none` is a capability, not a spelling**, and it's the chapter's
  worked example: an imported artifact (an old demo, a hand-built HTML
  page) can now carry front matter *and* still emit itself. Before, front
  matter nested the whole `<!doctype html>` inside a second document, so
  shipping it verbatim meant having no front matter — which meant it
  wasn't a row: no title, no metadata, invisible to every query. Now it's
  a row the database can see *and* a byte-exact artifact.
- Pair it with `hidden: true` — the honest way to keep an imported
  artifact out of the sitemap and the search index while keeping it
  linkable by source path (ties to ch. 10 and ch. 22).
- ★ What `none` does *not* do: lift the meat out of an imported page and
  render it through the theme. That's q50, and it's two operations
  (extraction, then chrome), deliberately not fused.
- Script shells: `[shells.llms] command = "python3 shells/llms.py"`;
  rows arrive as JSON on stdin, stdout is written at the route, non-zero
  exit fails the build. The experimental bench.
- **Gotcha with a real scar**: a script shell's source is a file in your
  tree, so it will be routed and *published* unless excluded. The example
  site shipped `shells/llms.py` this way. Add `shells/**` to `exclude`;
  `/llms.txt` still builds, because the command comes from config, not
  from a content row.
- ★ `md` shell specced; `/llms.txt` currently ships via a script shell.
- ★ Still open in q44: atom/sitemap becoming true part-map consumers; a
  `json` shell (though `cat` as a script shell already is one).

*Exit check for Part II: a theme of your own, with cards, a nav, and a
landing page.*

---

## Part III — Sites that get big

Target reader: 100+ files, several kinds of content, more than one
author. Here the database framing pays off and can finally be used.

### 19. The tree declares where, config declares what
- Marker files: `[markers] ".draft" = { draft = true }`, then `touch`.
- Same law as ch. 4, third appearance.
- What markers replace: `drafts/**` rules. What rules keep: routes, and
  patterns that cut *across* the tree (`**/*.scss`).
- Practical: hide a subtree from search with one `touch`.

### 20. Typed fields per subtree: `.schema.toml`
- `github_link = { type = "url" }`; types `string int bool list url image`.
- Buys three things at once: front-matter validation, filter
  type-checking, slot/field checking.
- Governed rows are strict (unknown key = load error naming the file);
  ungoverned rows stay tolerant.
- Worked example: `recipes/` with `course`, `time`; then group by it.
- ★ no list-of-records type, no JSON-LD emission (q40).

### 21. Hierarchy: the page's tree and the tree's tree
- Two axes, one recursive part kind (`outline_entry`), one fragment.
- **Heading axis**: `toc: true` (cascade it with a marker). Extracted
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

### 22. Drafts, hidden, and noindex
- The three flags, where they come from, what each means.
- `hidden` = routed but unlisted; `draft` = routed to `/drafts/{slug}/`.
- **Flags work on pages too** (fixed 2026-07-19) — same cascade as posts,
  front matter over marker/rule defaults. `hidden` reaches the row's
  route so star views filter it; `noindex` reaches the head. Worth a
  sentence on *why* this is called out: a page declaring `noindex: true`
  used to be accepted and silently dropped, which is the failure mode
  this whole system exists to prevent. Good place to teach "if a
  declaration seems ignored, `grackle explain` it."
- ★ **Profiles are specced, not built.** Today flags don't gate
  materialization — every `over = "*"` view must filter. Repeat the ch. 9
  warning; this is the sharpest edge in the system.
- ★ `/drafts/` is publicly crawlable today (q10). `noindex` is not in the
  route schema; listing `noindex` is an engine name-match (q33).

### 23. Bringing an existing site across

The migration chapter. Placed here because it needs routes (ch. 5),
shells (ch. 18) and flags (ch. 22) and nothing later. Written against
the real case: a 27-year tree where **187 of 227 page rows are
passthrough HTML** — hand-built demos, imported artifacts, pages older
than the tags they use.

- **Frame it as a spectrum, not a conversion.** Four tiers, and picking
  one per file *is* the migration:

  | the file is… | you do | it becomes |
  |---|---|---|
  | fine as-is, and you don't need it in queries | nothing | verbatim bytes, not a row |
  | fine as-is, but should be titled/searchable/linkable | front matter + `shell: none` | a row that emits itself |
  | worth engine chrome but not your theme | `shell: light` | canonical parts, null theme |
  | real content | front matter + markdown | an ordinary page |

  Most files stay in the top two rows. Say that plainly — a migration
  that demands rewriting 187 files is a migration nobody finishes.

- **The move that unlocks the rest**: `shell: none` means a file can be
  a database row *and* byte-exact output. Before, those were mutually
  exclusive — front matter nested the artifact inside a second `<html>`,
  and skipping front matter meant no title, no metadata, invisible to
  every query. Recover the whole tree's addressability without touching
  its bytes.
- **URL parity first, prettiness later.** Get the existing URL set
  reproduced, then change routes deliberately. `grackle diff --against
  <old-build>` is the instrument; `grackle query urls` the inventory.
  ★ Redirects for restructured trees are unsolved (q28) — state it early,
  because it decides whether someone can move at all.
- **What to do about the ugly parts of an import**: `hidden: true` keeps
  an artifact linkable but out of the sitemap and the search index; a
  `.noindex` marker does a whole subtree at once (ch. 19). This is the
  honest answer for demos and legacy trees, and it's one `touch`.
- **Where metadata comes from when the file can't carry front matter**
  (q49, ★ mostly unbuilt) — teach the precedence, since it's the shape
  the answer will take:
  1. **Derive** from the artifact. Measured: 14 of 57 raw HTML files
     carry a real `<title>` the database currently ignores, leaving 39
     user-facing rows titleless; object rows could carry their own
     bounds and format.
  2. **Declare** in a `.p01.png.toml` sidecar — the file-scoped member
     of the family that already exists at directory scope (`[markers]`,
     `.schema.toml`, `.section`).
  - ★ Neither half is built. Today the answer is front matter or nothing.
  - Worth stating as a principle, because it explains behaviour the
    reader will otherwise find arbitrary: **grackle reads what a file
    says; it does not guess from what a file omits.** A missing `<title>`
    does not mean "this is a fragment" — a 1996 page can be complete
    without `<html>`, and a real demo can have no title at all. Guessing
    fails toward not rendering something.
- ★ **Transplanting** — keeping an imported page's content but rendering
  it through your theme — is q50, and doesn't exist yet. Two operations,
  deliberately unfused: *extraction* (where's the meat: `<body>`'s
  children, or a selector) and *how much chrome the result wears*
  (`shell:`, which does exist). Today: `none` or rewrite by hand.
- **Order of operations**, the chapter's takeaway checklist: point
  `grackle.toml` at the tree → get the URL set to parity → add front
  matter only where a row must be queryable → pick shells → flag what
  shouldn't be indexed → *then* start converting to markdown, at
  whatever pace, forever.

### 24. Widgets, and the line at control flow
- `[widgets] callout = "<callout><div>\n\n{body}\n\n</div></callout>"`.
- Usage: `{% callout %}` … `{% endcallout %}`; body is ordinary markdown
  (no `markdown="1"`).
- No arguments, no conditionals — by design. An argumentful widget is the
  tripwire that says you want a template engine, and you don't.
- Errors: template with no `{body}`; missing end tag. Unregistered paired
  tags stay verbatim.
- Dogfood callout: the `★` and `note` boxes in this manual are widgets.

### 25. Blocks and rewrites
- Why the body is a block sequence, not a string. Three addressing modes:
  **position** (summaries), **selector** (rewrites), **identity**
  (notes).
- ★ `.rewrite.toml` is specced, unbuilt: `[[rule]] match = "table" wrap =
  "<div class='table-scroll'>"`. Selector subset is `lol_html`'s.
- ★ Notes as a second stream, and the sidenote payoff (ties back to
  ch. 14). Current summaries can ship dead footnote anchors — say so.
- Pipeline order, one diagram: tags → comrak → rewrites → layout picks
  blocks → theme.

### 26. Per-post CSS
- ★ Entirely specced: a `<style>` block in the body, SCSS, compiled,
  cached, hoisted, auto-scoped, `style_scope: false` to opt out.
- Where CSS belongs, decision table: one row → per-post `<style>`; a
  subtree → `.style.scss` ★; the whole site → theme.
- Gotcha to document now because the failure is invisible: **scoped SCSS
  cannot declare `:root` custom properties**.

### 27. Related posts
- `[related] limit`, `min_score`, `year_penalty`, `max_years` — policy
  only, no model choices in config.
- Embedded text is title/tags/body ⇒ retitling re-embeds.
- Related is **axes, not a list**: `similar`, `earlier`, `later`,
  `linked_from`, `translations`; `data-axis` for per-axis CSS. A new axis
  needs no theme change.
- `grackle query similar <url>`. Nothing embedded is ever published.
- ★ model upgrade / `reindex` undecided (q13).

### 28. Search
- The searchable set is a **query**, not a setting: `[views.search] over
  = "*" shell = "search"`.
- Engine assets (`search.bin`, `search.js`, `search.wasm`) ship
  automatically; **themes must not commit them**.
- A theme owes exactly two things: a trigger button and overlay CSS.
- Zero JS on the default page; ~288 KB on first click; last token is a
  live prefix.
- **The index holds prose, not markup**: raw-text elements (`<style>`,
  `<script>`) are skipped, so a styled post doesn't make `rgba` and
  `ffffff` searchable — while `margin` stays findable when a post
  actually discusses it. Relevant to anyone shipping `shell: none` rows
  or per-post CSS.
- Keeping things out of the index: `hidden: true` on the row, or narrow
  the view's `filter`. The searchable set is a query, so this is the
  same lever, not a second one.
- ★ overlay strings not localized.

### 29. More than one language
- `[i18n] default`, `locales`, `selector = "suffix" | "prefix"`.
- The load-time split: everything downstream sees the **logical** path.
  i18n off is a byte-identical no-op.
- A translation is a row, not a site copy.
- The switcher is just the `translations` axis — zero fragment changes.
- `LocalizedStr` anywhere a display name goes; the **three-level
  hierarchy**: inline > `[i18n.strings]` > engine built-in. `@key`
  references; `@@` escapes.
- Enum records for grouped-field value domains: `name` displays, `slug`
  routes, id stays the key.
- Locale-parallel views are **default-on**; opt out with `locales =
  "default"`. Empty locale ⇒ no routes.
- ★ Honest edges: `pretty_date`/`month_name` locale-free, `site.title`
  not localized, embedded views don't follow locale yet, prefix selector
  unexercised by a real corpus.
- ★ **The one a reader will actually hit (q47): listing views render no
  language switcher.** The `translations` axis is a *row* relation, so
  rows and mode-B landings get the switcher and a plain listing doesn't
  — a French reader landing on `/fr/blog/` has no way back. Say this
  outright and give the workaround (a `.slots/` locale link, or make the
  landing mode B).

*Exit check for Part III: a multi-section, multi-collection, searchable
site with typed content.*

---

## Part IV — Reference

Terse, generated where possible, no teaching. Each entry links back to
the chapter that teaches it.

### 30. Reference
- **30a. `grackle.toml`** — every key, alphabetical within table:
  `[site] [collections.*] [[collections.*.rules]] [markers] [views.*]
  [views.*.fields.*] [widgets] [records.*] [related] [links] [i18n]
  [i18n.names] [i18n.strings] [shells.*] [cache] [static]`. Mark
  built/specced per key. ★ flag the keys that don't mean what they say
  (`layout` on listings, `template` = "claims a file", row `layout:` as a
  four-tier flag) pending q33 — with the replacement named in each case,
  so the reference doesn't teach a spelling that's on its way out.
- **30b. Filter language** — grammar, operators, truthiness table, and
  the three field vocabularies (post / object / route).
- **30c. Part kinds** — the table from `parts.rs`; this *is* the theme
  API. Kind → parts → types (`Text Url Html Stream(k) Map(k) Flag`).
- **30d. Front matter** — reserved keys, per collection kind. Now
  includes `shell:` (`none`/`light`/`html`), and `hidden`/`noindex` on
  pages as well as posts.
- **30e. Tags in markdown** — `{% image %} {% post_url %} {% view %} {%
  include %}` (parameterless) + widgets. Note: unrecognised tags emit
  verbatim, deliberately.
- **30f. CLI** — build / serve / query / explain / diff, all flags.
- **30g. Error catalogue** — every load-time error, what it means, the
  fix. Sorted by message. High value: this is the page people land on
  from a search engine.
- **30h. Glossary** — row, table, view, route, part, slot, fragment,
  kind, variant, shell, axis, marker, landing, claim, scope chain.

---

## Part V — Understanding grackle

Optional reading. Explains the shape so users can predict behaviour
rather than memorize it.

### 31. What grackle is not
- Confirmed non-goals with reasons: comments, memberships/paywalls,
  ratings, live/external data, stateful interactive widgets as *modeled*
  content, control flow in templates, AST access, vector indexes.
- The honest workaround for each (edge/CDN for entitlements; ETL that
  commits data; raw passthrough + per-row assets).
- Says clearly: if you need these, use something else. That's fine.

### 32. Why it's shaped this way
- The four layers and their different rates of change.
- The recurring law, one more time, with all six of its appearances.
- Why load-time errors instead of 404s.
- The two honest weaknesses: themes need Rust for new part vocabulary;
  head facts aren't per-theme selectable ★.
- Pointer to `DESIGN.md` for anyone who wants the full argument.

### 33. What isn't real yet
- The ledger, in one table: profiles, `.style.scss`, `.slots/` typed
  fills, rewrites + notes stream, per-post `<style>`, `theme.toml`, md
  shell, CEL expressions, board kind, serve v2, pagination ×
  subdivision, per-block facts, audio/video field types, faceted
  filtering, transclusion.
- Each row: what it would look like, what blocks it, the q number.
- **Landed since the first draft of this outline** — move out of the
  ledger, into the chapters named: row-level shells (q44 → ch. 18),
  page flags (ch. 22), search skipping raw-text elements (ch. 28).
- **New arrivals** (2026-07-19), all user-visible enough to list:
  - q47 — no language switcher on listing views (ch. 29).
  - q48 — `type:` as row data. Held until something other than the
    renderer consumes it. Mention only if a reader asks "how do I say
    what a page *is*"; today the answer is subtree position +
    `.schema.toml`.
  - q49 — metadata for files that can't carry front matter: derive from
    the artifact (14 of 57 raw HTML files have a `<title>` the db
    ignores; object rows could carry their own bounds), then a
    `.p01.png.toml` sidecar. Affects anyone importing a legacy tree.
  - q50 — transplanting an imported page (extraction + chrome), and the
    blocked "deliberate omission vs forgotten hole" question underneath
    it (ch. 15).
- Kept current or deleted. A stale version of this page is worse than no
  page.

---

## Open questions about the manual itself

1. **Chapter count.** 33 is a lot. Candidates to merge: 14+15 (CSS +
   variants), 25+26 (blocks + per-post CSS, both largely ★), 31+32.
2. **Does Part I need a theme at all?** Currently Part I renders with
   canonical markup and Part II introduces themes. Alternative: ship a
   starter theme in ch. 3 so the first screenshot isn't unstyled. Leaning
   starter theme, since first impressions are the whole job of Part I.
3. **★-heavy chapters (25, 26) may be premature.** Option: hold them out
   of v1 and let ch. 33 carry them until stage B lands.
4. **Release notes as posts** is the natural dogfood, but it means the
   manual has a publishing cadence. Acceptable?
5. **Where does the manual site live and deploy?** `grackle/manual/` in
   this repo, served at `grack.com/grackle/`? Own repo later?
6. **Ch. 23 (migration) is half ★ — does it ship in v1?** The chapter
   now exists and the *built* half stands on its own (the four tiers,
   `shell: none`, flags, URL parity). The unbuilt half is the ending:
   q49 metadata derivation, q50 transplant, q28 redirects. Two of those
   are the questions a migrator asks first, so the chapter is honest but
   unsatisfying. Options: ship it as-is with the ★s loud; or hold it and
   let ch. 33 carry the material until q49's derive half lands (which is
   cheap — reading a `<title>` that's already there).
8. **Does ch. 23 want a worked migration?** A real before/after on a
   small imported tree would carry it better than any amount of prose.
   The example site now has one occupant (`demos/pane.html`); a second
   showing the `shell: light` tier would make the spectrum visible.
9. **Reference generation.** 30a/30b/30c should be generated from the
   Rust (config structs, `parts.rs`, error enums) or they will rot within
   a month. That's plausibly a `grackle docs` subcommand — a real feature
   request falling out of writing this.
