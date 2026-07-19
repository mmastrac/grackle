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
  (§7b). Point at ch. 30.
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
  is Rust — see ch. 31.

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
  part with no warning (q45 leftover).

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

### 18. Shells: the outermost serialization
- `shell = "html" | "atom" | "sitemap" | "search"`.
- Script shells: `[shells.llms] command = "python3 shells/llms.py"`;
  rows arrive as JSON on stdin, stdout is written at the route, non-zero
  exit fails the build. The experimental bench.
- ★ `md` shell specced; `/llms.txt` currently ships via a script shell.
- ★ row-level shells undecided (q44).

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
- ★ **Profiles are specced, not built.** Today flags don't gate
  materialization — every `over = "*"` view must filter. Repeat the ch. 9
  warning; this is the sharpest edge in the system.
- ★ `/drafts/` is publicly crawlable today (q10). `noindex` is not in the
  route schema; listing `noindex` is an engine name-match (q33).

### 23. Widgets, and the line at control flow
- `[widgets] callout = "<callout><div>\n\n{body}\n\n</div></callout>"`.
- Usage: `{% callout %}` … `{% endcallout %}`; body is ordinary markdown
  (no `markdown="1"`).
- No arguments, no conditionals — by design. An argumentful widget is the
  tripwire that says you want a template engine, and you don't.
- Errors: template with no `{body}`; missing end tag. Unregistered paired
  tags stay verbatim.
- Dogfood callout: the `★` and `note` boxes in this manual are widgets.

### 24. Blocks and rewrites
- Why the body is a block sequence, not a string. Three addressing modes:
  **position** (summaries), **selector** (rewrites), **identity**
  (notes).
- ★ `.rewrite.toml` is specced, unbuilt: `[[rule]] match = "table" wrap =
  "<div class='table-scroll'>"`. Selector subset is `lol_html`'s.
- ★ Notes as a second stream, and the sidenote payoff (ties back to
  ch. 14). Current summaries can ship dead footnote anchors — say so.
- Pipeline order, one diagram: tags → comrak → rewrites → layout picks
  blocks → theme.

### 25. Per-post CSS
- ★ Entirely specced: a `<style>` block in the body, SCSS, compiled,
  cached, hoisted, auto-scoped, `style_scope: false` to opt out.
- Where CSS belongs, decision table: one row → per-post `<style>`; a
  subtree → `.style.scss` ★; the whole site → theme.
- Gotcha to document now because the failure is invisible: **scoped SCSS
  cannot declare `:root` custom properties**.

### 26. Related posts
- `[related] limit`, `min_score`, `year_penalty`, `max_years` — policy
  only, no model choices in config.
- Embedded text is title/tags/body ⇒ retitling re-embeds.
- Related is **axes, not a list**: `similar`, `earlier`, `later`,
  `linked_from`, `translations`; `data-axis` for per-axis CSS. A new axis
  needs no theme change.
- `grackle query similar <url>`. Nothing embedded is ever published.
- ★ model upgrade / `reindex` undecided (q13).

### 27. Search
- The searchable set is a **query**, not a setting: `[views.search] over
  = "*" shell = "search"`.
- Engine assets (`search.bin`, `search.js`, `search.wasm`) ship
  automatically; **themes must not commit them**.
- A theme owes exactly two things: a trigger button and overlay CSS.
- Zero JS on the default page; ~288 KB on first click; last token is a
  live prefix.
- ★ overlay strings not localized.

### 28. More than one language
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

*Exit check for Part III: a multi-section, multi-collection, searchable
site with typed content.*

---

## Part IV — Reference

Terse, generated where possible, no teaching. Each entry links back to
the chapter that teaches it.

### 29. Reference
- **29a. `grackle.toml`** — every key, alphabetical within table:
  `[site] [collections.*] [[collections.*.rules]] [markers] [views.*]
  [views.*.fields.*] [widgets] [records.*] [related] [links] [i18n]
  [i18n.names] [i18n.strings] [shells.*] [cache] [static]`. Mark
  built/specced per key. ★ flag the keys that don't mean what they say
  (`layout` on listings, `template` = "claims a file") pending q33.
- **29b. Filter language** — grammar, operators, truthiness table, and
  the three field vocabularies (post / object / route).
- **29c. Part kinds** — the table from `parts.rs`; this *is* the theme
  API. Kind → parts → types (`Text Url Html Stream(k) Map(k) Flag`).
- **29d. Front matter** — reserved keys, per collection kind.
- **29e. Tags in markdown** — `{% image %} {% post_url %} {% view %} {%
  include %}` (parameterless) + widgets. Note: unrecognised tags emit
  verbatim, deliberately.
- **29f. CLI** — build / serve / query / explain / diff, all flags.
- **29g. Error catalogue** — every load-time error, what it means, the
  fix. Sorted by message. High value: this is the page people land on
  from a search engine.
- **29h. Glossary** — row, table, view, route, part, slot, fragment,
  kind, variant, shell, axis, marker, landing, claim, scope chain.

---

## Part V — Understanding grackle

Optional reading. Explains the shape so users can predict behaviour
rather than memorize it.

### 30. What grackle is not
- Confirmed non-goals with reasons: comments, memberships/paywalls,
  ratings, live/external data, stateful interactive widgets as *modeled*
  content, control flow in templates, AST access, vector indexes.
- The honest workaround for each (edge/CDN for entitlements; ETL that
  commits data; raw passthrough + per-row assets).
- Says clearly: if you need these, use something else. That's fine.

### 31. Why it's shaped this way
- The four layers and their different rates of change.
- The recurring law, one more time, with all six of its appearances.
- Why load-time errors instead of 404s.
- The two honest weaknesses: themes need Rust for new part vocabulary;
  head facts aren't per-theme selectable ★.
- Pointer to `DESIGN.md` for anyone who wants the full argument.

### 32. What isn't real yet
- The ledger, in one table: profiles, `.style.scss`, `.slots/` typed
  fills, rewrites + notes stream, per-post `<style>`, `theme.toml`, md
  shell, CEL expressions, board kind, serve v2, pagination ×
  subdivision, per-block facts, audio/video field types, faceted
  filtering, transclusion.
- Each row: what it would look like, what blocks it, the q number.
- Kept current or deleted. A stale version of this page is worse than no
  page.

---

## Open questions about the manual itself

1. **Chapter count.** 32 is a lot. Candidates to merge: 14+15 (CSS +
   variants), 24+25 (blocks + per-post CSS, both largely ★), 30+31.
2. **Does Part I need a theme at all?** Currently Part I renders with
   canonical markup and Part II introduces themes. Alternative: ship a
   starter theme in ch. 3 so the first screenshot isn't unstyled. Leaning
   starter theme, since first impressions are the whole job of Part I.
3. **★-heavy chapters (24, 25) may be premature.** Option: hold them out
   of v1 and let ch. 32 carry them until stage B lands.
4. **Release notes as posts** is the natural dogfood, but it means the
   manual has a publishing cadence. Acceptable?
5. **Where does the manual site live and deploy?** `grackle/manual/` in
   this repo, served at `grack.com/grackle/`? Own repo later?
6. **Reference generation.** 29a/29b/29c should be generated from the
   Rust (config structs, `parts.rs`, error enums) or they will rot within
   a month. That's plausibly a `grackle docs` subcommand — a real feature
   request falling out of writing this.
