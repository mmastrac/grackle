# grackle — a virtual database over the site, with a renderer attached

Rust replacement for the Jekyll build of grack.com. Goal: **equivalent rendered
HTML for most of the blog**, byte-identical URL set, then iterate toward
exactness using the existing Jekyll output as a golden reference.

## 0. The tour: one post, end to end

The one-sentence model: **the site is a database that happens to live in git,
and a theme is a stylesheet with opinions about where things go.** Everything
in between is a pipeline of typed, checkable steps:

```
file → row → query → doc model → part map → slots → CSS → URL
```

This section follows one post through all of them. Honesty note, per house
rules: every step below is built and measured, with three deliberate gaps —
the doc model's `notes` stream (§6d stage B), the per-theme `theme.toml`
head-fact selection (the engine renders all head facts today), and serve's
incremental invalidation (v1 rebuilds the world in ~0.4s; §7).

### 1. You write a file

```
_posts/2026/2026-07-17-espresso-grinder.md
```
```markdown
---
title: Rebuilding the espresso grinder
tags: [hardware]
---
The burrs were toast.[^why] Here's the teardown.

[^why]: Twenty years of oily beans.

![The culprit](burrs.jpg)
```

That is the whole authoring interface: a markdown file, in git, with minimum
front matter. No layout declared, no URL, no path to `burrs.jpg`.

### 2. The database claims it (§1–§3)

Directories are tables, files are rows. Every file belongs to **exactly one**
table, by precedence — posts, then objects (by extension), then tree:

```toml
[collections.blog]
kind   = "posts"
source = "_posts"
filename_formats = ["{year}-{month}-{day}-{slug}"]

[collections.objects]
kind = "objects"
extensions = ["png", "jpg", "jpeg", "gif", "webp", "svg"]

[collections.pages]
kind = "tree"
source = "."
```

The post lands in `blog`; `burrs.jpg` lands in `objects`; everything else on
the site is `tree`. The row's columns then fill by **one precedence law used
everywhere: nearest wins, first writer per key** —

| source | example | rank |
|---|---|---|
| front matter | `title:`, `tags:` in the file | always wins |
| markers (§4b) | `_posts/drafts/.draft` → `draft = true` for the subtree | nearest ancestor |
| rules (§4) | the `**` catch-all below | most distant |

```toml
[markers]
".draft" = { draft = true }        # config says what a marker MEANS;
                                   # the tree says WHERE it applies

[[collections.blog.rules]]
match    = "**"
defaults = { layout = "post" }
route    = "/blog/{year}/{month:02}/{day:02}/{slug}/"
```

The filename yields `(date, slug)`, the rule yields the route, and the row is
addressable: `/blog/2026/07/17/espresso-grinder/`. Everything is **checked at
load time, not discovered as a 404** — two rows on one URL, a dated route on
an undated row, a rule matching nothing: all errors naming the file and rule.

### 3. Views query it (§5, §5c)

Nobody writes `{% for post in site.posts %}{% unless post.draft %}`. Queries
are declared once:

```toml
[views.published]                  # a named query: no route, no layout
over   = "blog"
filter = "!draft && !hidden"

[views.blog_index]                 # materialized: query + routes
over = "published"
paginate = 5
routes = ["/blog/", "/blog/page/{n}/"]

[views.latest]                     # embeddable: query + layout, no route
over = "published"
limit = 3
```

**A view is a query; a route is just where it lands.** The new post enters
`published` and therefore appears in `/blog/`, the feed, the `hardware` tag
page, the July 2026 archive, and the home page — the *same query*, composed,
defined in one place. Filters are parsed and type-checked at load:

```
view blog_index: filter "!drafts"
  unknown field `drafts` (did you mean `draft`?)
    known fields: body_bytes, date, day, description, draft, hidden, ...
```

### 4. Rendering produces structure, not a string (§6d)

The body does not become one HTML blob. It becomes a **doc model**:

```rust
pub struct Doc   { blocks: Vec<Block>, notes: Vec<Note> }
pub struct Block { html: String, tag: &'static str, notes: Vec<usize> }
pub struct Note  { name: String, num: u32, html: String }
```

- **blocks** — the top-level sequence, addressable by *position*. A summary is
  literally `blocks[..cut]` — listings ship 2 paragraphs, not full bodies
  hidden by CSS (~93% of `/blog/`'s page weight deleted).
- **notes** — the footnote, a second stream associated with its block by
  *identity*. Where it renders is deliberately not decided yet.
- **rewrites** — rules addressing rendered HTML by *CSS selector*:

```toml
[[rule]]
match = "table"
wrap  = "<div class='table-scroll'>"
```

- **facts** — typed truths: the row has a date → `og:type=article`; the
  summary was cut → `data-truncated`; `burrs.jpg` resolved (bare name →
  nearest sibling or bucket, §6a) with known dimensions → `width`/`height`
  on the `<img>`.

Position, selector, identity: three addressing modes, and that trio is what
"reach into the markdown" means here.

### 5. A layout kind fills named parts (§5a, §5e)

This route is a `document` (the kinds: `document`, `listing`, `feed`, `raw`).
It emits a **part map, not a page**:

```
title:     "Rebuilding the espresso grinder"    text
crumbs:    Home > Blog > 2026 July > 17         fragment (schema-driven)
tags:      [hardware]                           stream
content:   <the block stream>                   stream
notes:     [^why]                               stream
neighbors: prev/next                            stream
```

Each part is flat, semantic HTML. No wrapper divs, no arrangement — the
layout kind genuinely does not know whether footnotes will become a sidebar.

### 6. The theme places parts in slots (§5e)

A theme is a **directory of data** — no code, no recompile:

```
themes/default/
  theme.toml       # which head facts to render
  shell.html       # the outer skeleton
  document.html    # optional per-kind arrangement
  theme.css
```

A fragment is straight-line HTML with holes; the whole hole algebra is three
rules — a hole is `data-slot`; **an empty part deletes its element** (every
`{% if %}` you'll never write); **a stream maps a fragment over its items**
(every `{% for %}`):

```html
<article data-kind="document">
  <nav data-slot="crumbs"></nav>
  <h1 data-slot="title"></h1>
  <div data-slot="content"></div>
  <aside data-slot="notes"></aside>       <!-- absent notes ⇒ no <aside> -->
  <nav data-slot="neighbors" data-fragment="neighbor"></nav>
</article>
```

Unknown slot names are load-time errors, exactly like filter typos.

### 7. CSS does the geometry (§5e)

Modern CSS is the declared baseline — nesting, `:has()`, container queries,
`@layer`, `aspect-ratio` — and all arrangement lives here:

```css
@layer theme {
  [data-kind="document"] {
    display: grid;
    grid-template-areas: "crumbs content" "tags content";
  }
  /* this theme wants Tufte sidenotes: claim the stream, add a column */
  article:has(> [data-slot="notes"]) {
    grid-template-areas: "crumbs content notes";
  }
}
```

The footnote just became a sidenote, and no layer above CSS was consulted. A
theme that doesn't claim `notes` gets the endnote fallback. The `light` theme
ships no fragments and no CSS at all — the null theme, proving the canonical
markup stands alone.

### 8. Build, serve, query — clients of one database (§7)

```
$ grackle build                    # pin a snapshot, materialize every route  (~225ms; Jekyll ~90s)
$ grackle serve                    # resident db: save → invalidate exactly the
                                   # affected pages → browser reload ping (ms)
$ grackle query 'posts where "rust" in tags limit 5'
$ grackle explain /blog/2026/07/17/espresso-grinder/
                                   # → row, rules that matched, deps, cache state
```

### Day two: every change has exactly one home

| you want | you touch |
|---|---|
| a new post | one markdown file |
| hide a subtree from search | `touch code/legacy/.noindex` |
| a "recent Rust posts" box | a `[views.*]` entry: `filter = '"rust" in tags'` |
| a photo-gallery page | a view `variant` + a `card` fragment + grid CSS |
| a new look, dark mode included | copy a theme directory, edit HTML + CSS |
| one weird table in one post | a `<style>` block there — scoped, compiled, validated (§6c) |
| footnotes in the margin | ~4 lines of theme CSS |

The rule that keeps it honest: **want an `if` in a fragment → you're missing a
fact; want a `for` → you're missing a view** (§5d). Both are design bugs, and
the load-time checker is the tripwire.

## 1. Core idea

grackle is a **virtual, on-disk database**: the filesystem is the storage
layer, and grackle maintains a live, queryable view over it. Nothing is
"loaded" in a build step that then exits — the database is the resident
object, and everything else is a client of it:

- **Tables** are directories (posts, page tree). **Rows** are files.
- Rows are **virtual**: hydrated lazily, on demand, in stages
  (stat → front matter → body → rendered HTML), each stage cached.
- **File watchers are the replication stream**: fs events become row
  upserts/deletes that advance the database revision and invalidate exactly
  the cached derivations that depended on the changed rows.
- **Queries/views** (tag groups, archives, pagination, feeds, adjacency)
  are demand-driven and memoized against the current revision.
- `build` = materialize one consistent snapshot to disk (AOT).
- `serve` = keep the database resident; render pages **on demand** per HTTP
  request; push change notifications to browsers.

## 2. Storage engine

```
FsStore
  ├─ table mapping: directory ↔ table (from config)
  ├─ row identity: source path (tree) / extracted (date, slug) (posts)
  ├─ row version:  content hash (mtime+size as a fast pre-check)
  └─ event ingest: notify watcher → debounced batch → one transaction
```

- **Hydration stages per row**, each pull-through cached:
  1. `stat` — existence, version
  2. `head` — front matter only (cheap; enough for indexes, lists, routing)
  3. `body` — raw content
  4. `rendered` — liquid → markdown → HTML (pre-layout)

  Index/list queries (`by_tag`, ordering, titles for next/prev) only ever
  force stage 2. Only an actual page render forces stage 4.

- **Revisions & snapshots (MVCC-ish).** Each ingested event batch produces a
  new revision. Readers (an HTTP request, a `build` run) pin an immutable
  snapshot (`Arc`-swapped), so a mid-render edit can never tear output —
  `build` renders the entire site from exactly one revision.

- **Debounced transactions.** Editor save-storms (write + rename + chmod)
  coalesce into one revision, one invalidation pass, one reload ping.

- **Invalidation** is dependency-tracked but deliberately coarse-grained,
  tracked per derived value as a set of typed keys:
  `Row(path)`, `Index(blog.order)`, `Index(blog.tags)`, `Template(name)`,
  `Config`. Example: a post body edit invalidates that post's `rendered`,
  pages that embed bodies (blog index pages containing it), and the feed —
  but not tag pages (they read stage-2 fields only) unless front matter
  changed. Template/include/scss edits invalidate by `Template(...)` key.
  (Considered: `salsa` for automatic fine-grained tracking; at 327 posts the
  hand-rolled version is simpler, debuggable, and fast enough. Revisit if
  dependency bugs bite. → Open question 1.)

## 3. Tables

**Row identity is always the source path**, for both table kinds. `(date, slug)`
is a *unique index* over posts, not the primary key — drafts have no date in
their filename, so a `(date, slug)` PK can't represent them. Identity =
path keeps every row addressable; dated-ness is a property, not an identity.

| Table kind | Identity      | Primary index          | Source         |
|------------|---------------|------------------------|----------------|
| `posts`    | source path   | `(date, slug)` unique  | `_posts/**`    |
| `tree`     | source path   | path hierarchy         | site root      |
| `objects`  | source path   | `by_name` (non-unique) | by extension   |

- **Posts**: ordered rows, reverse-chronological over the dated set.
  Secondary indexes: `by_slug` (for `post_url`), `by_tag`, `by_year_month`,
  adjacency (`next`/`previous`). Undated rows (drafts) are absent from the
  chronological/`by_year_month` indexes and sort last anywhere they appear.
  Indexes are built from stage-2 hydration only and carry their own revision
  counters (so "a body changed" ≠ "the ordering changed").
- **Tree pages**: hierarchical. Derived relations: `ancestors(page)`
  (breadcrumbs — a tree query, replacing the URL-string-walking plugin),
  `children(page)`.
- **Objects**: binary assets, selected by extension rather than directory —
  they live wherever they live. `by_name` is deliberately **non-unique**
  (measured: `screenshot5.png` and `screenshot6.png` genuinely collide), so
  resolution is a query that can fail, not a map lookup. See §6a for
  reference resolution, §4 for routing.

### Membership is disjoint

A file belongs to **exactly one** table, resolved by precedence:

1. **posts** — under a posts collection's `source`, and a `.md`
2. **objects** — matches a configured extension
3. **tree** — everything else

Stated once, this removes a whole class of ambiguity: without it, `assets/x.png`
would match both the objects table (by extension) and the tree's `**/*`
passthrough, and route-collision detection (§4) would fire on every image in
the site.

## 4. Schema: rules (defaults + routing)

One mechanism covers both "everything in this folder is a draft" and "this
path shape gets this URL": an **ordered list of rules** per collection. Each
rule is `match` (glob, relative to the collection source) plus any of
`defaults` (front-matter values) and `route`.

The DB analogy is a `DEFAULT` clause scoped by a predicate: rules supply
column values for the subset of rows they match. They don't partition the
table — they overlap and cascade.

```toml
# grackle.toml

[site]
url     = "https://grack.com"
baseurl = ""            # dev override via --config or CLI flag
title   = "grack.com"
author  = "Matt Mastracci"

[collections.blog]
kind   = "posts"
source = "_posts"
# Filename → (date, slug) extraction, tried in order.
# Second form covers the legacy MM-DD-YYYY posts; no match ⇒ undated row.
filename_formats = ["{year}-{month}-{day}-{slug}", "{month}-{day}-{year}-{slug}"]

  [[collections.blog.rules]]
  match    = "drafts/**"
  defaults = { draft = true }
  route    = "/drafts/{slug}/"          # undated: route must not use {year}

  [[collections.blog.rules]]
  match    = "hidden/**"
  defaults = { hidden = true }          # routed normally; excluded from lists

  [[collections.blog.rules]]
  match    = "**"                       # everything else, default flags
  defaults = { layout = "post" }
  route    = "/blog/{year}/{month:02}/{day:02}/{slug}/"

[collections.pages]
kind   = "tree"
source = "."

  [[collections.pages.rules]]
  match = "**/index.{html,md}"
  route = "/{dir}/"

  [[collections.pages.rules]]
  match = "**/*.{html,md}"
  route = "/{dir}/{stem}/"

  [[collections.pages.rules]]
  match = "**/*"
  route = "/{path}"                     # static passthrough

[collections.objects]
kind       = "objects"
extensions = ["png", "jpg", "jpeg", "gif", "webp", "svg"]
bucket     = "assets"                   # §6a: bucket dir NAME, not a path

  # Named routes: pin an object to a stable URL regardless of where it lives
  [[collections.objects.rules]]
  match = "assets/branding/logo-v3-final.png"
  route = "/logo.png"

  [[collections.objects.rules]]
  match = "**"
  route = "/{path}"                     # default: publish at literal source path
```

### Named object routes

Objects are routed by the same ordered rules as everything else, which makes
"publish this object at a specific named route" fall out for free rather than
needing a mechanism of its own.

The default `**` → `/{path}` keeps every original where it is today, which
matters more than it looks: `{% image %}` emits
`<a href="/assets/…/foo.jpeg"><img src="{thumb}"></a>` — the **thumbnail
links to the original at its literal path** — and the layout hardcodes
`/resource/profile.jpg` and nine `/resource/favicon/*.png` paths. Under the
default rule all of those keep working untouched, so named routes are purely
additive: a way to decouple an object's *published* URL from its *source*
path (`assets/branding/logo-v3-final.png` → `/logo.png`) when you want one.

Note this is about **originals**. Derived variants (thumbnails) are a separate
concern and live under `static.dir`, content-addressed — see §6b.

### Resolution order

**First writer wins, per key.** Rules are evaluated top to bottom; a rule may
only set keys nobody above it set. Specific rules go first, the `**` catch-all
last — which is exactly how the feature reads aloud.

Worked example, `_posts/drafts/foo.md`:

| Source | Contributes | Result |
|---|---|---|
| rule 1 `drafts/**` | `draft=true`, route `/drafts/{slug}/` | both taken |
| rule 2 `hidden/**` | — | no match |
| rule 3 `**` | `layout="post"`, route `/blog/...` | `layout` taken; **route already set → ignored** |

→ `draft=true`, `layout="post"`, URL `/drafts/foo/`. The draft inherits the
catch-all's layout without restating it, and overrides only the route.

**Front matter in the file always beats every rule.** Rules are defaults; an
explicit `permalink:` or `hidden: false` in the file wins outright (Jekyll
compat — `atom.xml` and friends rely on `permalink:` pinning literal paths).

This one mechanism subsumes `draft_preview.rb`, the `defaults:`/`scope:`
blocks in `_config-preview-drafts.yml`, and `monkeypatch.rb`'s page-permalink
rules.

### Constraints (checked at transaction time, not discovered as 404s)

- **Route collisions** → error, naming both rows.
- **Undated row routed by a dated template**: a rule whose `route` uses
  `{year}/{month}/{day}` matching a row with no date → error naming the file
  and the rule. This is the guardrail that makes undated drafts safe.
- **Dead rule** (matches zero rows) → warning; it's almost always a typo.
- **URL-set parity** with the Jekyll sitemap is a hard requirement
  (`grackle diff`) — this month's canonical/indexing work depends on the URL
  set not shifting. See §4a for the one intentional exception.

The route map doubles as the **reverse index for serving**: URL → row is a
lookup in the same structure, so `serve` needs no output directory at all.

## 4a. Profiles: `hidden` and `draft` add no public URLs

Reality check on the current site:

- `_hidden/` holds **14 real dated posts** and is **not built at all** today —
  it starts with `_`, so Jekyll ignores it.
- **No post anywhere sets `hidden:`.** `post.hidden` is therefore always
  false, and the `{% unless post.hidden %}` guards in `atom.xml`,
  `post.html` (next/prev/related), and `monthly_archive.html` are vestigial —
  written for exactly this feature, never yet exercised.

**Flags gate materialization, and the public profile emits nothing new.**
A build **profile** declares which flagged rows exist at all:

```toml
[profiles.public]                       # default; what publish.sh ships
include = "!draft && !hidden"           # flagged rows are not materialized

[profiles.drafts]                       # the /drafts mirror build
baseurl = "/drafts"
include = "*"                           # everything, flags and all
views   = ["hidden_index", "drafts_index"]   # profile-scoped views

[views.hidden_index]
over     = "blog"
filter   = "hidden"
route    = "/hidden/"                   # only exists in the drafts profile
layout   = "monthly_archive"            # a plain list of shelved posts
```

Consequences, all good:

- **Public build: zero new URLs.** Hidden posts aren't routed, aren't in
  `site.posts`, don't reach the feed — identical to today's "not built at
  all", so URL parity stays inviolable and §11.7 is settled.
- **`/hidden/` is a drafts-profile view**, listing the shelved posts for
  review. It rides the existing `_config-prod-drafts.yml` mirror (which is
  already a second build to `_site/drafts`), so it costs no new machinery.
- **The dormant guards become correct rather than vestigial**: inside the
  drafts profile, hidden posts *are* in `site.posts`, and
  `{% unless post.hidden %}` is what keeps them out of that build's feed and
  archives. Same template, right behavior in both profiles.
- **The adjacency bug evaporates** (§11.8 settled). Public build: hidden rows
  don't exist, so `page.next` can never point at one and no empty "Later
  post" block can render. The gap-vs-skip question only ever arises inside
  the drafts profile, where it doesn't matter.

| Flag | profile `public` | profile `drafts` |
|---|---|---|
| `hidden` | not materialized — no URL, no feed, no lists | routed; listed at `/hidden/`; excluded from feed/archives by the template guards |
| `draft` | not materialized | routed `/drafts/{slug}/`; listed at `/drafts/` |

Note the `/drafts` mirror is itself published (rsync'd to `_site/drafts`
today) — unlinked, but publicly reachable. Worth a `noindex` on that
profile given this month's indexing work (→ open question 10).

### ⚠️ Profiles are still specced-not-built — but the leak is now closed

Profiles do not exist in code. `grackle.toml` has no `[profiles.*]`; instead
the blog collection's rules route flagged rows into the **main build**:

```toml
[[collections.blog.rules]]
match = "drafts/**"
defaults = { draft = true }
route = "/drafts/{slug}/"
```

That routing once leaked. Probed by adding two posts dated *newer* than anything
real — one draft, one hidden — the flagged rows landed in the sitemap (573 → 575)
even though `published`/`latest`/`/blog/` correctly excluded them. A section
titled "add no public URLs" was emitting the most public URL there is.

**Fix (1) has now landed** (the small one, recommended "before any draft is
written"). `draft` and `hidden` are carried onto every `Route` — false for every
non-post, since only posts can be flagged — and exposed in `route_schema()`, so
the sitemap filter reads what it needs to:

```toml
[views.sitemap]
filter = '!draft && !hidden && (dir || ext == "html" || ext == "pdf")'
```

Re-probed after the fix: with the two future-dated probes present (329 posts),
the sitemap stays at **573** and neither probe appears, while the draft still
routes and renders at `/drafts/probe-draft/`. The flags are now *safe*, not just
latent — armed the moment a draft is written, and correct by construction rather
than by the corpus happening to have 0 drafts.

This was **grackle's divergence, not Jekyll's**. `publish.sh` builds drafts as a
*separate site* (`--config _config-prod-drafts.yml --destination
/site/_site/drafts`), so Jekyll's main sitemap never saw them. Routing drafts
into the main build created the exposure; the route-level flag closes it.

Fix (2) — **profiles** — still stands as the proper answer, and dissolves the
question entirely: a row that isn't materialized cannot be in a route set, so no
view can leak it and no view has to remember not to. It lands with phase 3.
Until then, the filter carries the discipline, and open question 19 is settled.

Given this whole project began with *"I'm having trouble with Google crawling
this site"*, this was precisely the wrong failure mode to ship — so it is the
first correctness gap closed on the way to a full `build`.

## 4b. Marker files: defaults declared by the tree

A marker file sets defaults for **its directory and everything below it**. The
config says only *what* a marker means; the tree says *where* it applies.

```toml
[markers]
".noindex" = { noindex = true }
".hidden"  = { hidden = true }
".draft"   = { draft = true }
```

```
_posts/
  hidden/
    .hidden                 <- every post in here, and below, is hidden
    2003-07-31-net-ranting.md
code/legacy/
  .noindex                  <- the whole legacy subtree, one file
  romtool/index.html
```

This is the **same principle as buckets** (§6a): positional resolution, nearest
wins. Add a marker → it applies from there down. Delete it → the default lifts.
`mkdir`/`touch` is the interface; the config never names a path.

### Resolution

Walk up from the row's directory to the root, accumulating **first-writer-wins
per key** — so the *nearest* marker shadows a shallower one, exactly like
sibling assets shadow bucket assets.

Full precedence for any default, highest first:

| Source | Why it ranks there |
|---|---|
| **Front matter** | Explicit, on the row itself |
| **Markers** (nearest ancestor) | Positional and local: it sits *in* the tree it describes |
| **Rules** (§4, first-writer) | A glob in config — the most distant statement about a row |

A marker file is never routed, and is exempt from the dotfile skip that would
otherwise hide it from the walk.

**That exemption has a cost worth knowing about.** The marker scan cannot reuse
the tree walk's dotfile/underscore skip (markers *are* dotfiles, and they live
under `_posts`), so it must carry the `exclude` globs itself — each rewritten
to also match the *directory* (`_site*/**` → `_site*`) so walkdir prunes the
subtree rather than walking into it. Without that it descends into `_site*`,
`vendor` and `target`: **~80ms instead of ~6ms**. It remains a second,
name-only walk over ~1500 files; folding it into the existing walks is an
available optimisation if the ~6ms ever matters.

### What this replaces

The `drafts/**` and `hidden/**` rules in §4 become unnecessary — the directory
declares itself:

```toml
# before: config encodes the path
[[collections.blog.rules]]
match = "hidden/**"
defaults = { hidden = true }

# after: the tree encodes the path
[markers]
".hidden" = { hidden = true }
```

The rules mechanism stays for what it is genuinely better at: routes (`route =`
is not a per-directory concept) and patterns that cut *across* the tree
(`**/*.scss`). Markers own the per-subtree defaults.

### `noindex` and the layout chain

Markers make `noindex` computable from the tree — but not *completely*: a
layout can also set it (`tag_index.html` has `noindex: true`, which is why the
noindex'd tag pages exist at all). So:

- **Posts**: marker + front matter is the whole story — no post layout sets
  `noindex` — so the field is complete and safe to expose.
- **View routes**: still need the layout chain (phase 2). `noindex` therefore
  stays out of the *route* schema (§5) until then, on the same reasoning as
  before: a field we cannot populate correctly is worse than no field.

## 4c. What counts as content: three layers

`gitignore = true` (default). Three mechanisms, each doing what only it can:

| Layer | Covers | Why it can't be one of the others |
|---|---|---|
| **`.gitignore`** | build artifacts: `_site*`, `_log*`, `vendor`, `_cache`, `grackle/target`, `.jekyll-cache`, `.sass-cache`, `.bundle`, `workspace`, `_tools/converter/target` | Already written, already authoritative, **self-maintaining** — add a line and grackle stops seeing it |
| **dot/underscore skip** | `_posts`, `_layouts`, `_sass`, `_includes`, `.git` | Jekyll convention; not expressible in `.gitignore` (these are tracked) |
| **`exclude`** | `docker/`, `scripts/`, `CHANGES/`, `TODO`, `*.sh`, `Gemfile`, `*.yml`, `*.toml` | Tracked *on purpose* but still not content — `.gitignore` must not list them |

Verified: all 10 build-artifact excludes were already in `.gitignore`, so that
half of the hand-written list was pure duplication and is now deleted. The
remaining entries are none of them gitignored — no overlap, nothing to rot.

### Where `.gitignore` actually earns its keep

Not the tree walk. Measured with `gitignore = false`, the row counts are
**identical** (327/228/838/1559) — because almost everything gitignored is
*also* caught by the dot/underscore skip (`_site*`, `_log*`, `_cache`,
`.jekyll-cache`, …). Only `vendor/` and `grackle/target` are excluded by
`.gitignore` alone, and both are empty of content.

It earns its keep in the **marker scan**, which structurally *cannot* use the
dot/underscore skip — markers are dotfiles living under `_posts` (§4b). That
walk has no other defence:

| | marker scan | total |
|---|---|---|
| `gitignore = false` | **205.4 ms** | **232.4 ms** — over the 200ms budget |
| `gitignore = true` | **5.9 ms** | **22.8 ms** |

A 35× difference, and the line between meeting the budget and blowing it. It
also retires the prune-glob workaround the marker scan previously needed
(rewriting `_site*/**` → `_site*` so walkdir would prune the subtree).

`.gitignore` is honoured via the `ignore` crate, with `git_global(false)` and
`parents(false)`: a contributor's personal global gitignore, or one above the
site root, must never change what the site publishes.

## 5. Views (the generators, declaratively)

Everything Jekyll plugins generated becomes a declared, incrementally
maintained view over a table:

```toml
[views.tag_index]
over     = "blog"
group_by = "tags"                       # one output row-group per tag value
route    = "/blog/tags/{key}/"
layout   = "tag_index"

[views.yearly_archive]                  # new with grackle: /blog/2010/ was a
over     = "blog"                       # 404 between /blog/ and /blog/2010/01/
group_by = "date.year"
route    = "/blog/{year}/"
layout   = "yearly_archive"

[views.monthly_archive]
over     = "yearly_archive"             # subdivision (§5c): GROUP BY year, month;
group_by = "date.month"                 # {year} comes from the parent's key
route    = "/blog/{year}/{month:02}/"
layout   = "monthly_archive"

[views.blog_index]
over     = "blog"
filter   = "!hidden && !draft"
paginate = 5
routes   = ["/blog/", "/blog/page/{n}/"]

[views.feed]
over     = "blog"
filter   = "!hidden"
limit    = 20
template = "atom.xml"                   # rendered as a bare liquid page

[views.sitemap]
over     = "*"                          # all routable rows
filter   = 'dir || ext == "html" || ext == "pdf"'
```

⚠️ **That filter was originally guessed as `exclude = "noindex || paginated>1"`,
and the reference build proves both halves wrong.** Measured over its 556 URLs:

- **`noindex` is ignored** — the tag pages are listed despite `tag_index.html`
  setting `noindex: true`.
- **paginated pages are kept** — all 64 `/blog/page/N/` are listed.
- Membership is 524 pretty dirs + 30 `.html` + 2 `.pdf`, and *nothing else*.

So jekyll-sitemap keys on **output type alone**. Images, `.css`, `.txt` and
`.xml` fall out by extension — which is also what keeps `/atom.xml` and
`/sitemap.xml` itself off the list, with no special case.

**`dir` is a distinct field from `ext == ""`.** A directory URL outputs an
`index.html`; an extensionless *file* (`/code/legacy/nnet/nnet`) outputs itself
and is not a page. Collapsing the two let four extensionless binaries into the
sitemap. Verified: 573 URLs = the reference's 556 + 17 rows that postdate that
build (2 posts, 1 archive, 1 pagination page, 12 tags, 1 PDF asset), 0 missing.

### Route fields

`kind` `view` `url` `ext` `key` (string); `dir` (bool); `page` `rows` (int).

**`noindex` is deliberately absent.** Computing it needs the layout chain
(phase 2), and a field we cannot populate correctly is worse than no field:
omitted, referencing it is a load-time error; present-but-wrong, it silently
lies. It also turns out not to be needed.

`over = "*"` views read the finished route set, so they run in a second pass.
Views iterate in name order, which meant `sitemap` originally ran before
`tag_index` and reported 1544 rows instead of 1559 — the row count was the
symptom that exposed it.

`filter`/`group_by`/`limit` are deliberately tiny — a predicate language over
row fields, not SQL. Anything fancier is a Rust `Generator` impl registered
under a name the config references. View outputs are routable rows like any
other, so they land in the same URL→row reverse index.

### The filter language

The grammar is, deliberately, a **CEL subset** — §5f pins that contract
(this language predates the decision and converged by accident):

```
expr    := or
or      := and ("||" and)*
and     := unary ("&&" unary)*
unary   := "!" unary | primary
primary := "(" expr ")" | "*" | field | field OP literal | string "in" field
OP      := "==" | "!=" | "<" | "<=" | ">" | ">="
```

A bare field is a **truthiness** test, which is what makes `!draft` read
naturally and gives `description` the useful meaning "has one": bool → itself,
string → non-empty, list → non-empty, int → non-zero, absent → false.

Post fields: `draft` `hidden` (bool); `title` `slug` `stem` `layout`
`description` `url` `date` (string); `year` `month` `day` `body_bytes` (int);
`tags` (list). `date` is ISO-8601, so string ordering *is* date ordering and
`date >= "2020-01-01"` works without a date type.

```toml
filter = '!draft && !hidden'
filter = 'year >= 2020 && "rust" in tags'
filter = '!(draft || hidden) && description'
```

**Expressions are parsed and type-checked once per view at load time**, against
a schema — not interpreted per row. This is the point, and it fixes a real
hazard in the first cut: that version split on `&&`, understood only `draft`
and `hidden`, and **returned true for anything it didn't recognise**, so
`filter = "!drafts"` silently matched every row. Now:

```
view blog_index: filter "!drafts"
  unknown field `drafts` (did you mean `draft`?)
    known fields: body_bytes, date, day, description, draft, hidden, layout, ...
```

Type errors are caught the same way, with the fix in the message:

| filter | error |
|---|---|
| `year == "2022"` | `` `year` is int, but it is compared to a string literal `` |
| `tags == "rust"` | `` `tags` is a list; use `"rust" in tags` instead of a comparison `` |
| `"x" in title` | `` `in` needs a list on the right, but `title` is string `` |
| `draft > 1` | `` `draft` is bool; ordering comparisons are not meaningful ``|

Verified against the corpus, each cross-checked against an independent count:
`tags` → 44 (matches the 44 posts with a `tags:` key), `description` → 7
(matches 7 `description:` keys), `"rust" in tags` → 5 (matches `query tags`),
`layout == "post"` → 327 (which also proves the §4 rule cascade is supplying
the catch-all's `layout` default to every row).

### Audited against `/code` and `/writing` (and the mindstorms gallery)

Walking the two big tree sections against this model, plus the assumption
that `demos/mindstorms` becomes a gallery view, mostly confirms the shape —
`code/graphics/raytracer/` is already a page bundle (index.md + sibling
screenshots + a zip + a sub-page), exactly §6a's measured case, and the
oddballs (front-matter-less `README.md`s, 1996-era `enel555.html`, the
extensionless `nnet` binary, download tarballs) all land correctly under
existing passthrough rules. Two things are worth stating, and three are
missing.

**Curated indexes are content, not views.** `code/index.md` is a
hand-authored, hand-ordered project list with `{% post_url %}` foreign keys
reaching across tables into posts. It must *stay* authored — a content-first
system keeps "a human chose this list" distinct from "a query derived it",
and the model already does: it is a `document`, and the FK lookup is already
vocabulary (§5d).

**The gallery is a restructure the tree already knows how to express.**
Today mindstorms is 451 zero-padded JPGs flat in one directory, with 17
hand-written HTML pages encoding disjoint ranges (`alpharex_2.html` owns
0074–0124) — the grouping exists *only* in the HTML. Positionally
restructured, the tree encodes it (`demos/mindstorms/alpharex/part-2/…`),
and the view wants to be:

```toml
[views.mindstorms]
over     = "objects"
match    = "demos/mindstorms/**"     # gap 2
group_by = "dir"
order_by = "name"                    # gap 3
variant  = "gallery"                 # §5e, open question 24
route    = "/demos/mindstorms/{key}/"
```

The three gaps, in order of generality — **all three built (2026-07)**
against the example site's gallery (§7a), which also killed the phase-1
gate: views now dispatch on the base collection's *kind*, never its name:

1. **Objects have no schema.** ✅ `object_schema()`:
   `path`/`dir`/`name`/`stem`/`ext`/`url` (str) + `size` (int), so
   `over = "objects"` filters type-check with the usual errors. Dimensions
   stayed out of the *filter* schema on purpose — they are render-time
   facts from the thumbnail pass (q26, also built: galleries emit
   `width`/`height` on every `<img>`), not load-time columns.
2. **View scoping needs `match`, not a bigger filter language.** ✅ A
   `match` glob on views, reusing rule globs; the filter language stays
   typed-fields-only.
3. **`order_by` does not exist.** ✅ Built and *required* for object views
   (`order_by = "name"` is the one value so far) — declared, not
   defaulted, exactly because the corpus's zero-padding making lexical
   order correct is luck.

Still open for the mindstorms case specifically: `group_by` over object
paths (one gallery route per directory) and the group `hero` (q23) —
the example's flat gallery didn't force them.

Two consequences ride along: group part maps need a **hero** (the existing
`alpharex_1.jpg` cover images argue "designated cover file beats first item"
— open question 23), and the restructure breaks URL parity for pages that
are, today, accidentally indexable (open question 28). Doing nothing also
works: with no front matter, the current pages are pure passthrough, so the
gallery is an opt-in restructure, one robot at a time. The
`page-break-inside: avoid` on every step page (these are building
instructions meant for printing) becomes a `@media print` block in theme
CSS, and the repeated inline `<style>` becomes the first real second use
case for §5b's `.style.scss` overlays.

## 5a. Presentation, from first principles

Four layers, each changing for its own reason and at its own rate:

| Layer | Owns | Changes when |
|---|---|---|
| **Schema** | what fields a row *has*, typed | the content model changes |
| **Rendering** | body markdown → semantic HTML fragment | an author writes |
| **Physical layout** | arranging rows + fields into `main` | the information architecture changes |
| **Visual theme** | the shell around `main` — chrome, `<head>`, CSS | the design changes |

A theme is **shell + CSS**, not CSS alone — the same thing Hugo and Jekyll
mean by it. `light` is the proof that this is the right cut: it differs from
`default` by having no nav, no footer *and* no stylesheet. Those aren't two
layers, they're one package.

Jekyll has no schema layer, and conflates the other three. This is not
theoretical — the conflation is measurable in this site.

### The diagnosis

**Six layouts and six includes implement about three concepts.**

*Three listings, three hand-written queries, three filters — and the filters
disagree* (evidence above): `monthly_archive` excludes `hidden or draft`,
while `tag_index` and `blog/index` exclude only `draft`. So hidden posts leak
into tag pages and the blog index but not archives. The query is *already*
declared once in config (§5, `[views.*]`) — the templates are re-deriving what
the database knows.

*Two document layouts*: `post.html` (article + temporal neighbours) and
`page.html` (breadcrumbs + article). Same shape — one row, full content —
differing only in which **relations** they show. And they carry **two
different breadcrumb implementations** (`page.html` emits
`<div class="breadcrumbs">`, `post-breadcrumbs.html` emits
`<nav class="breadcrumbs"><span class="breadcrumbs__part">`) that have drifted
apart.

*The shell knows about everything.* `default.html` branches on `multipost`,
`hide_sidebar`, `paginator`, `page.date` and `noindex`. That is the wrapping
model's fault, not the author's: when `{{ content }}` bubbles **upward**
through a layout chain, the outermost template ends up needing to know every
inner case.

### Layout kinds: there are three

Not "what this site has" — what a site of this shape *needs*:

| Kind | Input | Instances today |
|---|---|---|
| **document** | one row, full content + relations | `post.html`, `page.html` |
| **listing** | N rows, summarised | `tag_index`, `monthly_archive`, `blog/index` |
| **feed** | N rows, serialised | `atom.xml`, `sitemap.xml` |
| **raw** | one row, content *is* `main` | the 6 pages using `layout: default` |

`raw` is not a wart: `index.html` builds its own `<article class="page">` with
its own `<h1>` and a `blocks-50` grid. It wants the shell and nothing else.
That is a legitimate kind, and naming it stops it from being "the layout that
means no layout".

A **view** (§5) already supplies the query, the filter and the key. A layout
kind supplies the arrangement. `tag_index` and `monthly_archive` and
`blog_index` are then *the same layout* with different views — and their filter
disagreement cannot recur, because there is one filter, in one place, already
type-checked (§5).

`document` unifies `post`/`page` because the difference is **schema-driven, not
layout-driven**: a row with a `date` has temporal neighbours; a row in a tree
has ancestors. The layout asks the schema what relations exist; it does not
branch on "am I a post".

### Regions, not chains

Content stops bubbling upward. The **theme** owns a shell that declares
regions; a layout kind fills `main`:

```
theme (shell + css)
├── head     <- renders a subset of the computed head facts
├── header   <- site nav          (default: yes · light: no)
├── main     <- whatever the layout kind produced
└── footer   <- site chrome       (default: yes · light: no)
```

The shell never asks `{% if page.multipost %}` because it never needs to know.
That question only existed because `{{ content }}` bubbled *upward* and forced
the outermost template to know every inner case.

**`<head>` is computed, then selected.** The schema yields typed **head facts**
— `title`, `description`, `canonical`, `robots`, `og`, `jsonld` — and each
theme renders the subset it wants:

| | default | light |
|---|---|---|
| title | ✓ | ✓ |
| robots (`noindex`) | ✓ | ✓ |
| description, canonical, og, JSON-LD, favicons, analytics, CSS | ✓ | — |

A row with a `date` yields `og:type=article` + `BlogPosting`; one without
yields `website`. That's a *fact about the row*, not a branch in a template —
and it deletes all five of `default.html`'s if-chains. Note `light` already
branches on `noindex` today, so it needs the same facts, just fewer of them:
evidence that "compute facts, let the theme select" is the right shape rather
than a convenience.

### Two themes, because one is unfalsifiable

`default` (full chrome) and `light` (bare — used by exactly two legacy pages,
`demos/mindstorms/` and `writing/linuxwp/doc/`).

`light` is kept **deliberately, as the falsifier**. A layer boundary with a
single implementation is untestable: any leak from layout into theme goes
unnoticed because there is nothing to leak *against*. Two themes make the seam
load-bearing — every `document`, `listing` and `feed` must render under both,
so anything theme-specific that creeps into a layout kind fails immediately
and visibly. The two legacy pages that need bare chrome are the test suite.

**Theme is chosen per row** (unusual, but it is what this site does): `theme:
light` in front matter, defaulting to `default`. A layout hint in the §5a
taxonomy — schema read by the presentation layer.

**Layout kind is inferred, not declared.** It follows from what a row *is*:
a post or page → `document`; a view with `group_by`/`paginate` → `listing`; a
feed/sitemap view → `feed`; a row that opts out → `raw`. So today's `layout:`
front matter (37 `page`, 8 `post`, 6 `default`, 2 `light`) collapses into
"which theme" plus "did you opt out of the document wrapper" — and `page` vs
`post`, the most common distinction on the site, stops being a choice at all,
because the schema already knows which relations a row has.

### Schema drives rendering, not just display

Per-collection fields fall into three kinds, and the distinction *is* the
layer boundary:

| Kind | Read by | Example |
|---|---|---|
| **content field** | layout | `title`, `date`, `tags` |
| **render directive** | the renderer | `toc: true`, `math: true`, `style:` (§6c) |
| **layout hint** | the layout | `hide_sidebar`, `wide` |

```toml
[collections.blog.schema]
title = { type = "string", required = true }
date  = { type = "date",   required = true }
tags  = { type = "list" }
toc   = { type = "bool", default = false }   # a render directive
```

**This unifies with something already built.** `post_schema()` exists today for
filter type-checking (§5). Making it *the* schema means one declaration drives
filters, `<head>` generation, layout requirements, and validation — and a
layout can declare what it needs, so "layout `document` requires `date`, but
collection `pages` has no `date` field" becomes a load-time error like every
other constraint (§4).

### The theme boundary

A theme supplies the shell and the CSS; it consumes **stable semantic hooks**
emitted by the layout kinds. The existing BEM-ish naming
(`post-full__margin`, `multipost-listing__below-title`) is already a decent
contract; the layouts just need to treat those names as an interface rather
than an accident.

The boundary has a crisp test, and `light` is how we run it: **a layout kind
may never name a theme, and a theme may never know which layout kind filled
`main`.** `light` ships no CSS at all, so any layout kind that depends on
styling to be coherent is caught the moment it renders bare.

The renderer emits hooks too, and that is *not* a layering violation: `{% image
right foo.png %}` is the author saying "this floats right". The renderer emits
`class="image image--right"`; the theme decides what that means. The rule is
that a class is a **contract**, never a CSS implementation detail.

### What this costs: chrome parity

Redesigning layouts changes the chrome HTML, so `diff` cannot verify it.
That is affordable, and the boundary is already where it needs to be:

- **Body rendering stays at parity** — measured, 90.7% (§8a). `diff` compares
  *bodies only*; chrome was never in that measurement.
- **URL parity is untouched** — routes are §4, independent of presentation.
- **Chrome is small, ours, and eyeballable** — six templates, not 327 posts.

So: bodies verified by machine, chrome verified by looking at it.

```
row (stage 3)
  → liquid pass          (posts contain {% image %}, {% post_url %}, {{ site.baseurl }})
  → markdown → HTML      (comrak; .html rows skip this)         = stage 4, cached
  → layout chain         (post.html → default.html; layout front matter
                          merged up-chain like Jekyll's place_in_layouts)
  → bytes for URL
```

### Liquid surface to replicate

Objects: `site.*` (posts, tags, time, config keys), `page.*`, `layout.*`
(merged), `paginator.*`, `include.*`, `content`.
Custom filters: `date` (chrono-backed, `%-d` support), `date_to_xmlschema`,
`jsonify`, `cdata_escape`, `expand_urls`, `feed_images`, `visible`,
`titlecase` (registered so templates parse; site doesn't enable it).
Custom tags: `include` (Jekyll-style, unquoted + `key=value` params),
`post_url` (a foreign-key lookup into the posts table; missing → error),
`image` (below).

### Derived assets

`{% image [left|right|inline] ref %}` (194 uses / 68 posts) produces a
**derived-asset row**. All derived artifacts — thumbnails, compiled CSS,
embeddings — share one content-addressed cache; see §6b.

## 5b. Tree overlays: styles, slots and schema declared by position

> ✅ **The schema leg is built** (2026-07, `schema.rs`, forced by §7a's
> recipes and books): `.schema.toml` declares typed fields
> (string/int/bool/list/**image**) for its subtree, resolution accumulates
> nearest-wins like markers, and a governed row's extra front matter is
> *validated* — undeclared key or wrong type is a load error naming the
> file and the knowns, exactly the payoff promised below. Image-typed
> fields feed the thumb pass and the `hero` part (q23). Ungoverned rows
> stay as tolerant as ever. The `.style.scss` and `.slots/`-overlay legs
> remain as specced; per-row **themes** landed separately (a `theme:`
> field cascading via rule defaults — §5a's "theme is chosen per row",
> real at last, with a theme registry and per-theme stylesheets). Theme
> specs take a colon suffix for **subselection** (Matt, 2026-07):
> `theme: "recipes:spicy"` renders through `recipes` with the tokens
> space-joined into a `subtheme` shell part; the shell places it as an
> attribute hole on `<html>` (`data-slot-data-subtheme`) and CSS
> subselects via `[data-subtheme~="spicy"]` — rule 3 handles absence,
> the §5b data-scope token trick handles multiplicity, zero new engine
> machinery.

**This is the marker pattern (§4b) again**, which is the argument for it: the
tree declares *where*, the config declares only the vocabulary. A directory
already sets defaults for its subtree; the proposal is that it can also carry
**styles**, **slot fragments** and **schema**.

```
code/
  .style.scss              -> css, scoped to this subtree
  .slots/after-title.html  -> fills the document layout's after-title slot
  .schema.toml             -> github_link = { type = "url" }
  .noindex                 -> (existing marker)
  legacy/
    .style.scss            -> deeper, wins on conflict
    romtool/index.md       -> github_link: https://github.com/...
```

### Scoping is mandatory, not optional

Measured: `/blog/` page 1 draws from `_posts/2022/` *and* `_posts/2026/`;
`/blog/tags/rust/` likewise. **Listings mix subtrees.** An unscoped
`.style.scss` would bleed onto neighbouring posts in every listing — the same
latent bug §6c found with per-post `<style>` under `body.multipost`, but
guaranteed rather than latent.

So every rendered row carries its **scope chain**, and styles compile inside it:

```html
<article class="post" data-scope="code code/legacy">
```
```scss
// code/legacy/.style.scss
.gh { font-size: 0.8rem; }
```
```css
/* compiled */
[data-scope~="code/legacy"] .gh { font-size: 0.8rem; }
```

SCSS nesting does the scoping for free — the same trick §6c uses for per-post
`<style>`. `[data-scope~="…"]` matches whitespace-separated values, so a row
carries every ancestor scope at once and no name-mangling is needed
(`code/legacy` stays literal instead of becoming `s-code-legacy`, which would
collide with a real `code-legacy/` directory).

**Specificity**: an attribute selector is 0,1,0 — the same as a class — so
source order decides. Emit **outermost first**; deeper subtrees then win
naturally, matching "nearest wins" everywhere else.

### "Only for posts?" — no, but the tree means the *source* tree

Scope is the **source path** of any rendered row, uniformly for posts and
pages. That answers the question, but exposes a wrinkle:

**For posts the source tree is not the URL tree.** `_posts/2022/foo.md` lives
at `/blog/2022/12/16/foo/`, so `_posts/2022/.style.scss` styles "2022 posts" —
a nearly useless grouping. Nobody wants to style a year.

Source is still right, because the `.style.scss` must sit *next to* what it
styles — that is the whole positional idea. What it means is that per-subtree
styles only get interesting for posts once posts are **page bundles** (§6a):

```
_posts/2022/coffee-part-1/
  index.md
  .style.scss     <- styles exactly this post
  leak.jpeg       <- and its assets resolve as siblings (§6a)
```

At which point **§6c's per-post `<style>` and this become the same feature**,
with the bundle as the thing that unifies them. That is an argument for bundles
that §6a did not have.

### Slots: the layout declares, the tree fills

Layout kinds (§5a) expose named slots — finer-grained regions:

| `document` slot | default |
|---|---|
| `head_extra` | — |
| `before_title` | — |
| `after_title` | — |
| `margin` | breadcrumbs + tags |
| `after_content` | — |
| `relations` | prev/next |

```liquid
{# code/.slots/after-title.html #}
{% if page.github_link %}<a class="gh" href="{{ page.github_link }}">GitHub</a>{% endif %}
```

~~**This is where liquid finally earns its place.** The hand-rolled expander
(`tags.rs`) covers `{% image %}`/`{% post_url %}` because those are the only
constructs in *bodies*. A slot fragment needs conditionals — real templating.
§9a already chose the `liquid` crate; this is the use case that justifies it,
rather than page templates, of which exactly one survives (`/`).~~

**Superseded (§5e, binder built):** the conditional a slot fill needs *is*
rule 2 of the hole algebra — an empty part deletes its element. The example
above becomes `<a class="gh" data-slot-href="github_link">GitHub</a>`, no
templating anywhere: absent `github_link`, the attribute hole stays empty and
the element styles as a placeholder link, or the fill wraps it in an element
slotted on the field and the whole thing collapses. `.slots/` files are
binder fragments (`.html`) or markdown (`.md`) — see §5e "Tree-filled slots".
With this, **liquid's last claimed use case is gone**; §5d's retirement of
the crate is now total.

### Schema per subtree, and the payoff

`code/.schema.toml` adds fields for rows beneath it, accumulating down the tree
like markers:

```toml
github_link = { type = "url" }
```

The payoff: this is **the same schema that already type-checks filters** (§5).
One declaration then gives

- front matter validation (`github_lnk:` → error naming the file),
- filter type-checking (`'"x" in github_link'` → error: not a list),
- **slot template checking** — `{{ page.github_lnk }}` becomes a load-time
  error instead of rendering empty.

That last one is the real win: templates referencing fields is exactly where
typos hide, and it is the same hazard the filter language was built to kill.

### Where the CSS goes

One rule: **shared → the shared file, unique → inline.**

- Subtree `.style.scss` → appended to `main.css`, scoped. One request,
  immutable cache (§6b), correct on listings *because* it is scoped.
- Per-row `<style>` (§6c) → inline in `<head>`, scoped to that row.

### Constraints worth knowing before building this

1. **Scoped SCSS cannot declare `:root` custom properties** — they would be
   scoped to the selector and silently not apply. `@media` inside a scoped
   block is fine. This must be a documented constraint or a load-time error,
   because the failure is invisible.
2. **Every rendered row must emit `data-scope`**, summaries included — that is
   the case that makes scoping necessary at all.
3. **Order must be deterministic**: outermost-first, then lexical.
4. **Nothing on the site needs this yet.** Measured: no page under `code/` or
   `writing/` carries a field beyond `title`/`layout`, and `_sass` has no
   section-specific rules. Greenfield, with one motivating use case — though
   the mindstorms gallery (§5 audit) supplies the second: 17 pages repeating
   the same inline `<style>` is exactly a subtree `.style.scss`.

### The incremental path

With exactly one use case, a slot system is more machinery than the problem
needs. The honest ordering:

1. **`github_link` alone** → add it to the pages schema; `document` renders it
   when present. No slots, no overlays. ~10 lines.
2. **Scoped `.style.scss`** → the smallest overlay, and the one with no
   alternative: a section-wide look cannot live in front matter.
3. **Slots + per-subtree schema** → when a *second* subtree wants something
   *different*. That is when generality pays; before then it is a framework for
   one field.

Design it all now; build (1) and (2); let the third use case justify (3).

## 5c. A view is a query; a route is where it lands

§5 declared views as generators: each one had a `route`, and routes were the
only reason a view existed. The home page broke that, and the break was
load-bearing.

### What `/` actually is

`index.html` has two lines of its own content — an `<h1>` and one paragraph.
Everything else is three other things wearing a page costume:

| slot | filled by | kind |
|---|---|---|
| intro | authored prose | content |
| left | `{% include social.html %}` | site data, also used by the footer |
| right | latest 3 posts | a query |

Even the grid is not content: `.blocks-50 { display: grid; grid-template-columns:
1fr 1fr }` with two `.block-50` children is a **layout with two slots**,
hand-written into a content file because Jekyll gave it nowhere else to live.
`<h1>Connect</h1>` and `<h1>Latest Posts</h1>` are slot labels.

### The five-opinions problem

The reason to name a set is not to save a line of TOML. Five hand-written
`{% unless %}` clauses had drifted into three different answers to "what is a
post list?":

| view | template | excludes |
|---|---|---|
| blog_index | `blog/index.html` | draft |
| feed | `atom.xml` | **hidden only — the feed shipped drafts** |
| tag_index | `_layouts/tag_index.html` | draft |
| monthly_archive | `_layouts/monthly_archive.html` | hidden, draft |
| `/` | `index.html` | hidden, draft |

Nobody decided this; it accreted. Transcribing it faithfully into `grackle.toml`
also transcribed a bug: `monthly_archive` was written `!draft`, dropping the
`!hidden` its template actually had, and no diff could catch it because there was
nothing to catch it with. It is invisible today only because the corpus has 0
drafts and 0 hidden posts — the flags are pure potential energy.

So: **one named set**, and everything composes over it.

```toml
[views.published]          # query only: no route, no layout
over   = "blog"
filter = "!draft && !hidden"

[views.blog_index]  over = "published"  paginate = 5  routes = [...]
[views.latest]      over = "published"  limit = 3     layout = "link_list"
```

Fixing all five was provably free: build output stayed byte-identical, because
nothing is filtered today. It stops being free the first time a draft exists,
which is the point.

### Three shapes, one concept

| shape | route | layout | example |
|---|---|---|---|
| named query | — | — | `published` |
| embeddable | — | ✓ | `latest` |
| materialized | ✓ | ✓ | `blog_index` |

`route` is optional. `over` may name a collection, `*`, or **another view — but
only a query-only one.** That restriction is the whole reason composition stays
simple: allowing `over = "blog_index"` would raise "is `paginate = 5`
inherited?", and every answer surprises someone. Compose over things with
nothing to inherit. Cycles, unknown names, and composing over a materialized
view are all load-time errors naming the view.

### `self`, and the match it deletes

Each route carries `members`: the rows it materializes, decided once by the
declared query. Before it existed, `build.rs` re-derived them:

```rust
match view.as_str() {
    "tag_index" => { ...!p.draft && !p.hidden && p.tags.contains(key)... }
    "blog_index" => { let per = 5; ...!p.draft... }
}
```

That is the config declaring `filter`/`group_by`/`paginate` and the renderer
ignoring all of it — including hardcoding `per = 5` beside a `paginate = 5` it
never read. It is exactly how `blog_index` and its config could silently
disagree. Now the renderer iterates `members` and matches only on the *layout
kind* for titles: layout kinds are code, view names are the user's.

Routeless views have no route to hang `members` on, so their single row set
lives in `db.views` — which also makes named queries introspectable via
`export`, like every other table.

### Why compose over views, not routes

The tempting shape is `over = "/blog"`. It does not work:

* `/blog` is **66 routes**. "The posts from /blog" is ambiguous — page 1's five,
  or the whole set?
* Routes are *outputs*. Querying one means `/` depends on `/blog` having been
  materialized, inverting the dependency graph §2's incremental rebuild rests
  on. Views are pure functions of tables; routes are results. Keep the arrows
  pointing at tables.

### The embedding seam

```
grackle.toml   [views.latest] over="published" limit=3 layout="link_list"
  ↓ db.rs      routeless + ungrouped → one row set → db.views["latest"]
  ↓ tags.rs    {% view latest %} → look up rows, dispatch on layout
  ↓ render.rs  link_list(rows, site)
```

Nothing in `tags.rs` or `render.rs` knows what "latest" means; `render::link_list`
takes rows and a site and cannot reach the database, so it cannot grow a query.

Two deliberate refusals, both the same line drawn in §6d against exposing blocks
to templates:

* `{% include %}` **rejects parameters.** The layouts use `{% include
  article.html margin_html=... %}`; supporting that is step one of writing a
  template engine.
* `{% view %}` **dispatches to a layout kind** rather than handing rows to a
  template to iterate.

`{{ 'X' | prepend: site.baseurl }}` (12 uses) is recognised as a whole shape, not
as a filter pipeline. `{{ page.title | escape }}` still passes through verbatim —
an unimplemented construct must appear in the output, never evaluate to nothing.

### What it cost

`{% view %}` is not Liquid, so **Jekyll can no longer build `index.html`**
(`Unknown tag 'view'`), and `publish.sh` exits before its rsync. This is the
first piece of the site that has actually cut over. The consequence worth
remembering: **the reference build cannot be regenerated while this stands**, and
§8b exists because a stale reference lied to us by 17 points. To refresh it,
stash the change first.

### Grouping is one operation *(generalized 2026-07)*

"Isn't group_by just the same thing as tag?" (Matt) — yes, and the
question deleted two-thirds of the mechanism. `group_keys` had three
hardcoded specs (`tags`, `date.year`, `date.month`); they were one
operation — **group by a typed schema field**, read through the same
`filter::Row` access filters use — instantiated three times: a `List`
field multi-keys (one group per item), scalars single-key, `Null` means
absent from the partition (an undated row under a year grouping ≡ a
course-less recipe under a course grouping). The date specs survive as
aliases for the `year`/`month` fields the filter schema always had.
Proven the strong way: the main site's three groupings are
**byte-identical through the general path**. Every grouping now exposes
`{key}` plus a param named after the field; group chains are load-checked
against the base schema (the `order_by` discipline applied to grouping);
and grouped views work over any base — `group_by = "course"` on the
example's recipes materializes `/courses/{key}/` with the same machinery,
subdivision chains included. Residue, kept knowingly: `month_name` is a
display derivative special-cased on the `month` field until §5f
formatters give it a home.

### Subdivision: `over` a grouped view refines its partition *(built 2026-07)*

A grouped view is a partition of its base; a grouped view **`over` a grouped
view is a finer partition of the parent's groups** — GROUP BY year, month,
expressed compositionally:

```toml
[views.yearly_archive]
over     = "published"
group_by = "date.year"
route    = "/blog/{year}/"
title    = "{year}"

[views.monthly_archive]
over     = "yearly_archive"          # subdivision: year key comes from here
group_by = "date.month"
route    = "/blog/{year}/{month:02}/"
title    = "{year} {month_name}"
```

Three consequences, none of them new machinery:

1. **Group keys accumulate down the chain.** A month route carries
   `year`/`month`/`month_name` params — the parent's key plus its own — and
   the route template draws from all of them. Composite membership is
   provably identical to flat `date.year_month` grouping (a partition of a
   partition), which is how this landed under the byte-diff oracle.
2. **Provenance is structural, not declared.** The month group (2022, 12)
   has the year group (2022) as its parent *because that is how the query
   nests* — no parent pointer, no second mechanism. The chain roots at the
   **collection**, which carries its own crumb and index URL, so a
   breadcrumb trail is a provenance walk: collection (Blog, `/blog/`) →
   year (2022) → month (December) → row (16). This is what retires the
   hardcoded `"Blog"`/`"/blog"` strings in the crumb producers — trails
   become derived data. (Trail *content* changes — the year becoming a
   clickable crumb — are chrome, gated to the §5e step-3 by-eye window; the
   machinery already reproduces today's bytes.)
3. **Naming is config, not code.** Views declare `title` and `crumb`
   (defaulting to `title`) as templates over their group params — the same
   placeholder language as routes, failing loudly on unknown tokens. The
   `match layout { "tag_index" => format!("Posts Tagged …") … }` that
   re-derived titles in the renderer is gone; `"Posts Tagged “{key}”"` lives
   next to the query it names.

Composition rules, enforced at load: `over` may name a query-only view
(unchanged) or a **grouped, unpaginated** view — and the composer must then
be grouped itself, because subdivision is the only defined meaning; a
non-grouped view over a grouped one is an error. **Pagination × subdivision
is deliberately punted** (open question 30): a year *could* paginate while
months subdivide off the year's root, but `/blog/2022/page/2/` and child
routes then share the year root's URL namespace, and that conflict deserves
real thought rather than a rule chosen in passing.

## 5d. Templating: there is almost none, so don't build for it

The recurring question — a real template language, or §5b's slots, or
hardcoded Rust layouts — is a false trichotomy. It dissolves once you count
what the site's templates actually contain.

### Every liquid construct in every template, classified

~60 constructs across `_layouts/`, `_includes/`, `blog/index.html`, `atom.xml`
and `index.html`:

| looks like | count | what it **is** | where it belongs |
|---|---|---|---|
| `for post in site.posts` + `unless post.draft`, `if page.next`, `for related limit: 4` | **17** | a **query** | view config: `filter`/`limit`/`group_by` (§5, §5c) |
| `if page.date`, `if seo_description`, `if noindex`, `if multipost`, `if hide_sidebar`, `if summary`, `if is_page`, `if site.google_analytics` | **22** | a **schema fact** | a typed field (§5a: "a fact, not a branch") |
| `assign post = page`, `assign content = post.content`, `capture margin_html`, `capture listing_title` | **12** | **argument passing** | a function call |
| `for p in page.ancestors`, `for tag in post.tags`, `for page in (1..total_pages)` | **8** | real display iteration | **3 components** |

**Only three constructs are genuinely "loop over a list and emit markup"** —
breadcrumbs, tag pills, pagination nav. All three are already Rust functions in
`render.rs`. The other ~57 are queries, schema facts, and Liquid plumbing
wearing templating's clothes. `assign post = page` exists solely because
`article.html` wants its variable called `post`; `capture margin_html` exists
because Liquid has no parameters. Twelve constructs vanish the instant you have
function arguments.

The site does not have templating. It has a database and four presentation
layers, and Liquid was the only vocabulary available to say so.

### The rule

> **A template may not contain control flow.**
> Needs a loop → it is a view. Needs a conditional → it is a schema fact, or a
> different layout kind.

This is a **tripwire**, not an aesthetic. Every `{% if %}` you want is a missing
schema field; every `{% for %}` is an unnamed query. The table above is the
evidence: it holds for ~57 of 60, and the 3 exceptions are components.

It also preserves the discipline the rest of the design already has. `filter.rs`
is a *typed* expression language with load-time checking and "did you mean"
suggestions. A template language throws that away — untyped, runtime-resolved,
`{{ post.titel }}` silently rendering nothing. The ethos here is load-time
errors, not 404s; Liquid is the opposite by construction.

`/` is the existence proof, and it was the hardest page on the site:

```html
<section class="block-50"><h1>Connect</h1>{% include social.html %}</section>
<section class="block-50 latest-posts"><h1>Latest Posts</h1>{% view latest %}</section>
```

HTML, typed holes, **zero control flow** — matching the reference exactly. The
nine-line counter loop became `filter` + `limit`.

### This retires the `liquid` crate

§9a listed `liquid` 0.26 as **the biggest dependency risk** — stale, wrong
dialect, needing us to reimplement Jekyll's tags and filters on top. Under this
rule we never need it. The whole vocabulary is:

| construct | uses | note |
|---|---|---|
| `{% image %}` | 194 | §6a |
| `{% post_url %}` | 51 | foreign key into `by_name` |
| `{{ site.baseurl }}` + `{{ 'x' \| prepend: site.baseurl }}` | 12 | whole shapes, not a filter pipeline |
| `{% view %}` | 1 | §5c |
| `{% include %}` | 1 | parameterless only |

Anything unrecognised is emitted **verbatim**, so an unimplemented construct
appears in the output rather than evaluating to nothing.

### Custom widgets: named HTML expansions with a markdown body *(built 2026-07)*

**Built as specced**: a `[widgets]` registry in `grackle.toml` (`name →
wrapper template` with a `{body}` hole, validated at load — a template with
no hole is a config error), paired-tag expansion in `tags.rs` (the body is
expanded in its own right, so `{% image %}` and `{{ site.baseurl }}` work
inside a callout; a registered widget with no end tag errors naming the
file; unregistered paired tags stay verbatim). Both 2026 posts are
rewritten to `{% callout %}` — all three raw-HTML shapes collapsed to the
one form — and the `markdown="1"` kramdown idiom is out of the source
entirely. 9 callouts render boxed; the fixture is retired.

It should be **easy to add a custom block widget** — `{% callout %} … {% endcallout %}`
— that translates to a fixed HTML wrapper with the author's markdown inside. This
is not a new capability so much as the block-level sibling of `{% image %}`: a
named expansion, not control flow, so it stays inside the §5d rule and needs no
template engine.

The concrete motivation is a bug this design already hit. The callout boxes on
the 2026 posts are authored as raw HTML:

```html
<callout><div markdown="1">
**Disclosures**
...
</div></callout>
```

That `markdown="1"` is a **kramdown** feature (§8) — "parse my inner content as
markdown, then drop the attribute." comrak has no such concept: it does not even
recognise `<callout><div …>` as a block, so the `<div>` opens *inside* a
paragraph and the box collapses. Today the fix is to hand-normalise the source
into a form both parsers accept (split the wrapper tags onto their own lines,
blank-pad the body, keep `markdown="1"` for kramdown) — which works, but pushes a
formatting rule onto the author and leaves a Jekyll-ism in every post.

A widget dissolves the problem instead of patching it:

```markdown
{% callout %}
**Disclosures**

... ordinary markdown, parsed by comrak like any other block ...
{% endcallout %}
```

expands, before markdown, to the wrapper the theme styles:

```html
<callout>
<div>
{{ body }}
</div>
</callout>
```

The body is spliced in with blank lines around it, so comrak parses it as
markdown with no `markdown="1"` needed and no lazy-continuation trap — the raw
HTML and the kramdown dependency both leave the source entirely. The author
writes `{% callout %}`; the fragility is gone.

The shape this wants:

- A **registry** of `name → wrapper template` (a fragment with one `body` hole).
  Adding a widget is one entry, no code — the same "data, not code" move themes
  make in §5e.
- Recognised by the expander (`tags.rs`) as a **paired** tag
  (`{% name %}…{% endname %}`), the one structural addition over today's
  self-closing tags. Still no arguments, still no control flow (§5c's refusals
  stand); an argumentful or conditional widget is the tripwire that says "you
  want a template — you don't."
- **Load-time checked** like everything else: an unknown widget name is an error
  (or emitted verbatim, per the existing rule), never a silent empty expansion.
- Composable with §6d: a widget is just another producer of an `HtmlBlock`, so
  block-splitting and rewrites see through it unchanged.

Open question 29 tracks it. It is small, and it retires the last raw-HTML +
kramdown idiom on the site.

### Slots already exist; we never named them

```rust
pub fn listing(rows, title, breadcrumb_tail, site, pagination: Option<&str>) -> String
```

`breadcrumb_tail` and `pagination` **are slots** — optional injected fragments
the layout places but does not build. §5b's slot machinery is not a thing to
design; it is a thing to notice we have. And §5b's own conclusion already
agreed: `github_link` should be "a pages schema field the layout renders when
present. No slots, no overlays. ~10 lines."

**So §5b's slot system may never need building.** Its incremental path gated
slots on "a second subtree wanting something *different*" — under this rule,
that case resolves to a schema field too.

### The two honest weaknesses

1. **Themes are Rust.** A theme is shell + CSS, and the shell is code, so a
   third theme means recompiling. Fine today (there are two, and `light` exists
   only as a falsifier, §5a) — but the shell is the one artifact with a real
   claim to being a template, and it is also the one place a presence
   conditional (`{% if description %}`) is genuinely hard to model away. That
   is *why* `<head>` is computed from `Head` facts instead of templated. Watch
   whether that stays comfortable; it is the first thing this rule would break
   on.
2. ~~The pagination slot is empty.~~ — **filled** (`render::pagination`). The
   rule held: it was the best stress test of "component, not template" — the one
   case with a genuine range loop and a three-way conditional
   (`if page == current` / `elsif page == 1` / `else`) — and it fell out as ~40
   lines of Rust, semantically identical to the reference nav (whitespace-
   normalized diff clean). Page 1 links `/blog/`, page N>1 links `/blog/page/N`
   (no trailing slash), faithful to jekyll-paginate. So the two honest
   weaknesses are down to one: themes-are-Rust.

## 5e. The presentation synthesis: parts fill slots, CSS does the geometry

**Status: all four steps built — the synthesis is real.** Layout kinds now emit
part maps (`parts.rs`): named, typed parts — `Text`/`Html`/`Stream`/`Map`/
`Flag` — in canonical order, names asserted against a per-kind `schema()`,
producers never touching `Site` (URLs are root-relative; `baseurl` is
presentation). A **legacy composer** (`legacy.rs`, since deleted with step 3)
replayed the pre-§5e BEM markup from the maps, verified **byte-identical**
across the whole site. Two findings from the extraction: (a) all three crumb
markup shapes turned out to be *one uniform loop over `{label, url?}` crumbs*
— the drift was only ever in the composer, exactly as predicted; (b)
`body_class()` was already dead code (the violation lives as a hardcoded
string at the listing call site). The step records below are the build log,
with §5a–§5d as the fossil record.

**Step 2 built (the binder).** `binder.rs`: strict fragment parser + the hole
algebra (now four rules — see below; attribute holes were the one genuine
addition the build forced) + complete load-time validation, ~450 lines
including its 12 tests, standalone until the theme directory wires it in.
The part schemas gained types (`PartType`: text/html/stream-of-kind/
map-of-kind/flag) so holes are type-checked, not just name-checked, and
producers assert their own conformance at `set()`.

**Step 3 built (the chrome cut).** `themes/default/` exists: `shell.html` +
ten kind fragments + `theme.scss`, rendered through the binder
(`theme.rs`); the legacy composer is deleted; `_sass` is superseded by the
theme's stylesheet (content-level partials copied verbatim — body markup
did not change; chrome partials rewritten against `data-slot`/`data-kind`).
Verified exactly as priced: **bodies by machine** (all 327 post content
regions byte-identical across the cut), **chrome by eye** (browser pass:
posts, listings, pagination, tree pages, `/`, phone widths, both color
schemes). What the cut retired, each named in this section's autopsy list:
the two breadcrumb markup shapes (one `crumb` fragment now), the two
document shapes (one fragment, `[data-tree]` is two CSS declarations),
`body.multipost` (summary styles select on `[data-kind="summary"]`
context), and the Rust default shell. Notes from the build:

- **Dark mode landed as pure CSS, then was deliberately removed (2026-07)** —
  a custom-property palette plus one `prefers-color-scheme` block in
  `_chrome.scss`, zero engine involvement: the proof §5e promised that CSS is
  doing the lifting. It proved the mechanism and was then backed out to
  unconditionally light, because the *content* assumes a white background in
  a lot of places (screenshots, diagrams, legacy pages). The palette vars
  stay; a dark value set is one block away once the content can take it.
- **The placeholder-link rule earns its keep everywhere at once**:
  disabled pagination arrows, the current page tile, and inert crumb tails
  are all `<a>` without `href`, styled via `a:not([href])`. `aria-current`
  rides an attribute hole (`data-slot-aria-current` filled only on the
  current page), so the CSS gap picker and a11y share one part.
- **Identity slots are live** (`.slots/nav.md`, `.slots/copyright.md` at
  the root): the copyright is a single-paragraph fill *unwrapped by the
  block-arity rule* into the shell's `<p>`; the nav is a markdown list
  filling a flow `<nav>`. No theme file contains the site's words.
- **Trails are provenance walks now**: posts render Home > Blog > 2022 >
  December > 16 with every archive level linked (the §5c payoff, one
  config edit: `crumb = "{month_name}"` + collection `crumb`/`index`/
  `trail`). The `Site.baseurl` parameter fell out of the presentation
  layer entirely — parts are root-relative and fragments are literal.
- Cascade layering is import-order + scoping rather than `@layer` for now:
  content styles scope to the content *region* (`.doc-body`), so chrome
  never fights body typography. `@layer` remains attractive once `grass`'s
  handling is verified; nothing structural blocks it.

**Step 4 built (the null theme) — and it dissolved into a fallback rule.**
`parts::canonical()` renders any part map with no fragments at all: kind
root = `<section data-kind>` stamped with facts, scalars = `<span
data-slot>`, urls = real links, streams/maps recurse, canonical order
throughout. The mechanism is better than the design asked for:
**`Fragments::render` falls back to canonical for any kind the theme
declines to arrange**, so themes are *partial by construction* — a theme
with no fragments IS the null theme and needs no directory, and a new theme
can start from one fragment and grow. Two refinements fell out:

- **`PartType::Url`.** The null theme should be navigable, which forced the
  admission that url-shaped scalars are a *type*, not a naming convention.
  Attribute holes (`data-slot-href`) now validate against Text-or-Url, and
  canonical renders urls as links.
- **The falsifier runs on every real row, in the test suite.** The
  completeness property — every part's bytes survive into the canonical
  rendering; if a part can vanish, no fragment can put it back — is checked
  over the actual corpus (327 posts, 180 listings incl. pagination maps,
  every tree-page shape) on every `cargo test`.

`layout: light` pages (2) render as minimal shell + canonical `raw` — the
Rust `light_shell` is now three lines of head around the null rendering.
Incidentally proven the same day: a one-line edit to `.slots/copyright.md`
moved the copyright year across 500+ pages with no theme file touched —
identity slots doing exactly what they were built for.

### One law, already proven twice

The design's best move keeps recurring without being named: **compute typed
facts, let the presentation select.** `<head>` is computed facts a theme
renders a subset of (§5a). `/` is HTML with typed holes and zero control flow
(§5d). Both times, the same shape killed a pile of conditionals and made a new
class of error load-time-checkable.

Everything between the layout layer and the browser should be that shape.
Currently it isn't, and the seams are visible in `render.rs`:

1. **The shell knows which layout kind filled `main`.** §5a's crisp test —
   "a theme may never know which layout kind filled `main`" — is violated by
   `body_class(kind, multipost)`: the shell is told "multipost" and stamps
   `class="multipost"` on `<body>` so the theme's CSS can branch. That is
   `{% if page.multipost %}` rewritten in Rust, which is exactly what §5a said
   the regions model would delete.
2. **The breadcrumb drift got ported, not fixed.** §5a diagnosed two divergent
   breadcrumb implementations in Jekyll (`<div class="breadcrumbs">` vs
   `<nav class="breadcrumbs"><span class="breadcrumbs__part">`). `render.rs`
   now contains both: `margin()` emits the `nav`/`span` form, `document_page()`
   emits the `div` form. Faithful porting preserved the disease — and its cost
   surfaced: the `div` form was **entirely unstyled** (the breadcrumb CSS is
   scoped to `.post-full__margin .breadcrumbs`, which the page form is not
   inside), so on pages it rendered at full body size with no spacing and
   collided with the `display:inline` title on narrow widths. Patched with a
   `.content > .breadcrumbs` rule (size + gap) — but that is a *second* style
   block for a *second* markup shape, exactly the drift this section is about.
   §5e's unification (one `crumbs` part, one theme fragment) retires both.
3. **Layout kinds emit arrangement scaffolding.** `document()` wraps content in
   `post-full` → `post-full__main` → `post-full__below-title` — divs that exist
   only so the theme's grid has something to grab. Arrangement lives in HTML
   structure, where it belongs to the theme, not the layout.
4. **`document` has two shapes** (§8b), because the theme's grid imposed
   structure back onto the layout layer. The tension was recorded honestly;
   this section is what resolves it.
5. **Themes are Rust** (§5d weakness 1, open question 20). A third theme means
   recompiling. "Easily theme-swappable" is the one property the current cut
   cannot deliver, and it is the property the whole presentation layer exists
   to provide.

### Slots exist in five places; none of them is called that

| where | what it is |
|---|---|
| §5a regions | slots on the shell (`head`/`header`/`main`/`footer`) |
| §5b `.slots/after-title.html` | slots filled positionally by the tree |
| §5d `listing(.., breadcrumb_tail, pagination)` | slots as `Option<&str>` fn params |
| §6d note placement (sidenote/endnote) | a stream choosing a slot |
| `{% include %}` | a slot filled from a file |

Five mechanisms, one concept. The synthesis: **a slot is a named, typed hole.
Layout kinds produce fills; themes produce placement; nothing else exists.**

### The model

```
db row / view rows                             (§3, §5, §5c)
  → doc model: blocks + notes + facts          (§6d — unchanged)
  → layout kind: a PART MAP, not a page        (new)
  → theme: fragments with slot holes + CSS     (new — themes stop being Rust)
```

**A layout kind emits a part map** — named, typed parts, each a flat piece of
semantic HTML or a typed scalar. No arrangement wrappers. For `document`:

| part | type | source |
|---|---|---|
| `title` | text | schema |
| `url` | text | route |
| `crumbs` | fragment | tree ancestors *or* date trail — schema-driven (§5a) |
| `date` | fact | schema |
| `tags` | stream | schema |
| `content` | stream of blocks | §6d |
| `notes` | stream of notes | §6d |
| `neighbors` | stream | adjacency index |
| `truncated` | fact | build-time cut (§6d) |

`listing` = `title`, `crumbs`, `items` (a stream of `summary` part maps),
`pagination`. `feed` bypasses themes entirely — serializations have no look.
`raw`'s content *is* `main`, unchanged.

**A theme is a directory of data, not code:**

```
themes/default/
  theme.toml        # which head facts to render (§5a, unchanged mechanism)
  shell.html        # the outer skeleton: holes for header/main/footer
  document.html     # optional: per-kind arrangement fragments
  summary.html      # optional: how one listing item is arranged
  theme.scss
themes/light/
  theme.toml        # title + robots, nothing else
                    # no fragments, no css: the null theme
```

A fragment is straight-line HTML with holes, and the whole hole algebra is
four rules (built: `binder.rs`):

1. **A hole is `data-slot="name"`.** The element's content is replaced by the
   part. Scalar parts are escaped text; fragment parts are trusted HTML.
2. **An empty part deletes its element.** This one rule replaces every
   presence-conditional — the case §5d called "genuinely hard to model away"
   in the shell. `<footer data-slot="footer">` with nothing to say does not
   render a footer. No `{% if %}` exists because nothing needs one.
3. **A stream maps a fragment over its items.** `<div data-slot="items">`
   renders the fragment of the items' kind once per row — the child kind
   comes from the part schema, so `data-fragment="…"` is an *override* (the
   variant hook, open question 24), not a requirement. The loop lives in the
   engine; the fragment stays straight-line. This is how the no-control-flow
   rule (§5d) scales past one level of nesting.
4. **An attribute hole is `data-slot-attr="name"`** — `<a data-slot-href=
   "url">` sets `href` from a text part, escaped; an absent part omits the
   attribute wholesale. The payoff is that HTML's own semantics absorb the
   variants the old markup branched on: `<a>` with no `href` is the spec's
   *placeholder link*, so "linked crumb vs inert tail" and "page number vs
   current page" are one fragment plus `a:not([href])` in theme CSS — the
   pagination component's three-way conditional dies at the platform level,
   same move as `:has()` killing `body.multipost`.

Implementation notes, measured against the built binder: the parser is
deliberately strict (well-formed nesting, double-quoted attributes, raw-text
`<script>`/`<style>`, comments/doctype verbatim) — a malformed fragment is a
build error with file:line, not something to recover from. The emitted markup
keeps `data-slot` (it *is* the CSS contract) and strips the authoring-only
`data-fragment`/`data-slot-*`; the root element of every rendered fragment is
stamped `data-kind` plus `data-<fact>` per true flag. All checks run at load
— unknown slot, fact-as-content, content slot on a void element, scalar with
`data-fragment`, attr hole naming a non-text part, stream slot whose child
fragment is missing — each error naming the file, the line, and the known
names. After load, rendering is infallible.

**Every name is load-time checked** against the part schema of the kind the
fragment is bound to — unknown slot, unfilled required slot, unknown fragment:
errors naming the file, with the known-names list, exactly like the filter
language (§5). `{{ post.titel }}`-class bugs die the same death twice.

### CSS does the geometry

Layout kinds emit parts in **canonical semantic order** — reading order, the
order a screen reader or the null theme sees. Themes never reorder markup;
they place it with `grid-template-areas` keyed on slot names:

```css
/* default theme: full post = margin column + content */
[data-kind="document"] {
  display: grid;
  grid-template-areas: "crumbs content" "tags content" ". neighbors";
}
/* pages: one column, crumbs above — same markup, different declaration */
[data-kind="document"][data-tree] { grid-template-areas: "crumbs" "content"; }
```

The §8b two-shapes tension dissolves: one `document` kind, one markup, and
"post vs page" is two grid declarations in a file the theme owns. `body.multipost`
dies: the summary styles itself via `[data-slot="items"]` context, and the
shell is never told anything.

**The styling contract changes from classes to structure.** Emitted markup
carries semantic elements + `data-slot` + schema facts as data attributes
(`data-kind`, `data-tree`, `data-truncated`, `data-scope` §5b). The BEM names
were "already a decent contract" (§5a) only in the sense that they existed;
they are an inherited accident, and two of them have already drifted (evidence
above). Slot names are better: **the same name appears in the part schema, the
theme fragment, the CSS selector, and the tree overlay filename.** One
vocabulary, checkable end to end. Renderer hook classes (`image--right`)
survive — those are author intent (§5a), a different thing from chrome.

### The modern CSS baseline

The theme contract assumes modern CSS — nesting, `:has()`, container queries,
`@layer`, subgrid, `aspect-ratio` (all Baseline as of ~2023). This is a
declared floor, not an optimization: each feature retires a piece of machinery
the contract would otherwise have to carry.

| feature | what it retires |
|---|---|
| `@layer` | specificity management by convention. Cascade order is declared once — `reset, base, theme, overlay, post` — giving §5b's subtree styles and §6c's per-post styles a principled slot instead of §5b's source-order gymnastics ("emit outermost first") |
| nesting | BEM's structure-flattened-into-strings. Theme CSS mirrors the fragment's DOM shape under a `[data-kind]`/`[data-slot]` root; an attribute selector is 0,1,0, same as a class |
| container queries | context classes. A `summary` fragment styles itself against *its container's* width — the same fragment works in a wide listing, a card grid, or a narrow embed with the engine stamping nothing |
| `:has()` | every upward-stamped helper class. `article:has(> [data-slot="notes"])` widens the grid for sidenote posts; `body_class()`/`multipost` die at the platform level, which is what finally satisfies §5a's "the shell is never told anything" |
| `aspect-ratio` + dimension facts | client-side measurement and layout shift (see the archetype test below) |

BEM's three historical justifications — specificity wars, no scoping,
decoupled flat selectors — are each answered by the platform now. So
`post-full__below-title` is not merely misplaced (a theme's geometry decision,
named by the layout layer, encoded in a string); it is obsolete as a
*category*. Role names in the contract, structure in the selector tree.

Two consequences:

- **Theme CSS is checkable.** Fragments are parsed at load (§5e), so the
  engine can verify that every `[data-slot=…]` selector in a theme's CSS names
  a real slot of the kind the fragment binds — the filter-language discipline
  (§5) extended into the stylesheet.
- **§6c's compile step loses one leg, keeps two.** Flattening nesting for old
  browsers was one of three jobs; under this baseline it drops. Validation
  (syntax errors as build errors) and auto-scoping remain, and both still pay.

### The archetype test: any layout is theme CSS plus a fragment choice

The model's bet is that *all geometry* lives in theme CSS, so "can it do
layout X" decomposes into "can modern CSS express X" (a browser question) and
"does the part schema carry what X's CSS needs" (the engine's only
obligation). Auditing the archetypes:

| archetype | fragment side | CSS side | engine gap |
|---|---|---|---|
| document, margin or sidenotes | canonical | grid areas, `:has()` | — |
| album gallery (Finder-ish) | theme maps `items` to a `card` fragment | `repeat(auto-fill, minmax())`, `aspect-ratio`, `object-fit` | **hero part** |
| Pinterest masonry | `card` fragment | see below | ~~dimension facts~~ ✅ **built** (2026-07): the `gallery`/`figure` kinds + object views; figures carry `width`/`height` from the thumb pass, and the example site runs CSS-columns masonry on them (§7a) |
| magazine / full-bleed | canonical + per-block hints | named-grid-lines full-bleed pattern | **per-block facts** |
| timeline / film-strip | `items` → small fragment | grid, `scroll-snap` | — |
| dense index / table | `items` → row fragment | plain grid | — |

The audit surfaces **four genuine gaps, and each resolves to "add a part or
fact" — never to control flow.** That is the model behaving as designed: the
gallery archetype didn't demand a template feature, it demanded a schema
field.

1. **A `hero` part on summaries** (→ open question 23). A card grid needs an
   image per item. Source: front-matter `image:` or the first image block,
   thumbnailed through §6b.
2. **Per-view fragment variants** (→ open question 24). `/photos` wants cards
   while `/blog` wants summaries, and both are `listing`. The view declares
   `variant = "gallery"`; the engine resolves `listing--gallery.html` with
   fallback to `listing.html`, load-time checked. Routes already carry the
   view name, so `data-view` comes free for CSS.
3. **Per-block facts** (→ open question 25). Full-bleed needs one block to
   escape the content column: a block-level directive → `data-` attribute on
   that block → the theme spans it. Slots straight into §6d's block stream.
4. **Dimension facts on images** (→ open question 26). Emit
   `aspect-ratio`/width/height on every image — grackle already knows them,
   because it makes the thumbnails (§6b). A static generator's structural
   advantage over client-side layout, and it kills layout shift site-wide.

**The one honest limit is masonry.** True Pinterest packing with strict
reading order is the single archetype CSS cannot fully express yet — native
masonry is still settling in the working group (the `display: masonry` vs.
grid-integration debate) and is not Baseline. Interim: CSS `columns` (reading
order runs down columns) or row-span tricks fed by the dimension facts above.
When native masonry lands, it is one declaration in one theme file, zero
engine work — the engine's only job was to have shipped the facts.

The meta-point, worth stating as the completeness criterion: **§5e turns "can
we do layout X" from an engine question into a browser question, and the
engine's obligation becomes crisp — every part or fact a plausible theme
could need must be in the schema.** The four gaps are the current delta.

### What this buys, concretely

- **Sidenotes become a theme decision** (open question 18, dissolved). The
  `notes` stream exists (§6d); a theme that wants Tufte margins declares a
  grid column and places `data-slot="notes"` beside content; a theme that
  doesn't gets the canonical fallback — an endnote section after content.
  Same markup, both themes, no layout change.
- **The ★ gets its vocabulary** (open question 17): `data-truncated` on the
  summary, star gated in theme CSS. The fix §6d wanted, expressible now.
- **Dark mode is a theme concern at last** (§8b found none exists): a
  `prefers-color-scheme` block in `theme.scss`, zero engine involvement —
  the proof that CSS is actually doing the lifting. (Proven, then removed:
  see the step-3 notes — the content isn't dark-safe yet.)
- **A third theme is a directory.** Copy `themes/default/`, edit HTML and
  SCSS, done. Open question 20 dissolves: no Rust, no recompile, and the
  engine's load-time checks tell you every hole you got wrong.
- **`light` upgrades from falsifier to null theme.** No fragments, no CSS
  means the canonical part order must be semantically complete markup on its
  own — a stronger test than "renders under two themes", run automatically on
  every row.
- **Includes are subsumed.** An include is a fragment with no holes filling a
  slot (`social` fills a shell slot in the default theme and a `/` slot).
  The parameterless refusal (§5c) stands; parameters are what part maps are.

### The precedence law, stated once

The same resolution order already governs rules (§4), markers (§4b), and
buckets (§6a). Slot fills join it:

> **Nearest wins; first writer per key.**
> front matter > tree overlay (`.slots/`, §5b) > layout kind > theme default.

### Tree-filled slots: `.slots/` is a table *(settled 2026-07)*

The precedence law's second clause, made concrete. A directory may carry a
`.slots/` subdirectory; each file in it fills one slot for every row beneath —
**filename = slot name = key, content = fill**. It is a table in the §3 sense:
fills are rows (versioned, watched by serve, queryable), and resolution is
positional — nearest `.slots/<name>.*` up the *source* path wins, the same
ascent §6a uses for asset names and §4b uses for markers. Third user of one
algorithm.

**The motivating case is the shell.** The current shell hardcodes the section
nav, and "© 1998-2026 Matt Mastracci — contact" — *content living in
presentation*. Moving it into `shell.html` per theme would just fork it per
theme (the copyright year drifts between copies). Instead the shell gets a
part schema (`nav`, `copyright`, …) and the site root carries the content:

```
.slots/
  copyright.md      # © 1998-2026 [Matt Mastracci](…) — [contact](/contact/)
  nav.md            # the section list
```

Every theme places `<p data-slot="copyright">`; none of them owns the words.
A second theme inherits the site's identity instead of copying it.
Per-directory *config* stays where §5b put it (`.schema.toml` — "the config
declares only the vocabulary"): TOML never carries prose.

**Extension picks the pipeline.**

- `.md` → tags + comrak, becomes an `Html` part.
- `.html` → a binder fragment: holes allowed, validated at load against the
  schemas like any theme fragment. This is what retires §5b's liquid case —
  see below.

**The block-arity rule.** A fill is checked against the content model of the
slot element it lands in, at load:

- Element takes only non-block content (`<p>`, `<h1>`–`<h6>`, `<span>`,
  `<a>`, `<time>`, …): the fill must render to **exactly one block**, which
  unwraps to its inline content. Zero or two-plus blocks is a **hard error**
  naming the fill file and the count — never silent invalid nesting.
- Element takes flow content (`<div>`, `<section>`, `<footer>`, `<nav>`, …):
  any number of blocks, verbatim.

**Typed fills are the target, `Html` is v1.** A slot declared
`Stream("link")` should eventually parse a markdown *list of links* into link
maps — the nav's content is then data in the tree while each theme maps its
own fragment over it (`<h2>`s in one theme, a `<ul>` in another). Schema-
directed parsing, the same move the filter language makes.

Inherited wrinkle, restated from §5b: for posts the source tree is not the
URL tree, so per-subtree fills only get interesting for posts once page
bundles exist. Identity slots at the root — the actual motivating case — are
unaffected.

### What it costs

- **Chrome markup changes wholesale.** Already accepted and priced in §5a:
  bodies verified by machine, chrome by eye. This is the moment that budget
  gets spent — spend it once, on this, not twice.
- **`_sass` is rewritten against the new contract.** That rewrite *is* the new
  default theme, and the natural moment to add the dark mode §8b flagged.
- **A fragment binder must be written.** ✅ Was (`binder.rs`, ~760 lines with
  tests, hand-rolled parser — no `lol_html` needed): strictly less machinery
  than the `liquid` crate §5d retired, for strictly more checking.

### Tripwires

- A layout kind wants to emit a wrapper div → that div belongs in a theme
  fragment.
- A theme fragment wants a conditional → there is a missing fact; empty-
  collapses covers presence, facts-as-attributes cover variants.
- The binder grows an expression syntax → stop; it is becoming a template
  language, and §5d already litigated that.
- A slot name appears in CSS but not the part schema → the load-time check
  should already have caught it; if it didn't, the check is broken, which is
  the real bug.

## 5f. One expression language: CEL, subsetted *(specced 2026-07; build at the q23 forcing point)*

q31 settled the direction — extend `filter.rs`, borrow no engine — and this
section pins the contract: **the expression language is a subset of CEL**
(the Common Expression Language, cel.dev). Not "CEL-like": every expression
in `grackle.toml` must be *grammatically valid CEL*, and anything the
evaluator doesn't support is a load-time "valid CEL, not supported yet" —
never a grackle-only dialect.

### Why CEL and not our own syntax

- **We already are.** The filter language predates the decision and landed
  inside CEL's grammar by convergent evolution: `!draft && !hidden`,
  `year >= 2020 && "rust" in tags`, `layout == "post"` are valid CEL with
  the same meaning. The constraint costs ~nothing and no config breaks.
- **The grammar is specified by someone else.** Operator precedence, string
  escapes, number forms — cel.dev documents them; we document only our
  *subset and our functions*, never a syntax.
- **The escape hatch is real.** Rust CEL crates exist; if the hand-rolled
  evaluator ever chafes, the swap cannot break a config file. That is what
  the compatibility contract *buys*, and why it is a contract rather than a
  taste.

### The surfaces

| config key | expression type | status |
|---|---|---|
| `filter =` | `bool` over the row schema | built — the §5 language, already CEL |
| `fields.NAME =` | a typed value over the row (content, text, …) | q31's target; replaces the deriver-struct |
| future derivers (`hero`, `lede`) | same | q23 / q25 |

Route/`title`/`crumb` templates stay the `{token}` placeholder language:
string interpolation over group params, not computation. Folding them in
would put logic where §5d forbids it.

### Typing: the schema is the CEL environment

CEL's spec includes a static checker driven by a declared *environment* —
variables and function overloads, with types. That is exactly the shape
already built: `post_schema()`/`route_schema()` are the variable
declarations, and q31's function registry is the overload set. The existing
discipline transfers whole: parse once at load, check against the
environment, error with the known-names list and did-you-mean. Checked mode
is not optional — an expression that doesn't type-check is a config error.

### Functions: registered in Rust, never defined in config

```toml
[views.published.fields]
summary = 'truncate(content, {"max_blocks": 4, "max_chars": 700})'
```

- The registry declares each function like a schema entry: name, source
  type, option keys with their types, return type —
  `truncate: (content, map) -> content`.
- **Named options are a CEL map literal with string keys.** CEL has no
  named arguments, and we do not invent them — that would fork the grammar
  and void the swap. An unknown option key or wrong value type is a load
  error naming the knowns, like everything else.
- **Values carry facts.** `truncate` returns content bearing the
  `truncated` fact, which the part layer stamps as `data-truncated` (§6d).
  A fact is part of the value's type, not a side channel.
- **The standard library is what we register, nothing more.** CEL's own
  stdlib (`size()`, `matches()`, timestamps…) arrives function-by-function
  when a config actually needs it, each behind the typed registry.

### The divergence ledger (honesty over purity)

Two places the existing language is not CEL, both contained:

- **`*` (match-all) is not CEL grammar.** It is a whole-string sentinel,
  equivalent to omitting `filter` — recognised before the parser runs, not
  part of the grammar. Stays.
- **Bare-field truthiness is not CEL semantics.** `description` meaning
  "has one" is grammatically fine (an ident expression) but CEL's checker
  rejects a non-bool in bool position. Ours is a semantic superset — and
  the failure direction is the right one: a swapped-in engine would error
  **loudly at load**, never silently change meaning. Kept for `!draft`
  ergonomics; tighten to explicit presence tests only if the swap ever
  actually happens.

### Take / refuse

**Take**: the grammar, operator semantics, the environment/checker shape.
**Refuse**: macros (`all`/`exists` comprehensions — a comprehension over
rows is a **view**, §5d); the protobuf type system (our types are the
schema's five); dynamic or late binding (everything checks at load);
evaluating any expression not written by the site's author.

### Tripwires

- A function wants to return different *parts* depending on a condition →
  that is a layout-kind decision, not an expression; the §5d rule extends
  unchanged.
- A function wants to read *other rows* → that is view composition, not a
  function; expressions stay row-local.
- The subset grows until the hand-rolled parser strains → that is the
  signal to swap in a CEL crate, not to keep growing; the contract exists
  precisely so the swap is cheap.

## 5g. Shells: the outermost serialization *(Matt, 2026-07; html root + atom/sitemap/search built)*

**Shell is its own axis**, distinct from layout (which parts a row emits)
and theme (which fragments arrange them): the shell is what the arranged
parts are *serialized into*. A page is parts in the HTML shell; the feed
is the same rows in the atom shell; the sitemap is routes in the sitemap
shell. Views declare `shell = "atom" | "sitemap" | "search"` (built —
this retired q33's template-filename match), and the HTML shell got the
treatment the idea deserved:

### The root HTML shell: themes inherit, never write, the skeleton

Built. The engine owns `root_shell`: doctype, `<html lang data-kind=
"shell" [data-subtheme]>`, `<head>` from the computed facts, `<body>`
around the theme's chrome. A theme's `shell.html` is now **body chrome
only** — no theme writes a skeleton, and three duplicated skeletons (main
theme, two example themes) plus the Rust `light_shell` all collapsed into
one function. What this bought, each previously a defect:

- **A fragmentless theme yields a valid document.** The null theme used
  to render the shell as `<section data-kind="shell">` — not a page. Now
  canonical body chrome sits inside a real skeleton; "a theme needs no
  directory at all" is finally true all the way down.
- **`light` dissolved**: a minimal head (title + robots) in the same
  root shell as everything else, not a third skeleton.
- **`subtheme` moved to the engine root** — no attribute-hole opt-in per
  theme; `theme: "recipes:spicy"` stamps `<html data-subtheme="spicy">`
  everywhere, unconditionally.
- The migration was **accounted byte-for-byte**: 547 changed main-site
  files, 545 explained by `<head data-slot="head">` → `<head>`, 2 (the
  light pages) by the shell stamp plus one trimmed blank line. Nothing
  else moved.

Pending here: a theme that wants to ADD head content (fonts) lost its
former ability to write into the skeleton's head — the mechanism when
wanted is an optional `head.html` theme fragment appended after the
computed facts.

### The search shell: the searchable set is a query *(Matt's framing, built 2026-07)*

`/search.bin` was the last hardcoded serialization — a posts-only
projection baked into the pass. Now a view declares it, in exactly the
sitemap's star shape:

```toml
[views.search]
over = "*"
route = "/search.bin"
shell = "search"
filter = '(kind == "post" || kind == "page") && !draft && !hidden'
```

The rows that pass the route-schema filter are the searchable set,
serialized as the postcard index. Posts contribute date and tags; pages
contribute their rendered body (the same bytes that ship — markdown pages
their `Doc`, raw-HTML pages their fragment; titleless pages wear their
URL). Other route kinds are silently unsearchable even if admitted. The
example searches notes AND recipes/books/manual with the filter above
(18 docs, 5 KB); the main site declares `kind == "post"` and its index is
**byte-identical** through the view path — flipping pages in is a one-line
config decision, not a code change. The js/wasm consumers are emitted only
when a search view exists; a site without one ships zero search bytes.
Noticed en route and closed the same day: listing-shaped pages (indexes,
the homepage) matched every term their link titles mention, so the route
schema gained `stem` (derived from the route's source filename, Null for
sourceless view routes — which pass `!=` by the filter's Null rule,
pinned by test). The example filter appends `&& stem != "index"` — the
same clause page filters already use, now meaning the same thing at the
route layer. 18 docs → 15; "lentil" finds the dal and nothing else. The
honest cost: `manual/index.md`'s own prose is unsearchable too — the
filter keys on shape (index-ness), not on how listing-heavy the body is;
an `embeds`-count fact would be the finer instrument if that ever hurts.

### Script shells: the experimental bench *(Matt, 2026-07 — yes, the pun; built)*

A **shell script** as a shell type: `[shells.name] command = "…"`
registers a serialization the engine doesn't speak, and a view opts in
with `shell = "name"`. The engine pipes the view's member rows to the
command's stdin as JSON and writes its stdout at the view's route
verbatim — PDF, PostScript, whatever the command emits. `sh -c`, run
from the site root; non-zero exit fails the build carrying stderr.

The payload schema is **TEMP by declaration** (stamped
`"schema": "grackle-shell/0"`, asserted by consumers): `{schema, shell,
view, route, site{url,title,author}, rows[{url, title, date,
date_pretty, tags, html}]}` — the same projection the atom shell eats.
It gets versioned the day anything beyond an experiment depends on it;
until then it may change without ceremony.

This is the **promotion path for shells**: prototype as a script, and a
shell that earns keeping becomes a built-in (exactly how atom/sitemap/
search would have been prototyped had the bench existed). First
occupant: the example ships `/llms.txt` via `shells/llms.py` — the md
shell's named forcing consumer, running as an experiment before the md
shell exists. Known limits, accepted for a bench: rows are post-shaped
only (the temp schema's projection), and the command runs on every
build — script shells are for cheap serializations, not compilers.

### The md shell *(specced; consumer named)*

A markdown serialization of part maps, and its forcing consumer is
**`/llms.txt`**: a view over published rows, shell `md` — titles, URLs
and summaries as a markdown listing, which is pure scalar parts and
serializes trivially. The open half is full-text export (`llms-full.txt`
-style): `content` parts are rendered HTML, so a body-markdown shell
needs either the source markdown carried alongside the Doc (cheap —
grackle has it in hand at render) or an HTML→md downconversion (build
machinery for a worse result). Leaning: carry the source. Deliberately
pending until built: whether `md` is view-only or a row can request it
(`/post-slug.md` twins), and how widgets serialize (expanded HTML in md,
or unexpanded source?).

### Still open (q44)

Row-level shells — `layout: light` still conflates "minimal shell" with
a theme choice; a `shell:` row field (`html`/`light`/`none`-bytes) would
split them — and the atom/sitemap serializers becoming true part-map
consumers (a feed entry IS a document-parts subset), and a `json` shell
when something wants one (though a script shell now covers the
experiment: the payload already is JSON — `cat` is a json shell).
Versioning the script-shell payload schema rides with the first
non-experimental consumer.

## 6a. Object references: paths and names

### The measurements that shape this

- Every existing reference is a **root-relative path**:
  `{% image assets/2022/12/part-2-disassembly-a.jpeg %}`.
- **Posts keep assets in a bucket**: posts at `_posts/2022/`, images at
  `assets/2022/12/` — disjoint trees, never siblings. Zero images are
  colocated with posts today.
- **Tree pages already use side-by-side assets**: `code/legacy/romtool/`
  holds `index.html` *and* `screen1.png`. Bubbling is not hypothetical here;
  it's how that content is already organised.
- **6 basenames look ambiguous site-wide — 4 dissolve under bubble+bucket**:
  `screen1.png`/`screen2.png` collide only between `code/legacy/romtool/`
  and `code/legacy/hp48/hp-ide/`, each sitting beside its own page → nearest
  wins, correctly, for both. `a.png`/`codes.png` collide only across
  *collections* (`assets/2003/06/` vs `code/legacy/deathmatch/`) → the post
  finds its bucket copy, the page finds its sibling. **Only
  `screenshot5.png`/`screenshot6.png` are genuine collisions** (both in
  `assets/`, 2003/07 vs 2004/01).

The two-phase rule isn't a compromise — it's the shape the content is
already in.

### The rule

A reference is a **path** if it contains `/` or `://`; otherwise it's a
**name**.

**Paths** resolve from the site root, exactly as today. All 194 existing
invocations take this branch → unchanged output, parity preserved, and even
the 2 genuine collisions stay non-problems because nothing references them
bare.

**Names** bubble up from the referencing row's directory to the site root.
At each level, in order:

1. **Siblings** — direct children of this level. Hit → done.
2. **This level's bucket** — if this level contains a directory matching the
   configured asset pattern (`assets/`), scan it as a **subtree**. Hit → done.
3. **Ascend.**

Exhaust the root → error.

```toml
[objects]
extensions = ["png", "jpg", "jpeg", "gif", "webp", "svg"]
bucket     = "assets"              # directory NAME that marks a bucket, not a path

[collections.blog]
# bucket = "img"                   # optional per-collection override of the pattern
```

**Buckets are positional, not configured.** This is the whole point: there is
no list of bucket paths to maintain and no "collection vs global" ordering to
declare, because the tree already encodes both:

- `_posts/assets/` is a **posts-only** bucket — solely because only posts
  bubble through `_posts/`.
- root `assets/` is the **global** bucket — solely because everything
  eventually reaches root.
- A nearer bucket beats a farther one automatically, for the same reason a
  sibling does. Locality falls out of the walk instead of being ranked by
  config.

Add `_posts/2022/assets/` tomorrow and it starts winning for 2022 posts, with
no config change. Delete it and resolution falls back to root. The
configuration is one word.

Worked examples against the real tree:

| Reference | Walk | Result |
|---|---|---|
| post `_posts/2022/…coffee.md` → `part-1-leak.jpeg` | `_posts/2022/`: no siblings, no `assets/` → `_posts/`: same → **root**: no sibling, but root has `assets/` → scan subtree | `assets/2022/12/part-1-leak.jpeg` |
| page `code/legacy/romtool/index.html` → `screen1.png` | level 1: **sibling hit** | `code/legacy/romtool/screen1.png` — never consults a bucket, so `hp48/hp-ide/screen1.png` is irrelevant |
| post `_posts/2003/…md` → `a.png` | bubbles to root → `assets/` subtree | `assets/2003/06/a.png` — the `code/legacy/deathmatch/copyprotection/a.png` copy isn't in a bucket, so it can't collide |

**Ambiguity is per-step.** 2+ hits within one level's siblings, or within one
bucket's subtree → error listing every candidate. Across levels there is no
ambiguity by construction: nearer wins, which is shadowing, which is the
point. Exhausting the root → error naming the reference and the referencing
row. Both are transaction-time constraints (§4) — a bad reference fails the
build at the file that caused it rather than shipping a broken `<img>`.

Name resolution is therefore **additive and immediately useful**: paths keep
the existing corpus byte-identical, sibling lookup already matches how
`code/legacy/*` is organised, and the root `assets/` dir is discovered as a
bucket automatically — bare names work for posts today, with no restructuring
and no bucket configuration at all.

### `{% image %}` vs `<img>`/`<iframe>` (and `<style>`)

Do **both**, but let the reference form decide, which keeps them from
fighting:

- `{% image %}` stays — 194 uses need it, and it carries the
  `left`/`right`/`inline` mode that markdown image syntax can't express.
- A **post-render `lol_html` pass** rewrites `<img src>` and `<iframe src>`
  **only when the src is a bare name**. Anything containing `/` or `://`
  passes through untouched — which is what makes this safe by construction:
  every existing raw `<img>` in 20 years of posts uses a path, so the pass
  cannot perturb them, and `diff` stays clean.
- This makes plain markdown `![alt](foo.png)` work (comrak emits `<img>`,
  the pass resolves it) with no new tag, and gives `<iframe src="demo.html">`
  the same treatment — so an `{% iframe %}` tag is unnecessary. Iframes
  resolve and rewrite but are **not** thumbnailed.
- The same pass is where `feed_images` already lives (§8) and where `<style>`
  extraction happens (§6c), so this is one HTML rewrite stage doing four
  jobs, not four pipelines.

### Row links and view links *(Matt's rule, built 2026-07)*

The day the example grew locale prefixes, slugged tag routes and
templated pagination, hand-typed URLs in content became lies waiting to
happen — URLs are DERIVED values here. Matt's rule closes the gap, and
it is this section's principle finishing its job: **authored links
reference what the database owns.**

1. **A link to a row references its source file** — relative to the
   linking file (`carbonara.md`, `advanced/markers.md`) or
   root-relative (`/recipes/carbonara.md`) — and the engine renders the
   URL, exactly as `{% post_url %}` always did for posts. An unknown
   source is a build error naming the file, with a closest-match
   suggestion.
2. **A link to a view uses `view:` syntax** — `view:gallery`,
   `view:recipes_by_course/dinner` — rendered through the owning view's
   route template (tag slugs applied, multi-level chains keyed
   positionally), locale-aware (a French row links into its locale's
   archive when it materialized), and verified against the route set: a
   typo'd key errors LISTING the keys that exist.
3. **`[links] policy`** grades enforcement. `strict` (the example)
   errors on raw internal URLs, answering with the correct form —
   `"link the source instead: /recipes/carbonara.md"`. `loose` (default;
   the main site until cutover migration) resolves the new forms but
   leaves raw URLs alone.

Resolution is a comrak AST pass over Link nodes (`render_doc_with`),
per-row, against a `LinkSpace` built once per build (source→URL over all
three tables, the route set, and URL→suggested-form for the strict
errors). The byte-oracle rule that made it safe on a 20-year corpus:
**the engine rewrites only where the browser would get it wrong** — a
relative link whose source-resolution and URL-resolution agree (the
`downloads/foo.zip` idiom, 27 files' worth) ships byte-identical;
`.md` references and cross-dir links get the engine's answer. Main site
verified byte-identical under loose.

**`.slots/` fills now render THROUGH the resolver, per consuming page**
(2026-07, same day): fills store raw source and render at page time with
the page's locale, so one `nav.md` of `view:`/source links serves every
locale — `view:blog_index` is `/blog/` on an English page and
`/fr/blog/` on a French one — and `nav.fr.md` (the row suffix
convention, no config) exists only to translate labels, winning within
its nearest-wins level. Main site byte-identical: raw-URL fills under
loose resolve to themselves. Pending here: raw-HTML pages bypass the
resolver (the §6d `lol_html` rewrite stage is their seam); the
closest-match suggester is stem-exact, not fuzzy; `{% post_url %}`
could now dissolve into a plain source link; strict for the main site
rides the publish-cutover migration.

## 6b. `_cache/`: one content-addressed store for every derived artifact

> ✅ **Thumbnails built** (`thumbs.rs`). The `thumbs/` leg of the store below is
> real: `{% image %}` sources are hashed with blake3, cached at
> `_cache/thumbs/{hash}.{ext}`, and published at `/static/{hash}.{ext}` — the
> split this section argues for. The `css/` and `embed/` legs remain specced.
> Measured: 260 thumbnails (matching the reference `_thumbs/` count), a warm
> build reads-and-hashes only (0.4s), a cold one decodes/resizes/re-encodes
> (2.5s). WebP is deferred; the contest is `{orig, png, jpg}` today.

Everything expensive is a pure function of bytes → cache it by the hash of
those bytes. Keys are content hashes, so entries are **self-invalidating and
never stale**: a changed input is simply a different key. The cache is
therefore always safe to delete (a cold build just costs time) and safe to
share across profiles and branches.

```
_cache/                      # gitignored; not published
  thumbs/{hash}{ext}         # blake3(image bytes + variant) → smallest of {orig,png,jpg,webp}
  css/{hash}.css             # hash(scss source + resolved imports) → compiled css
  embed/{model}/{hash}.f32   # hash(markdown body text) → 384-dim vector
```

```toml
[cache]
dir = "_cache"

[static]
dir = "/static"              # published URL prefix for derived assets
```

Two distinct things that today are conflated:

- **`_cache/` is the build cache** — gitignored, never published, keyed by
  content.
- **`static.dir` is the published location** — where derived assets get URLs
  (`/static/{hash}{ext}`).

Today `_thumbs/` is *both*: `thumbnail.rb` writes into the output dir and the
URL points at the cache. Splitting them is what lets the cache be deleted
without touching the site, and lets `serve` skip publishing entirely.

**Cache is keyed by content, not by path**, which means a renamed post keeps
its embedding, a moved image keeps its thumbnail, and the drafts profile
shares every entry with the public profile.

### What moving to `/static/` buys (now that image URLs are free to change)

Derived assets are exempt from URL parity (§11.12), and the current scheme was
carrying two workarounds that simply evaporate:

- **Extensions come back.** Today's thumbs are `/_thumbs/{md5}-600-600` —
  *extensionless*, so the server can't infer a Content-Type and browsers fall
  back to sniffing. `/static/{hash}.webp` is self-describing.
- **`_thumbs/.htaccess` disappears.** `thumbnail.rb` writes a one-line
  `.htaccess` into its output dir purely to set `Cache-Control` on
  extensionless blobs. With everything under `static.dir` content-addressed,
  *one* rule covers the whole directory.
- **Immutability becomes true, not asserted.** Every URL under `static.dir` is
  a content hash, so `Cache-Control: public, max-age=31536000, immutable` is
  correct by construction — and that is exactly the Lighthouse
  "efficient cache lifetimes" finding from earlier this month, answered
  properly instead of with the `?20260627-02` query-string trick the
  stylesheet uses today. Content-addressed CSS retires the manual cache-buster
  bump entirely: change the SCSS, get a new URL, no config edit, no forgetting.
- **WebP becomes available for free.** The variant contest already picks the
  smallest of {original, PNG, JPEG}; adding WebP is one more encoder and the
  extension now travels with the URL.

### Embeddings (this retires LSI) *(built 2026-07)*

**Built** (`embed.rs`), with three deliberate departures from the spec
below, each an improvement the build surfaced:

1. **The embedded text is `title: … \n tags: … \n body: …`** — title and
   tags are signal, not metadata, so they are in the text and therefore in
   the **cache key**. The original "retitling never re-embeds" claim is
   deliberately inverted: retitling *should* re-embed, because the title
   changed what the post is about. (Whitespace-only edits still don't:
   the text is trimmed.)
2. **Ranking policy is config** (`[related]`): `limit`, `min_score` on the
   *adjusted* score, and year-distance handling — `year_penalty` (soft
   per-year subtraction) and/or `max_years` (hard cap). A 2004 post is
   probably not relevant on this blog but might be on another; the site
   declares its prior (here: penalty 0.01/yr, min 0.4). Observed effect:
   the blogging-meta post still pulls its genuinely-related 2009/2010
   platform posts (raw 0.58–0.64 beats the penalty); weaker cross-era
   matches drop.
3. **Stale-while-revalidate** — the resident database's move (§7). An
   `index.json` maps post name → current vector hash; a post whose text
   changed serves its **old vector until reprocessed**. `build` (AOT,
   publishes) embeds pending posts *before* rendering; `serve` renders
   immediately on stale vectors and re-embeds on a background thread,
   poking the rebuild channel on completion. Proven live in the serve log:
   edit → rebuild 366ms on the stale vector → "embedded 1 posts in 0.2s
   (background), re-rendering" → automatic fresh re-render. Failure
   (offline model download) logs and waits for the next natural rebuild —
   no hot retry loop.

Mechanics as specced: vectors cached content-addressed
(`_cache/embeddings/{hash}.vec`, 1.5 KB each), model beside them
(`_cache/models/`, downloaded once), L2-normalised so similarity is a dot
product, brute-force over the corpus, and **a post never matches itself** —
its own vector is the perfect cosine, so the exclusion is pinned by a test
(identical twin vectors: the twin ranks, the self does not) rather than
left as an incidental filter. Measured: full re-embed ~20s inside a
38s cold build; **warm build 1.5s total**. `grackle query similar <url>`
makes the ranking inspectable, embedding pending posts first so it never
lies.

**"Related" is AXES, not a list.** A post relates to others along multiple
axes — embedding similarity, earlier, later — and pivots along each. The
part model says so: `document` carries `relations: Stream("relation")`,
each relation = `{axis, label, items: Stream("neighbor")}`. The axis rides
into markup as `data-axis` (an attribute hole) for per-axis styling; the
label lives on the group, which retired the label-on-first-item hack the
flat model needed; an axis with nothing to say contributes no group
(rule 2); and a future axis — same-tag, series — is one more group pushed
by the producer: no schema change, no theme change, the `relation`
fragment renders axes it has never heard of.

`_config-prod.yml` sets `lsi: true`, which is the `Populating LSI... /
Rebuilding index...` phase visible in the build log — a dominant chunk of the
90-second build, recomputed **from scratch every time** because Jekyll has no
content-addressed cache for it.

Replacing it: embed each `.md` body once, cache by content hash, and compute
`related_posts` as cosine similarity.

- **Model**: `all-MiniLM-L6-v2`, 384-dim. 327 posts → 327×384 f32 ≈ **500 KB**
  total. Similarity is a brute-force dot product over 327 vectors —
  microseconds. **No vector index** (HNSW/FAISS) is remotely justified at this
  scale; adding one would be the classic mistake.
- **Cache key = hash of the markdown body only** (post-front-matter), so
  retitling or retagging a post doesn't re-embed it.
- **Incremental by construction**: edit one post → one embedding recomputed →
  related-posts recalculated in microseconds. The LSI phase disappears from
  the build entirely, and `serve` can afford to keep it live.
- **Liquid surface**: `site.related_posts` (Jekyll-compatible, `limit: 4` in
  `post.html`) → top-N by cosine similarity, excluding self and any row the
  active profile didn't materialize. `grackle query similar <url>` falls out
  for free and makes the ranking inspectable.
- **Embeddings never publish** — build-time only, no URL, no bytes shipped.

This is the one place where the port can be *better* rather than merely
equivalent: LSI's related-posts were mediocre, and the diff harness can't
check them (§8 lists related posts as knowingly-inexact anyway), so there's no
parity cost to improving them.

**Crate: `fastembed`, not `rust-bert`** — rust-bert is 2 years stale, 16k
downloads, and drags in libtorch (~2 GB), which is hostile to the Docker
build. `fastembed` is actively maintained, 1.2M downloads, ONNX-based, and
ships MiniLM directly. (`candle` is the pure-Rust fallback if ONNX ever
proves awkward.)

### TF-IDF search index — the searcher is the same code, compiled to wasm *(built 2026-07)*

**Built, with the architecture upgraded mid-design**: instead of a JSON
index consumed by a hand-written JS searcher (whose stemmer would be a
drift-prone port of the Rust one), the search core is **one crate**
(`search-core`: stem, tokenize, index build, rank) used by both ends —
`grackle build` calls it to ship `/search.bin` (postcard, not JSON: the
format is private to the two ends of the same crate; `grackle query search`
is the inspectable surface), and the identical code compiles to
WebAssembly (`search-wasm`, a ~90 KB cdylib behind a raw no-bindgen ABI:
`alloc`/`init`/`search`) shipped as `/search.wasm`. **Symmetry by
construction**: the browser stems queries with the same compiled function
that stemmed the corpus, and the stemmer is free to stay simple (or swap
to Snowball later) because it cannot desynchronize.

The page ships an icon, nothing else: clicking it injects `/search.js`
(3.6 KB loader — bytes and pixels only, every search decision is in the
wasm), which fetches the blob + index and answers per keystroke. The
**last query token is a live prefix** over the sorted term map ("bluet"
finds bluetooth, "jekyl" finds the Jekyll posts) — real
search-as-you-type, cheap in Rust, awkward in the JS it replaced.

Measured (327 posts): 7,125 terms, 29,793 postings, **195 KB index built
in 22ms** per build (no TF disk cache — tokenizing the corpus is
single-digit ms, so the spec'd cache would be machinery without a cost to
pay for; the per-row/corpus-wide decomposition survives in memory).
Postings capped at 40/term, scores TF·IDF quantised u16, title/tag hits
boosted 5×, stopworded, years searchable. First-click payload ≈ 288 KB
(js + wasm + index), all cacheable; every page's default payload stays
**zero JS**. The wasm blob and its loader are **engine assets**
(`grackle/assets/`, embedded via `include_bytes!`, emitted when a site
declares a search view — they must version with `/search.bin`'s format,
so they can't be theme-committed; a theme owns only the trigger and the
overlay CSS). Rebuild with `cargo build -p grackle-search-wasm --release
--target wasm32-unknown-unknown` and copy to `grackle/assets/search.wasm`.
*(Moved from `themes/default/` 2026-07 when the example site wanted
search — the icon was one shell edit plus overlay CSS, no blob copied.
The index itself is now a declared SHELL — `shell = "search"`, §5g — so
the searchable set is a query over the route schema, spanning tables.)*

**Swiftype is retired**: the `data-swiftype-index` attributes left the
shell with the chrome cut, and the launcher this replaces was a
third-party service tab.

The original sketch, kept for the record — the shape survived even though
the JSON/JS specifics did not. A different tool for a different job,
sharing the same cache discipline.
Embeddings answer *"what is this like"* (fuzzy, build-time, 500 KB of f32);
TF-IDF answers *"where does this word appear"* (exact, shippable to a browser,
no model at runtime).

- **Stemming**: `rust-stemmers` (Snowball English) — `parsing`/`parsed`/`parse`
  collapse to one term.
- **Decomposition mirrors the cache**: per-post **term frequencies** are a pure
  function of the body → content-addressed like everything else
  (`_cache/search/{hash}.tf`). **IDF is global**, so it must be recomputed
  whenever the document set changes — but that's a cheap fold over 327 cached
  TF maps, not a re-tokenisation. Same shape as embeddings: the expensive
  per-row part caches, the corpus-wide part is a fast reduction.
- **Published as a derived asset**: `/static/{hash}.json` → `immutable`
  caching for free (§6b), and it self-busts when any post changes.
- **Shape**: `{terms: {stem: [[docId, score], …]}, docs: [{url, title, date}]}`.
  Postings sorted by score, scores quantised to u16, stopwords dropped.
- **Budget**: 327 posts is small, but 20 years of prose is a lot of
  vocabulary. This needs a **measured size check** before it's a good idea;
  pruning knobs (min doc frequency, max postings per term, title/heading
  boosting) are cheap insurance. If the raw index lands multi-MB, the fallback
  is a small term dictionary plus lazily-fetched postings shards.

**This retires Swiftype.** The header search is currently
`javascript:document.getElementById('st-launcher-tab').click()` — a launcher
for a third-party service (Elastic's Swiftype), which is also why the layout
still carries `data-swiftype-index` attributes and a `<meta class="swiftype">`
tag. A static JSON index plus a small first-party script removes that
dependency. Note the main site ships **zero** JS after this month's cleanup,
so search must **lazily load on interaction only** — the default page weight
stays at zero.

## 6c. Per-post `<style>` (SCSS)

**This formalises something the posts already do.** Three posts contain
`<style>` blocks, and all three are *already written in SCSS shape* — nested
rules and `&` parent selectors:

```scss
table#bit_twiddling_truth_table {
  thead { background-color: #fafafa; th { padding: 0.2rem; } }
  th, td { &.slashed-background { … } }
}
```

Jekyll passes `<style>` through raw, so today these only render because
**native CSS nesting** happens to be supported in current browsers. They are
unvalidated (a syntax error fails silently at runtime) and broken on anything
older. Compiling them through `grass` flattens the nesting — *widening*
browser support while preserving intent.

The rule: any `<style>` block in a row's body is extracted by the HTML rewrite
pass (§6a), compiled as SCSS, cached by hash in `_cache/css/`, and hoisted
into `<head>`.

- **Inline `<style>` in `<head>`, not a `<link>`.** These blocks are small and
  per-page; a separate file would add a render-blocking request, which is
  precisely what we just spent this month removing from the font path.
- **Auto-scoped by default.** `body.multipost` renders many posts on one page,
  so today a post's `<style>` leaks onto its neighbours on index pages — a
  latent bug that has simply never bitten (the existing blocks are
  ID-selector-scoped by hand). Compiling as SCSS makes scoping free: nest the
  author's rules under the post's unique selector. Opt out with
  `style_scope: false` in front matter for the rare global rule.
- **Syntax errors become build errors**, named to the post — a transaction-time
  constraint like every other (§4), instead of a silently dead rule.

**Expected diff: exactly 3 posts** (`2022-12-20-deriving-a-bit-twiddling-hack`,
`2009-07-20-disable-controls-with-a-css-only-glass-panel`,
`2026-06-11-life-before-main`) — nesting flattens, and the block moves from
body to `<head>`. Visually identical in a modern browser, fixed in an old one.
`diff` should classify these as "equivalent", and they're the argument for
doing it rather than a reason not to.

## 6d. Blocks and rewrites: two ways into the rendered markdown

**Status: blocks built (stage A, 2026-07); notes and rewrites remain.**
`markdown::render_doc` parses once and yields both the whole render (posts
and the feed use it unchanged — post pages verified byte-identical) and the
top-level block sequence, in one render pass where the old pipeline rendered
every post twice. **The summary is a computed field on the view's rows** —
a derived column, not a rendering attribute:

```toml
[views.published.fields.summary]
truncate = { max_blocks = 4, max_chars = 700 }
```

`Doc::truncate` is mechanism only (blocks kept until a budget runs out,
block granularity, at least one always kept, `max_chars` counting visible
text); the *deriver* (`truncate`) discriminates the field definition and is
validated at load — no deriver, or an unknown one, errors naming the known
set. **Fields flow with rows through `over` composition** the way filters
do: declared once on `published`, every listing composed over it inherits
the column; redeclaring the name overrides, nearest wins. The deriver's
fact (`truncated`) rides along, feeding `data-truncated`. Listing previews
consume the field named `summary` by convention; no summary field in the
chain means rows ship whole. This is also where the §5e archetype gaps
land later: `hero` (open q23) and `lede` are more derivers producing more
columns. **Marked not-quite-right (open q31)**: deriver-as-struct-key is a
stopgap shape — if the config grows *functions*, a field wants to be an
expression (`summary = truncate(content, max_blocks=4)`), and this gets
revisited rather than extended. (Two wrong altitudes were corrected in one session getting here:
the cut rule started as engine code — policy belongs in config — and then
as a view *attribute* — a summary is a property of the rows, not of the
view's rendering.) **Measured: `/blog/` 160 KB →
15.7 KB, `/blog/tags/rust/` 180 KB → 11.3 KB (93.8%).** The nth-of-type
truncation CSS is deleted; `truncated` is a schema fact stamped
`data-truncated` (★ settled, q17); concat-equals-whole is a corpus test
pinning the footnote post as the only exception. Deferred to stage B with
reasons: the **notes stream** (needs its consumer — sidenotes want a third
grid column, q18) and the **rewrite stage** (its five use cases are mostly
unbuilt subsystems: §6a names, §6c styles).

Markdown is currently an opaque blob: `content` goes into a `<section>` and
nobody can touch it. Two mechanisms open it up, and they are **not
alternatives** — they solve different problems and compose.

| | addresses by | serves | example |
|---|---|---|---|
| **Blocks** | position | *layout* — placing parts of the content | summary takes the first 2 paragraphs |
| **Rewrites** | CSS selector | *transformation* — changing content in place | wrap every `<table>` in a scroll container |

### Blocks, and the 93% that justifies them

Markdown renders to a **sequence of top-level blocks** (paragraph, heading,
code, list, table, html) rather than one string. A layout kind then takes what
it needs: `document` takes all of them, `summary` takes the first few, a future
`lede` slot takes `blocks[0]`.

The justification is measured, not aesthetic. Today the site truncates
summaries in **CSS**:

```scss
// _style.scss — body.multipost
.post.post-summary .post-summary__main > section, .post > section {
  > p:nth-of-type(2), > :nth-of-type(4) { ~ * { display: none !important; } }
}
```

So every listing ships **complete post bodies** and hides most of them:

| `/blog/` | bytes |
|---|---|
| page total | 140,884 |
| post bodies | 134,635 (96% of the page) |
| actually visible | ~3,564 |
| **shipped, then `display:none`'d** | **131,071 — 93% of the page** |

`/blog/tags/rust/` is **169 KB** to show five previews. With blocks, the
summary layout simply never emits blocks 3..n — same rendering, ~93% smaller,
and the CSS truncation rule becomes dead code. That is a bigger win than
anything in the font/image work earlier this month, and it falls out of the
model rather than being a special case.

Blocks stay **internal to layout kinds** — they are not exposed to templates.
A template iterating an AST is a trap (Hugo keeps `.Content` a string for
exactly this reason); templates get *slots* (§5b) and *rewrites*, which are
addressed by name and selector rather than by walking a tree.

### Rewrites: one selector-driven stage, not five ad-hoc passes

The design has quietly accumulated five separate transformations of rendered
HTML: bare-name `<img>`/`<iframe>` resolution (§6a), `feed_images` (§8),
`<style>` extraction (§6c), code blocks → Rouge shape, and stripping comrak's
injected heading anchors (§8a). Those are all the same operation — *match
something, change it* — and `lol_html` is already a dependency for the first
two.

So make it one stage with a rule table, and let the tree extend it
positionally, exactly like §5b:

```toml
# code/.rewrite.toml
[[rule]]
match = "table"
wrap  = "<div class='table-scroll'>"

[[rule]]
match    = "a[href^='http']"
template = ".hooks/external-link.html"   # gets href, text
```

A rule whose replacement is a **template** is Hugo's render-hook idea, but
addressed by CSS selector instead of by node type — a better addressing
language, and one that already exists in the codebase.

### Why they compose rather than overlap

**Blocks give position; selectors give type and attributes.** Neither can do
the other's job:

- A streaming rewriter cannot *relocate* content into a different layout slot,
  which is what a lede or a summary needs.
- A block list cannot say "every external link", which is what a rewrite needs.

And blocks *remove* the need for positional selectors in the rewriter, which
matters because `lol_html` supports only a subset of CSS (attribute, class,
descendant/child combinators, some `:nth-child` — **verify before relying on
`:first-of-type` or `:has()`**). Position comes from the block index; the
selector only has to match kind.

Pipeline order:

```
source
  → tag expansion        ({% image %}, {% post_url %})   -- pre-markdown
  → comrak               -> Vec<Block>                   -- the AST split
  → rewrite each block   (selector rules, per-subtree)   -- post-markdown
  → layout kind picks blocks -> main
  → theme shell
```

### What this changes elsewhere

- **§9a's comrak rationale survives, for a different reason.** comrak was
  chosen over `pulldown-cmark` partly for a mutable AST pass (code blocks).
  Blocks need the AST too — to split at top level — so comrak still wins, but
  the justification is now "we need the tree", not "we mutate it".
- **§6c's `<style>` extraction becomes a block kind**, not a special case: a
  `<style>` is an HtmlBlock the layout hoists.
- **The CSS truncation rule can be deleted** once summaries stop shipping what
  they hide.

### Measured: the spike (327 posts)

```
posts: 327, mismatched: 1
blocks/post:  min 1   median 4   max 124
cut blocks:   min 1   median 2   max 4
summary bytes if truncated at build: 185,612 of 727,547  (74.5% saved)
```

**326 of 327 posts satisfy `concat(blocks) == markdown_to_html(src)` byte for
byte.** comrak's `format_html` takes any node, so blocks are a loop over
`root.children()`, not a parser change — and a summary is then a literal
*prefix* of the document, which the diff harness can prove rather than eyeball.
This is now a corpus test, not an assumption.

74.5% is the whole-corpus average; the 93% above is `/blog/` specifically,
because the posts it shows are the long ones. Both hold.

**The single mismatch is footnotes** — see "Footnotes are not blocks" below.

### Corrections to the above

Two things this section originally got wrong:

1. **It is `:nth-of-type`, not "4th child".** `> :nth-of-type(4)` is
   `*:nth-of-type(4)`: it matches any element that is the 4th **of its own tag**
   — the 4th `<p>`, or the 4th `<h2>`, or the 4th `<pre>`. So the cut is at the
   earlier of the 2nd paragraph and the 4th-of-anything. The 2nd paragraph
   nearly always wins; the other arm only fires on posts opening with four
   headings or four code blocks. Ported faithfully it is ~10 lines.
2. **There is a *second*, independent truncation.** `_post-summary.scss:189`
   also clips by height:
   ```scss
   $preview-line-height: 16pt; $preview-fade-start-line: 5; $preview-max-lines: 7;
   > section { max-height: calc(7 * 16pt); overflow: hidden; mask-image: ...fade at line 5; }
   ```
   Whatever blocks we ship, **only ~7 lines are ever visible**. The DOM cut is
   far more generous than the visual one — which means once exactness is banked,
   there is much more room to cut than the block rule suggests.

### The ★ needs a decision before blocks ship

`_post-summary.scss:205` puts a ★ after `> p:last-child`. Today that `<p>` is
the post's *genuine* final paragraph, which is `display:none`'d on any truncated
post — so no star renders. The star only appears on posts short enough **not** to
be truncated: **79 of 327**. That reads backwards from what a "there's more"
marker should do, and is probably a latent bug.

Truncate at build time and the last kept block becomes a real `last-child`:
**321 of 327 summaries would sprout a star.** So blocks are *not* visually
neutral until this is settled. The clean fix is to stop inferring truncation
from the DOM and state it — emit `class="truncated"` on the section, gate the
star on it — which would put the marker on the right 248 posts for the first
time. That is a visible change to the site and needs sign-off.

### Footnotes are not blocks — they are a second stream

The one corpus mismatch (`life-before-main`, +898 bytes) is not an edge case to
paper over. It is a **category error** in the block model.

comrak's parser *relocates* footnote definitions to the end of `root.children()`
— the author writes each definition directly under its referencing paragraph,
and that adjacency is destroyed at **parse** time. Rendered standalone, each
definition emits its own complete `<section class="footnotes"><ol>` wrapper, so
`concat` yields N sections instead of one merged one. Those are the 898 bytes.

A footnote definition is not a block. **It is an annotation on a block.** Both
hooks already exist:

```
definition content only:  format_html over the definition's *children*
                          → <p>The definition text.</p>   (the ↩ backref is gone:
                            it is formatter-injected, not in the AST)
block → note association: NodeValue::FootnoteReference { name, ix }
```

So model two streams:

```rust
pub struct Doc   { blocks: Vec<Block>, notes: Vec<Note> }
pub struct Block { html: String, tag: &'static str, notes: Vec<usize> }
pub struct Note  { name: String, num: u32, html: String }
```

The exception then **dissolves** rather than being special-cased:
`concat(blocks) == whole` holds 327/327 for the content stream, and placement
becomes a layout decision:

- **sidenote** — each block, then its notes into a margin slot (Tufte-style)
- **endnote** — all blocks, then the gathered section (today's behaviour)
- **summary** — `blocks[..cut]`, notes dropped

That last one fixes a bug for free: summaries currently ship
`<sup><a href="#fn-0">` refs whose definition is past the cut and
`display:none`'d — dead anchors on every listing. (There is also a latent
duplicate-`id="fn-0"` collision if two footnote posts ever share a listing page.
Only `life-before-main` defines footnotes today — the 2004 post's `[^` are regex
character classes — so it is theoretical, but it stops being possible.)

**Sidenotes need a layout change**: the post grid is
`grid-template-columns: 8.75rem minmax(0, 1fr)` — a *left* sticky margin
(breadcrumbs + tags) with the content column claiming everything else
(`width: calc(100% + 8.75rem + 2rem)`). There is no right margin to render into.
A third column is a theme change, and it is the first genuine use case for a
layout owning a slot the document stream does not.

### The third addressing mode

Footnotes contradict this section's opening claim that blocks + rewrites is
enough:

| mechanism | addresses by | can do sidenotes? |
|---|---|---|
| blocks | position | ✗ — the note is not at its position |
| rewrites (`lol_html`) | selector | ✗ — streaming; cannot move a definition *backwards* to its ref |
| notes | **identity** (`name` ↔ `#fn-0`) | ✓ |

Association by identity is what neither covers, and it exists only at the AST.
That is the concrete argument for AST-level access — not a preference.

### Risks

1. **Truncation semantics must be reproduced exactly** — see the `:nth-of-type`
   correction above; the original phrasing of this risk was itself wrong.
2. **Rewrite rules are unbounded rope.** A selector table that can inject
   templates is a small language; it needs the same load-time validation as
   filters (§5) or it becomes the untyped front matter problem again.
3. **Per-element template calls cost.** 327 posts × many elements. Mitigated by
   the content-addressed cache (§6b), but worth measuring before allowing
   templates in rules.

## 6e. Hierarchy: the page's tree and the tree's tree *(specced and built 2026-07)*

> ✅ **Both axes are built** (`outline.rs`, 2026-07), against the example
> site (§7a). **Path axis**: `.section` is engine vocabulary like
> `.slots/` (a bare file, no config), the scan rides the same .gitignore
> defence as markers, `order:` front matter landed on pages, the root's
> index leads, index-less directories appear as unlinked labels (q27's
> semantic, shipped), nested `.section`s resolve nearest-wins, and
> `aria-current` rides the attribute hole; trees derive once per section
> per build — only `current` moves per page. **Heading axis**: `toc:`
> rows carry their outline, extracted *from the rendered block bytes
> themselves* (id and text read out of the shipped `<h2 id=…>`), so link
> and target cannot desync — pinned by a sync test; nesting tolerates
> level jumps; the h2–h3 window is hardcoded v1 policy pending the §5f
> `outline()` deriver. One recursive `outline_entry` kind serves both
> axes through one theme fragment — the unification this section bet on,
> demonstrated on the example's `configuration` page, which renders the
> section tree and its own outline side by side.

The site has two hierarchies, and they are the same shape seen on two axes:
**headings nest by level** (h2 contains its h3s) and **pages nest by path**
(`code/legacy/` contains its 22 projects). Both are hierarchy *derived from
position* — §6d's position axis, read in depth instead of in sequence. And
half of the machinery already exists: **breadcrumbs are the upward walk** of
the path tree (`ancestors()`, §5c provenance). What's missing is the
**downward walk** on either axis:

| | toward the root | toward the leaves |
|---|---|---|
| heading tree | — (the title is the root) | **page ToC** — this document's outline |
| path tree | breadcrumbs ✅ (§5c) | **section tree** — a manual-style file ToC |

Measured, both cases are real: 7 posts use `##` headings (the long technical
ones — exactly the ToC audience), and `code/`/`writing/` hold 36 index pages
up to five levels deep, with 23 index-less directories (q27) between them.

### One part vocabulary, two producers

The unifying move is the §5e one: both ToCs are **the same recursive part
kind**, produced from different sources.

```
"outline_entry" => [("label", Text), ("url", Url),
                    ("current", Text),                   // aria-current, the pagination trick
                    ("children", Stream("outline_entry"))]
```

The `document` schema gains two parts sharing it: `outline` (this document's
headings) and `section` (the enclosing section's page tree) — both
`Stream("outline_entry")`. A docs-style page showing the file tree on the
left and its own headings on the right is then *two grid areas in theme CSS*;
a theme that declines either slot loses nothing (rule 2 deletes the empty
element; canonical renders whatever exists — the null theme gets navigable
ToCs for free, and the completeness falsifier extends without new code).

This is the **first self-referential schema**, and the binder already
handles it: streams render their child fragments by kind, recursion
terminates on finite data, and a `toc` fragment containing
`<ol data-slot="children">` is just a fragment that maps itself. Worth a
test; not worth new machinery.

**Why not relations?** `relations` (§6b) are flat row↔row groups along
axes; outlines are recursive *containment*. Forcing an outline into a
relation flattens it; forcing relations to recurse complicates every
theme. Two shapes, kept apart on purpose.

**Derived, not authored — so parts, not slot fills.** `.slots/` carries
content a human wrote; ToCs are computed from structure that already
exists. Both exit through `data-slot` holes, but a ToC never lives in a
file — there is no `{% toc %}` tag and no `.slots/outline.md`, for the same
reason there is no `{% for %}`.

### The page outline (heading axis)

- **Source: the same parse that renders.** `render_doc` walks the AST once
  (§6d); collecting `(level, id, text)` per heading is a few lines in that
  walk. The ids are comrak's `auto_ids` — already emitted, already verified
  (§8a: zero heading diffs) — and because the outline is extracted from the
  same AST pass that emits them, link and target *cannot* desynchronize
  (the search-wasm argument, §6b). Nesting the flat list by level is the
  standard outline algorithm; a level jump (h2 → h4) nests under the
  nearest shallower entry. (The refactor deleted `Block.tag` as unused;
  this is its replacement in better form — structured heading facts, not a
  string per block.)
- **Opt-in is schema, cascaded by the tree.** `toc: true` front matter —
  §5a's canonical *render directive* example, finally real — with markers/
  rules supplying subtree defaults, so "everything under `doc/` has a ToC"
  is one marker, no per-file editing.
- **Depth is production policy, not CSS.** The §6d lesson applies
  unchanged: never ship levels a stylesheet hides. v1 hardcodes a sane
  range (h2–h3); the §5f expression form is the future home —
  `toc = outline(content, {"max_depth": 3})`, one more deriver in the
  typed registry, riding the q23 forcing point.

### The section tree (path axis)

- **The root is declared positionally.** A marker (working name
  `.section`) makes its directory a section root: every rendered row
  beneath it carries a `section` part — the root's subtree of pages, with
  the current row marked. Config says what the marker means; the tree says
  where. Finding your root is the same nearest-wins ascent as markers,
  buckets and slot fills — one more user of the one algorithm.
- **Membership and labels come from the database.** Rendered rows only
  (v1); labels are page titles (schema); an index-less directory is q27's
  unlinked label — **this feature is q27's forcing point**, and the
  auto-index view it recommended would share the `outline_entry` fragment.
- **Ordering must be declared, not inherited from `ls`.** `order:` front
  matter, else lexical filename — the §5-audit `order_by` gap again (the
  corpus zero-pads, so lexical is correct today; that is luck, and the
  field makes it intent).
- **`current` is the pagination trick verbatim**: an attribute hole fills
  `aria-current` on the row's own entry; theme CSS selects on it; a11y and
  styling share one part.
- Derivation is once per section per build, not once per page — every page
  in a section shows the same tree, only `current` moves.

### Costs and edges, named now

- Heading text needs inline-markup stripping (a heading containing a link
  outlines as its text). Same AST walk, small.
- Static passthrough rows are not pages: legacy HTML trees (mindstorms)
  get no section part until they become rendered rows — consistent with
  the §5 audit's opt-in restructure, not a new gap.
- A marker that declares *scope* is a new marker flavor: today's markers
  set row defaults; `.section` names a subtree unit. If it also wants
  options (depth, ordering), markers grow a payload — or the §5b
  `.schema.toml`-style per-directory config does it. That choice is q35.

## 6f. i18n: the locale axis *(Matt's direction, first slice built 2026-07)*

q41 called this the one classic SSG feature the model lacked outright.
The slice that exists follows Matt's framing — **path selectors tell us
which language variant to select, and that is configurable** — and the
model absorbed it with less machinery than the survey feared, because
every hard part mapped onto something already built.

```toml
[i18n]
default = "en"
locales = ["fr"]
# selector = "suffix" (default: dal.fr.md) | "prefix" (fr/recipes/dal.md)

[i18n.names]
fr = "Français"        # what the translations axis calls the locale

[records.tags.meta]    # enum records: the value domain of a grouped field
name = { en = "meta", fr = "méta" }   # display name carries the lang axis
# slug = "…"           # route slug, defaults to the id

[records.course.dinner]               # ANY grouped field, not just tags
name = { en = "Dinner", fr = "Dîner" }
intro = { en = "These dinner recipes are sure to please!", fr = "…" }
```

The design, piece by piece:

- **The selector splits every row's path into (logical path, locale) at
  load** (`I18nCfg::split`, both selectors pinned by test). Rules, globs,
  route tokens, schema governance and theme rules all see the LOGICAL
  path — so `red-lentil-dal.fr.md` rides the same rule, the same
  `.schema.toml`, the same recipes theme (subtheme included) as its
  original, and lands at `/fr/recipes/red-lentil-dal/`: the default
  route, locale-prefixed. i18n off = the selector never fires = the main
  site is a byte-identical no-op (verified against the oracle).
- **A translation is a row, not a copy of the site.** Rows sharing a
  logical path pair through `by_logical` — the ONLY index that sees
  translations. `order`, `by_key`, `by_tag`, `by_year_month` and tree-view
  collection admit default-locale rows only, which makes every listing,
  feed, archive, section tree and embedding single-locale **in one place
  each**, not as N scattered filters.
- **The language switcher is a relations axis.** `translations` joins
  similar/earlier/later/linked-from: dateless neighbors labelled via
  `[i18n.names]`, both directions, zero fragment changes in any theme —
  the §6b axes design absorbing its fifth member. And **the visible
  switcher is theme CSS geometry**, not a mechanism: the relation
  fragment already stamps `data-axis`, so both example themes lift
  `.relation[data-axis="translations"]` out of the relations footer and
  absolute-position it as a chip in the document's top-right corner
  (label hidden, 🌐 prefix, `:has()` drops the footer rule when
  translations was the only axis). Reading order — and screen readers —
  keep it with its sibling axes; §5e's law held: a new UI affordance
  cost zero engine changes. One true fix fell out of building it:
  embedding similarity ranks within a locale now (a translation is the
  same text — it would top its original's Related list; pinned in
  `embed::rank`).
- **Enum records** (generalized from tag records at Matt's ask,
  2026-07): `[records.<field>.<id>]` declares the value domain of ANY
  grouped field — tags, courses, whatever a view groups by — with
  `slug` (route-facing, locale-independent, defaults to id), `name`
  (string or per-locale map; fallback locale → default → id), and
  `intro` (the value's own landing prose — q45 mode A per key: the
  course archive introduces the course, beating the view's intro for
  that route). The French note's pill reads *méta*; the French tag
  ARCHIVE is now titled *« méta »* too, because `{key}` in grouped
  titles/crumbs renders through the record's name at the route's
  locale — display wears the name, URLs wear the slug, keys and params
  keep the id. Slugging happens at one seam per base kind
  (`route_value`) and now covers every grouped field. The retired
  `[tags.id]` spelling is a load error naming the new form. A typo'd
  locale key in a record is a load error. The shape q40's structured
  records will extend.
- **Filters see the axis**: `locale` joined the post, page and route
  schemas (route: Null = default locale, passing `!=` by the Null rule).
  The example's search deliberately declares nothing about locale — French
  rows are searchable ("lentilles" finds the French dal), which is the
  right default for a bilingual site and one filter clause to change.

### Display names: one shape, one hierarchy *(built 2026-07; hierarchy Matt's)*

Every human-facing string a site emits resolves through one three-level
hierarchy — **inline beats global beats engine built-in**:

1. **Inline, at the site.** Any display-name position (view
   `title`/`crumb`, collection `crumb`, tag `name`) takes a
   **`LocalizedStr`**: a bare string, or a per-locale map —
   `crumb = { en = "Notes", fr = "Carnet" }`. Writing a value here IS
   the surgical override; nothing else consults the fallbacks.
2. **The global map, `[i18n.strings]`.** Two kinds of key live here:
   *engine vocabulary* keys override what the engine emits everywhere,
   and *user keys* are shared strings any site can pull in by
   **reference** — `title = "@tagged"` (`"@@…"` escapes a literal `@`).
   Declare a common string once, per locale; reference it all over;
   override inline at the one site that differs.
3. **Engine built-ins.** The engine's vocabulary is a closed, named set
   (`ENGINE_STRINGS`: `home`, `drafts`, `related`, `later`, `earlier`,
   `linked_from`, `translations`, `page` = `"Page {n}"`), each with its
   English default. Adding an engine string means adding it to the
   table — the closed set IS the inventory, so nothing can be emitted
   that can't be translated.

Load rules keep resolution total and typos loud: a per-locale map may
only name declared locales and must include the default; a `@reference`
must resolve (error naming every known key); global values are literal
(no reference chains); and a **non-engine global key nobody references
is an error** — which is precisely how a typo'd engine override
(`hom = …`) surfaces now that user keys are legal. Templates stay
templates — `{key}`/`{month_name}` placeholders render after the locale
resolves, so shared strings can carry them.

**Resolution locale is the row's locale** for row-scoped surfaces (axis
labels, trails — the French note's trail reads `Accueil › Carnet › 10`
with zero engine special-casing) and **the view's locale** for listing
surfaces — which is the default locale today, so the one seam where
locale-parallel listings plug in later is already marked in
`listing_title_and_trail`. Main site: no overrides, all bare strings →
builtins → byte-identical, verified.

**Honest edges, named now**: a localized POST's trail is complete —
`Accueil(→ /fr/) › Carnet(→ /fr/blog/) › 10 January 2026`: "Home" is
**existence-checked** — it links the locale's own homepage when a
translated index exists (`index.fr.html` → `/fr/`, which the example now
ships), else the site root; the collection-index
crumb locale-prefixes (the French index exists whenever French rows do),
and the inert date tail shows the whole date when the collection
declares no archive chain (a bare day only reads after year › month
crumbs; main site keeps its day tail, byte-identical). Localized tree
PAGES walk URL ancestors — the duplicate home crumb on `/fr/…` URLs is
**cured** (q45: `ancestors()` skips locale-prefix homes; Home is the
trail root's job), and a section crumb appears in French exactly when
the section's landing has a French variant (`index.fr.md` → the
claimed row's URL is `/fr/recipes/`). `collection.index` is still a
literal URL — `index` naming a VIEW remains q45's open (a), the
landing chain. `.slots/` fills localize by the same suffix convention
(`nav.fr.md` beside `nav.md` — built, and their view links resolve per
consuming page's locale, §6a).
`month_name` in group params is computed at route build, locale-free —
localizing it belongs to the locale-parallel-views work. The search
overlay's strings live in `/search.js` (client-side, engine asset) —
pending until search itself is locale-aware. `site.title` is not yet a
`LocalizedStr` (the shell renders one site title). **Locale-parallel views: built, and DEFAULT-ON** (forced when Matt asked
where a French note's tag pill should lead; flipped to opt-out on his
call — "the default locale sits above the selector, so /atom.xml and
/fr/atom.xml both fall out"). Every materializing **row-query** view —
grouped archives, paginated and plain listings, members-backed shells
like the feed — partitions per declared locale: that locale's rows, the
locale-prefixed route (default locale unprefixed), title/crumb/trail
resolved at the route's locale (the route carries `locale`, and
`listing_title_and_trail` reads it — the seam marked earlier, used on
schedule). A locale with no rows materializes **nothing**: the partition
is real, not mirrored — the example gets `/fr/blog/` ("Carnet"),
`/fr/atom.xml` (its own self-link, French entries only),
`/fr/blog/tags/meta/` and `/fr/courses/dinner/`, but no `/fr/books/`
(no French books) and no `/fr/photos/`. Opt-out is
`locales = "default"`; `"*"` states the default explicitly. Exempt by
design: **star views** never multiply (they query the finished route set
and filter on `locale` — one sitemap spans all locales), **object
views** carry no locale (declaring `locales` there is an error), and
**embedded views** follow their embedding page (pending). Per-locale
pagination totals count within their locale; the pagination producer's
q32-hardcoded routes gained the locale prefix. A monolingual site
declares no locales and the default is a proven no-op (byte oracle).
Also noticed: `pretty_date` is locale-free — "10 January 2026" on a
French page; localized date formatting is pending, probably as an
engine-strings-adjacent month-name table. Localized group *keys* (a French
course name) are q40-adjacent. `{% post_url %}` targets the physical
name, so a French body can cite `…hello-field-notes.fr`. The markers
walk uses physical paths — irrelevant for suffix, a known caveat for the
prefix selector, which is built and tested but not yet exercised by a
corpus.

## 7. Clients of the database

Both `build` and `serve` are one render path: `build::render_site` produces the
whole site as a `URL → bytes` map in memory, and the two clients differ only in
what they do with it — `build` writes it to disk, `serve` holds it resident.
(Verified: refactoring build onto the map left its disk output byte-identical
save the feed's build-timestamp.)

- **`grackle build`** — AOT materialization: render the map, write every entry
  to a directory. Feeds the existing `publish.sh` rsync unchanged. Templates
  parse once to ASTs per run.
- **`grackle serve`** — 🟡 **built (v1).** The database daemon: raw `hyper`
  (no axum, no TLS), the resident render map answering URL → bytes with no
  output directory. A `notify` watcher rebuilds the whole world on any content
  change (~0.3s) and bumps a version; an injected script polls it and reloads
  the browser. The snapshot lives in a `keepcalm` RCU cell (`SharedMut::new_rcu`):
  reads are lock-free clones that never contend with the writer, and the watcher
  swaps a whole new snapshot with `set` — which skips even the RCU write-copy —
  so a rebuild never blocks in-flight requests (verified: 20 concurrent reads
  through a rebuild, all 200). **v1 is coarse on purpose** — the design's target is
  URL → reverse-index → *render one page on demand* against an incrementally
  invalidated snapshot (single-digit ms per edit); today it re-renders
  everything (still sub-second) and polls rather than streaming SSE. Those are
  the §2 upgrades, not yet built.
- **`grackle query`** — REPL/CLI over the live DB (`urls`, `posts where
  tag=rust limit 5`, counts, `explain <url>` → row, deps, cache state).
  Doubles as the migration validator: compare counts/URL sets/tag sets
  against Jekyll output.
- **`grackle diff --against _site-prod`** — golden comparison: normalized
  HTML diff (whitespace/attribute-order-insensitive) per URL with a summary
  matrix (identical / equivalent / differs / missing). The iteration driver.

## 7a. The example site: the falsifier for site-independence *(started 2026-07)*

grackle has been developed against exactly one corpus, and §9b shows the
cost: `"blog"` hardcodes, view-name policy, and a phase-1 gate survive
*because nothing can contradict them*. The design already knows this
argument — a boundary with a single implementation is untestable, which is
why `light` exists (§5a) and why the null theme runs as a falsifier (§5e).
**A second site is the same move one level up**: the falsifier for
site-independence.

`grackle/example/` is that site — self-contained (own `grackle.toml`, own
theme, own `.slots/`, own `_cache/`), invisible to the main corpus (the
`grackle/**` exclude already covers it), built and served like any site:

```
grackle --config example/grackle.toml serve --port 8081
```

It is deliberately a **kitchen sink**: each section exists to force a
parked feature, in parallel rather than in sequence.

| section | exists to force |
|---|---|
| `photos/` (varied aspect ratios) | ✅ **forced it** — object views (§5 audit gaps 1–3), dimension facts (q26), CSS-columns masonry; still open here: variants (q24) |
| `manual/` (mdbook-style tree, `order:` in front matter) | ✅ **forced it** — §6e section trees, `.section` markers (q35 settled), q27 unlinked labels |
| long posts with `##` headings, `toc:` front matter | ✅ **forced it** — §6e page outlines |
| `recipes/` (typed front matter: servings, prep time, course) | ✅ **forced it** — `.schema.toml` validation (§5b), *and* a deliberately alien second theme selected by one rule default (`theme = "recipes"`): §5a's per-row themes, real. Still open: `group_by` over schema fields |
| `books/` (a book-of-the-month club) | ✅ **forced it** — tree views (`over = "pages"`, `match`, `order_by = "-month"` over a schema field), the `card`/`card_list` kinds (newest featured large), `hero` on document pages (q23: `cover:` beats `image:`), and cross-table embedding: the homepage shows latest posts, latest recipes, and the current book side by side — three tables, one query language |
| a second theme, minimal | ✅ partial themes proven twice over: the default grew fragments only as features needed them, and the recipes theme is shell + CSS styling *canonical* markup |

Two rules keep it honest:

1. **The example never gets special-cased engine code.** Anything it needs
   is a real feature or a real bug — the whole point is that its needs
   contradict the main site's assumptions. Day one already produced two
   contradictions on schedule: the posts collection must be *named*
   `blog` (the phase-1 gate in `views.rs`, §9b's accepted asymmetry, now
   with a corpus that objects), and a site without a theme directory
   should be the null theme by §5e's own words ("needs no directory at
   all") — the example sidesteps it by shipping a real minimal theme, but
   the gap is now demonstrable.
2. **It has no byte oracle, on purpose.** The main site is verified
   against Jekyll; the example is verified by the engine's own invariants
   (load-time constraints, the null-theme completeness falsifier, route
   collision checks) — which is exactly the discipline a *new* grackle
   site would live under, tested for the first time.

## 7b. The backtest: 36 real sites against the model *(surveyed 2026-07)*

Method: 12 parallel survey agents, each auditing 3 sites against a compact
model card — personal/systems/dev blogs, longform, linkblogs, food sites,
portfolios, docs, digital gardens, unusual-static, magazines/podcasts.
35/36 fetched (rachelbythebay blocks this egress; judged from known
structure). **90 reported misses: 14 structural, 33 moderate, 43 minor.**
Raw reports are in the session archive; what follows is the synthesis.

### The headline: the core model holds

Every blog-shaped site backtests cleanly — danluu, jvns, matklad,
macwright, simonwillison, seths, sive.rs, paulgraham, the linkblogs:
collections + route templates + views + part maps cover them without
strain, and several agents noted the archive/grouping/feed machinery maps
"textbook". Two reported misses were **false** — full-body listings (jvns
TIL, seths.blog) are exactly §6d's "no summary field in the chain = rows
ship whole", and prev/next navigation is the earlier/later relations axes
— which says the *model card under-communicated*, not that the model
missed. (One true triviality fell out: matklad's per-post "fix typo"
GitHub link wants the row's repo-relative source path as a document fact —
storage is literally git, the fact is free.)

### The gap clusters, ranked (→ new open questions)

1. **The link graph** (→ q38). Digital gardens live on backlinks
   (andymatuschak, maggieappleton, gwern) and the model's only
   cross-page signal is embedding similarity. But the shape is already
   built: scan bodies for internal links at load, invert, and backlinks
   are one more **relations axis** — the §6b axes design absorbing its
   first non-temporal, non-similarity member. Transclusion ("render row
   X inline here") is the harder half.
2. **Set-scoped computed fields** (→ q39). Meal plans rolling up
   ingredients across referenced recipes, subtree photo/day counts
   (paulstamatiou), calendar widgets with per-day counts, term indexes
   (diataxis): all *aggregation over a view's members*, where §5f fields
   are row-scoped. The expression language wants `count()`/`sum(field)`
   over member sets.
3. **Structured record fields** (→ q40). Ingredient lists with
   qty/unit/name/cost, podcast chapters with time+label, schema.org
   Recipe emission: `.schema.toml` stops at scalar-and-list; a
   list-of-records type feeds all three.
4. **i18n** (→ q41). docs.astro (locale-prefixed parallel trees) and
   solar.lowtechmagazine (12 languages with cross-links): no
   translation axis exists on rows. The one classic SSG feature the
   model simply lacks.
5. **Client-side faceted filtering** (→ q42). Recipe sites and gardens
   combine facets at request time (diet × cuisine × season); declared
   views can't enumerate the combinations. The architecture already
   exists in miniature: ship a facet index the way `/search.bin` ships
   tf-idf, filter in the client — a *client-side view*.
6. **Media beyond image** (→ q43). Audio/video field types (sive.rs's
   250 interviews, two podcast sites), RSS enclosures, srcset/multi-
   format renditions (fasterthanli.me), externally-hosted originals
   (macwright's CDN): the §6b image pipeline generalized.
7. **Per-row scoped assets** — ciechanowski's per-article JS/CSS pairs
   are §5b's unbuilt `.style.scss` leg plus its obvious script sibling;
   already specced in shape. The *interactive-widget* half of
   ciechanowski (stateful WebGL islands as the site's identity) stays
   honestly out: raw HTML passthrough + per-row assets carries the
   delivery, the engine never models the widget.
8. **External/live data** — trending ranks, HN counts, live solar
   charge: not expressible from a git tree, and the honest answer is an
   ETL that *writes* git-tracked data before build (order_by then works
   on it). Kottke's "vintage post today" is the benign case: a
   date-seeded deterministic pick is fine for a daily build. Noted, not
   questioned — the model's answer is "commit the data".

### The confirmed non-goal, sized

The single biggest real-world cluster — **memberships, paywalls,
comments, ratings** (waitbutwhy's store/forum, craigmod's and atp.fm's
memberships, 404media's gated bodies, every recipe site's reviews) — is
the dynamic-server non-goal, now measured: it is what most *monetized*
sites add atop exactly the static core grackle models. The design keeps
the line: entitlements are an edge/CDN concern layered over static
output; user-generated content is an external embed. gwern's URL-keyed
annotation database and hover-transclusion remain the one genuinely
sui-generis structural outlier surveyed.

## 8. Known-inexact from day one (accepted, iterate later)

| Area | Why | Plan |
|---|---|---|
| Code highlighting spans | Rouge ≠ syntect token boundaries/classes | 🟡 **half done.** Wrapper divs + inline-code classes emitted via the AST pass (§9a) — rouge-cause diffs 45 → 1. Still missing: Rouge's pygments token spans (`<span class="c1">`) for the ~12% of blocks with a real language. Under-measured: 4 of 6 highlighted posts are liquid-skipped (§8c) |
| kramdown edge syntax | IALs `{:.x}`, `markdown="1"`, footnote markup | comrak `smart` + extensions first; triage real diffs per-post; hand-normalize stubborn 20-year-old posts. **`markdown="1"` found in the wild** (2 posts, the callout boxes): comrak drops the `<div>` into a `<p>` and the box collapses; one post hand-normalised, the other left raw as the widget's test fixture, because `{% callout %}` widgets (§5d, open q29) are the real fix — they retire the raw-HTML idiom entirely |
| Related posts | LSI is unreproducible *and* unwanted | **Superseded** (§6b): embeddings replace it outright. Deliberately not equivalent — this is an improvement, and `diff` can't judge relatedness anyway |
| Feed body HTML | `feed_images`/`expand_urls` operate on rendered HTML | ✅ **done** (regex port, §render). `expand_urls` makes root-relative `href`/`src` absolute; `feed_images` injects `align`/`width` on float images — both byte-verified against the reference. `<content>` bodies still carry the markdown gap (§8c), feed-only, low stakes |

## 8a. The markdown gap, measured (phase 2a)

> ⚠️ **Superseded by §8c.** The 90.7% below was measured against a reference
> built with syntax highlighting *disabled*, five days before the config turned
> Rouge on. It was two builds agreeing by accident, not accuracy. The honest
> number against a rebuilt reference is **90.0%** — the same figure, arrived at
> legitimately, after implementing the Rouge shapes the stale reference had
> hidden the need for. **The method below is sound and worth reading; the
> headline is not.**

The kramdown→comrak gap was the one risk that could sink the port. It is now a
number rather than a worry.

**Method.** 225 posts that are both liquid-free *and* untouched since the
reference build (`git log --since` — the June 27 commit rewrote titles and
descriptions, so a naive comparison would have measured content drift and
blamed comrak). comrak configured to kramdown's defaults: `auto_ids`,
smartypants, tables, strikethrough, footnotes, description lists, raw HTML
passthrough. Normalisation folds only invisible differences: whitespace,
entity spellings, self-closing style.

| verdict | n | |
|---|---|---|
| Identical | 19 | byte-identical |
| Equivalent | 185 | after normalisation |
| **Differs** | **21** | 9.3% |
| | **204 / 225** | **90.7% usable** |

**Of the 21, five differ *only* in curly-quote choice** → 92.9% once that is
handled. The two systematic causes:

1. **Decade abbreviations.** kramdown renders `'95` with an *opening* quote
   (`‘95`); comrak with an apostrophe (`’95`). comrak is typographically
   right — `'95` is an elision — but kramdown is the target, and a corpus that
   opens in 1998 says "Windows ‘95" a lot. Fixable in an AST/text pass.
2. **Raw HTML in prose.** A literal `<solution>` written as text: kramdown
   auto-closes it (`<solution></solution>`), comrak leaves it open.

**Read:** the gap is a handful of characterisable patterns, not 225 unique
problems — "fix a few things", not "the last 1% costs more than the first 99%".
§8's plan stands, and the pessimistic option (keep kramdown behind a Ruby
shim) is not needed.

**Two harness bugs worth remembering**, both of which made the first run report
a meaningless 100% differ:

- `extract_body` searched from `<article>` to end-of-file and took the *last*
  `</section>` — which belongs to the outer `<section class="content">`, so it
  swallowed the whole page. The unit test passed only because the test itself
  pre-sliced the input to `</article>`: **a test that hid the bug it was meant
  to catch.** It now feeds the real page shape.
- The reference layout appended `<a class="fullpost">` *inside* the body
  section. Layout chrome, never markdown output — counting it as a markdown
  difference is a category error.

Both were found by looking at an actual delta rather than trusting the tally.
A 100%-fail result is a harness bug until proven otherwise.

## 8b. What the first render pass found

**`_site-prod` is a pre-redesign artifact and is useless as a *visual*
reference.** It renders the old design entirely — `grack.com` top-left, a
sidebar, a narrow column. The layouts *and* the CSS changed this month. It
remains valid for the §8a body diff (markdown→HTML is layout-independent, and
the 225-post set was filtered to untouched files), but any visual or chrome
comparison must go against the **live site**.

**`{% post_url %}` takes `dir/stem`, not a bare stem.** All 51 uses are
`2009/2009-07-28-a-quieter-window-name-transport-for-ie` — because posts live
in year subdirectories. `by_name` is keyed on the collection-relative path
minus extension (§3). Caught immediately: the first build failed with a
dangling-reference error naming the file, which is the §4 constraint
philosophy working.

**`grass` rejects a nested `@import` that libsass accepts.** `_sass/_post.scss:240`
has `pre > code { @import "rouge"; }` — scoping Rouge's syntax classes by
nesting. libsass (what Jekyll uses) allows it; grass errors with "this at-rule
is not allowed here". The site is legal input that grass will not take. Fixed
by resolving `@import` textually before handing grass the flattened source, so
the site's sass is untouched. grass's "dart-sass-compatible" reputation
needs this caveat.

**grass and sassc agree.** 2232 selectors vs the live build's 2231 — a
one-rule formatting difference, not a semantic one.

**The document margin has no `<time>`.** On a full post the date is carried by
the breadcrumb trail (`… > 2022 December > 16`); `post-date` belongs to the
*summary* layout only. Adding one was a real diff against live, found by
diffing markup rather than by looking at the page.

**"Skip pages containing liquid" was the wrong test — expand first, then
decide.** 18 pages were skipped on a bare `contains("{%")` check. Measured, 17
of them use *only* `{% image %}` (72), `{{ site.baseurl }}` (9) and
`{% post_url %}` (5) — all already handled. Expanding first and skipping only
on a *surviving* unknown construct took it to **1**. The output now contains
zero unexpanded liquid.

Only `/` genuinely needs more: `assign`/`for`/`if`/`unless`/`include` plus a
`| plus:` filter, to render "latest 3 posts". Note what that block *is* — a
query over posts (limit 3, `!hidden && !draft`), re-derived in a template
exactly like the three listings were (§5a). It is a **view** wanting to be
embedded in a `raw` page, not a reason to implement liquid.

**`document` needs two shapes, and the theme is why.** Pages are not posts
structurally: the theme styles `.post:not(.post-summary)` as a two-column
margin layout, and `.page` as a single column with breadcrumbs *above* the
article and no `post-full` wrapper at all. Reusing the post shape made the page
header 800px against live's 640. §5a claimed one `document` kind because the
*relations* unify (date→neighbours, tree→ancestors); the structure does not,
unless the theme changes too. Recorded as a real tension rather than papered
over.

**The theme has no dark mode.** Zero `prefers-color-scheme` rules, and
`html`/`body` set no background, so the page relies on the browser's default
canvas while `.post-header` is explicitly white. In a dark viewport that
becomes a white band on a dark page with black text. It affects the live site
identically — this is a property of the theme, not of grackle — but it is
worth knowing.

### Chrome gaps still open (unmeasured by construction)

`diff` compares post **bodies**, so none of this is caught by any number we
quote. §5a said chrome parity was not required — but "not required" and "not
noticed" have quietly become the same thing. Against `_layouts/default.html`,
`render::default_shell` is missing:

- the **footer About block** (`{% if page.hide_sidebar != true %}`): profile
  image + `{% include social.html %}`. So social links are absent site-wide
  except on `/`, which renders them inline.
- the **search nav item** (`<h2 class="smaller">` → swiftype launcher).
- `{{ site.time | date: '%y' }}` in the copyright — we hardcode 2026.
- the swiftype `<meta>` and several `apple-touch-icon` sizes.

`hide_sidebar: true` in `/`'s front matter is consequently inert: it exists to
stop the footer About duplicating `/`'s own Connect block, and we render
neither. It is a schema fact (§5d) with nothing to gate yet.

## 8c. The reference build lied by 17 points

The single most important measurement lesson of the project, and it very nearly
went unnoticed.

§8a's headline — **90.7% usable** — was an artifact. `_site-prod` was built
June 2. Five days later:

```
6437c22  2026-06-05  Code formatting
-  syntax_highlighter: nil
+  syntax_highlighter: rouge
+  syntax_highlighter_opts:
+    default_lang: text
```

The reference was built with **highlighting switched off**. It emitted bare
`<pre><code>` — which is exactly what our comrak emitted. We were not close; we
were two builds agreeing because both had Rouge disabled. Rebuilding the
reference against the *current* config:

| | stale (Jun 2) | fresh | + rouge pass |
|---|---|---|---|
| usable | **90.0%** | **72.6%** | **90.0%** |
| rouge-cause diffs | — | 45 | **1** |

Our output never changed. Only the yardstick did. And the final 90.0% landing on
the original 90.0% is a **coincidence** — the first was luck, the second is
earned.

**The rules this buys:**

1. **A reference build is an input, and inputs have versions.** It must be
   rebuilt from the *current* config before any number derived from it is
   quoted, or the number is about a site that no longer exists.
2. **Agreement is not evidence unless it can disagree.** Both the 90.7% and the
   later `latest` check (§5c) matched for reasons unrelated to correctness. A
   test that cannot fail is not measuring.
3. **`classify_cause` over-attributes.** It is a ±window keyword heuristic:
   `identd` was filed under "link" when the actual delta was `‘95` vs `’95`,
   because a link happened to be nearby. Read deltas, not tallies.
4. **`_site-prod` can no longer be regenerated** (§5c): `{% view %}` is not
   Liquid, so Jekyll fails the whole build. `git stash push index.html` first.
   Given the above, this is a real cost, not a footnote.

### The gap is parser-side, and that decides the renderer question

With the reference honest, the residue is:

```
    10  inline / prose      5  list      4  link      3  table      1  code block
```

Spot-checked, these are **parse**-stage, not render-stage:

- `Windows ‘95` vs `’95` — smartypants, applied into Text nodes by
  `o.parse.smart`. comrak is typographically *right* (the apostrophe elides
  "19") and we want to match kramdown being wrong. No renderer touches this.
- `<li>text</li>` vs `<li><p>text</p></li>` — kramdown decides looseness **per
  item**, CommonMark per **list**, so comrak `<p>`-wraps all three items in a
  list where kramdown wrapped one. A dialect difference.
- At least one "differs" is not a bug at all: `deriving-a-bit-twiddling-hack` is
  missing a whole *Thanks to…* paragraph in the **reference**, which is stale
  relative to the post.

**Zero heading, zero footnote, zero image diffs** — the four node types we have
opinions about are not where we lose. The 90/92% ceiling is a *parser* ceiling.
If we ever chase it, the fork is comrak's parser, not its formatter (→ §9a).

### The 97-post blind spot

`diff` skips 97 of 327 posts as "body contains liquid". Many are **false
positives**: `{{ github.event.issue.number }}` and `{{ secrets.DEVICE_NAME }}`
in the bluetooth posts are GitHub Actions expressions inside code samples, not
Liquid. So 30% of the corpus is unmeasured, and the 90% is computed over an
unrepresentative 230. Worth tightening the skip predicate before trusting the
number further — and note this same blind spot hides the highlighting gap: only
6 posts use real-language fences, and **4 of them are liquid-skipped**, so the
"1 remaining rouge diff" is 1 of 2 compared, not 1 of 6.

## 9. Crate layout *(as built; the original sketch is in git history)*

A cargo workspace: the engine, plus the search core split out so the same
code compiles to wasm (§6b).

```
grackle/                     workspace root = the engine binary
  src/
    main.rs      CLI (query / export / build / serve / routes / diff)
    config.rs    grackle.toml; view composition semantics (query/chain/
                 group_specs/fields_for — the ONE over-chain walker)
    store.rs     FsStore: front-matter split, tree walk, .gitignore law (§4c)
    markers.rs   marker scan + nearest-wins defaults (§4b)
    route.rs     filename formats, route templates, template params
    db.rs        tables, rows, indexes, routes, load-time constraints (§3, §4)
    views.rs     views become routes: row sets, grouping, subdivision (§5, §5c)
    filter.rs    the typed predicate language (§5); q31 grows it into
                 the expression language
    tags.rs      the {% %} expander: image/post_url/view/include + widgets
    markdown.rs  comrak-as-kramdown; Doc = whole + blocks (§6d)
    parts.rs     part maps: typed schemas, producers, canonical() (§5e)
    binder.rs    fragment parser + hole algebra + load-time checks (§5e)
    slots.rs     .slots/ tree fills, block-arity rule (§5e)
    theme.rs     theme = fragments + fills; shell assembly
    render.rs    head facts, escaping, light shell, feed/sitemap XML
    embed.rs     embeddings cache + related-posts ranking (§6b)
    thumbs.rs    content-addressed thumbnails (§6b)
    build.rs     render_site: the passes; build = write map to disk (§7)
    serve.rs     resident snapshot behind raw hyper; watcher; live reload
    diff.rs      golden comparison vs the Jekyll reference (§8)
  search-core/   stem/tokenize/index/rank — used by build AND the browser
  search-wasm/   the same core behind a raw no-bindgen wasm ABI (§6b)
```

~8.3k lines in the engine + ~450 in the search crates (vs the ~3–4k
ballparked before §5e/§6b/§6d existed). The sketch this replaced imagined
`store/watch/snapshot` and `db/{posts,tree,views}` submodule trees, a
`render/liquid.rs`, and axum+SSE serving; reality is flatter, liquid never
happened (§5d), and serve is raw hyper with polling (§7).

## 9a. Dependencies: the inventory is `Cargo.toml`, this doc keeps decisions

This section used to carry a per-crate table with versions and health notes.
It rotted exactly the way §9b says shadow copies do — it still listed `axum`
after raw hyper replaced it, and `lol_html`/`syntect` before they were
dependencies at all — so the inventory is **removed for good (2026-07)**:
what is depended on is answered by `Cargo.toml` alone. What stays here are
the *decisions*, which don't have version numbers:

- **No template engine.** `liquid` was this section's biggest listed risk;
  retired by not taking it — the site measured out at ~3 real templating
  constructs, all Rust components (§5d).
- **No expression engine.** The config language is hand-rolled against a
  CEL grammar contract; a CEL crate is the recorded contingency, not a
  dependency (§5f).
- **comrak over pulldown-cmark.** The mutable AST is load-bearing — the
  Rouge code-block shapes, and §6d's block split, both live there (below).
- **No vector index, no rust-bert.** 327 vectors is a brute-force dot
  product in microseconds; and embeddings run on ONNX (`fastembed`), not
  libtorch — rationale with the measurements in §6b.
- **Raw hyper, no axum**, with a `keepcalm` RCU cell for the resident
  snapshot; no SSE, live reload is a poll (§7).
- **`ignore` is load-bearing, not convenience.** The marker scan has no
  other defence against `_site*`/`vendor` and costs 205ms without it (§4c).
- **`lol_html` is deferred with its consumers.** The two shipped HTML
  rewrites (`expand_urls`/`feed_images`) are small regexes; the
  selector-driven stage arrives with §6d stage B.
- **`salsa` declined** — hand-rolled typed invalidation keys suffice at 327
  posts (open question 1).

The bar for a new dependency: taken for a measured reason, and recorded
here only when the decision itself is interesting.

### Why we do **not** write our own AST → HTML renderer

The tempting conclusion from §6d (footnotes) and §8 (Rouge shapes) is to own the
formatter. The measurements say no.

**The fidelity argument fails.** §8c: the residual gap is parse-stage —
smartypants, tight-vs-loose lists, kramdown table syntax — with **zero heading,
footnote, or image diffs**. A renderer moves it by approximately nothing. The
90/92% ceiling is a parser ceiling; if we ever chase it, we fork the *parser*.

**The control argument is satisfied more cheaply.** Everything we want is
reachable without owning the 1,861 lines of `html.rs`:

| want | mechanism |
|---|---|
| Rouge-shaped code blocks | AST → `HtmlBlock` (below) |
| headings without comrak's injected `<a class="anchor">` | `HeadingAdapter` — we own enter/exit |
| footnotes as sidenotes | partition root children + format the definition's *children* (§6d) |
| bare-name `img`/`iframe` (§6a) | mutate `NodeLink`/`NodeImage.url` in place |
| `<style>` extraction (§6c) | detach the `HtmlBlock` |
| **anything else, total control** | replace the node with `NodeValue::HtmlInline`/`HtmlBlock` |

That last row is the escape hatch: `render.unsafe_` is already on for 20 years of
hand-written HTML, so raw nodes pass through verbatim. It is "own the renderer"
**per node type, incrementally, at zero upfront cost**, while comrak keeps
handling escaping, `escape_href`, entities, tight/loose lists and table edges for
the ~15 node types where we have no opinion. Our opinions are concentrated in
four of ~20: code blocks, headings, footnotes, images.

**The tripwire:** if the escape-hatch list ever exceeds ~⅓ of node types, we have
written the renderer accidentally and badly, and should write it deliberately.

### Neither code-block adapter fits (measured)

Worth recording, because the obvious answer is wrong:

- **`CodefenceRendererAdapter`** is a `HashMap` keyed by language and only fires
  when the info string is non-empty (`html.rs:513`). The corpus is overwhelmingly
  **indented** code — only **7 posts use fences at all** — and indented blocks
  have `info == ""`. It would never fire where it matters.
- **`SyntaxHighlighterAdapter`** *does* fire for empty info and could open
  Jekyll's two wrapper divs, but comrak then hardcodes `</code></pre>`
  (`html.rs:566`) with no hook to close them.

So code blocks became the first real use of the AST escape hatch. Two details the
reference caught that reading would not have:

1. **Inline code needs it too** — `<code class="language-text highlighter-rouge">`
   on every backtick span (1,732 of them). But a hand-written `<code>` inside a
   `<td>` or `<a>` stays bare, because kramdown passes raw HTML through. comrak
   models those as *different nodes* (`Code` vs `HtmlInline`), so mutating
   `NodeValue::Code` hits exactly the right set for free.
2. **Rouge does not escape quotes.** It escapes `&`, `<`, `>` only; comrak also
   emits `&quot;`. Reusing comrak's escaper would diff every code block
   containing a double quote — and this corpus is full of
   `link_section = "..."`.

### What §6d's blocks change here

`lol_html` shrinks. §6d wanted it to unify five ad-hoc HTML passes; three of the
five (img/iframe, `<style>`, code shape) happen **at the node**, before HTML
exists — no re-parse, no selector matching. `lol_html` drops back to what it is
actually good at: user-authored `.rewrite.toml` rules over rendered output.

## 9b. Seams audit: is responsibility still split right? *(2026-07)*

Taken after the dedup refactor (one chain walker in `config.rs`, `views.rs`
split out of `db.rs`, one front-matter splitter, `Route::new`), with the
whole codebase freshly re-read. Verdict first: **the load-bearing boundaries
hold, and the leaks that exist are all one disease.**

### What holds

The pipeline's layer per module is real: config *declares*, `db` *resolves
and constrains*, `views` *materializes*, `parts` *produce*, binder + theme
*arrange*, CSS does *geometry*, `build` *orchestrates*, `serve` *hosts*.
The crisp tests all still pass on inspection: producers never see `Site`,
no theme can compute, no fragment can loop, and the recent features entered
without bending anything — relations-as-axes arrived as one more producer
push (no schema, binder or theme change), and summaries arrived as a config
field flowing through `over` (no rendering attribute, no engine policy).
The predictor of this health is the discipline itself: **everything
declared is load-checked** (filters, fragments, fields, widgets, slots), so
a responsibility placed in config *stays* there — code can't quietly
reinterpret what it would first have to type-check.

### The one recurring disease

§5c named it: *the config declared `filter`/`group_by`/`paginate` and the
renderer ignored all of it*. That was cured for row membership (`members`).
The same disease survives in four smaller pockets — each is the renderer
(or a producer) re-deriving something config already owns:

1. ~~Producers hardcode routes config owns~~ (→ q32) — **cured
   (2026-07)**: pagination takes URLs rendered from the owning view's
   `routes` templates, and tag pills render `Config::tag_url` from the
   declared-or-unique tags view's template (no tags view = unlinked
   pills). The i18n work is what finally forced it: the hardcodes had
   grown locale prefixes in two places before the cure.
2. **`build.rs` holds policy keyed on view names** (→ q33). Three spots:
   `view != "blog_index"` decides which listings get `noindex`;
   `"blog_index" => Some("blog_index")` supplies a fallback layout; the
   feed pass selects its view by `template == "atom.xml"` — a
   serialization chosen by template *filename*. This is §5a's "the shell
   knows about everything," recurring in miniature in the orchestrator.
   `noindex` wants to be a view attribute (a schema fact like every
   other); serializations want to be declared layout kinds
   (`layout = "feed"`), not filename matches.
3. **The sitemap predicate evaluates twice** (→ q33, same family).
   `views::build_star_views` parses and runs the filter to *count* rows;
   `build.rs` re-parses and re-runs it to *enumerate* them. One source
   string, so they cannot disagree today — but two evaluation sites is
   precisely how Jekyll's three listing filters drifted (§5a). Star routes
   should carry their members like every other route.
4. **Three definitions of "not content"** (→ q34). §4c legislated the
   three layers (gitignore + dot/underscore skip + `exclude`) — for the
   tree walk. But `slots.rs` carries a private `SKIP` list duplicating
   half of `grackle.toml`'s `exclude`, and `serve.rs::is_content` carries
   a third. Add an exclude to config today and the watcher still rebuilds
   on it and the slots walk still descends it. Both walks should derive
   from the §4c layers.

None of these is urgent — every one is invisible until a config value
changes out from under its shadow copy — but that is also the §5c lesson:
the drift is only ever invisible *until* it isn't.

### Accepted asymmetries, named so they don't read as leaks

- ~~`if q.base != "blog"` in `views.rs`~~ — **dead (2026-07)**: object views
  forced kind-based dispatch, and the example site's posts collection is
  named `notes` to keep it dead.
- ~~`post_trail` hardcodes `"blog"`~~ — **generalized (2026-07)**: the posts
  collection is found by kind. Still single-posts-table; a second posts
  collection remains future work.
- ~~`themes/default` hardcoded~~ — **dissolved (2026-07)**: `themes/*` is a
  registry, theme is per row (`theme:` front matter or rule default), each
  theme compiles its own stylesheet, and a site with no themes directory
  is the null theme. What remains: `default` as the conventional default
  name, and search assets living in the default theme.
- The CLI's `query search` indexes raw markdown where build indexes
  rendered HTML — documented at `search_docs`; a deliberately cheap smoke
  query, not an inconsistency to fix.
- `render.rs` has become "head facts + escaping + XML serializations" —
  its doc admits it. If stage B touches the feed anyway, the
  serializations can move out; renaming for its own sake isn't worth a
  commit.

## 10. Phasing (each phase has a checkable exit)

| Phase | Deliverable | Exit criterion |
|---|---|---|
| 0 | FsStore + posts table + `query` | ✅ **done** — 327 rows; URL set matches the Jekyll sitemap exactly (325 shared + the 2 posts published after that sitemap was built); loads in **~3.5ms warm / ~11ms cold**, vs a 200ms budget. Snapshots/watcher deferred to phase 3, where they're actually exercised. |
| 1 | route mapping: all tables routed, `export` (JSON), `routes` (tree) | ✅ **done** — 1559 routes across posts/pages/objects/views; **every one of the 556 Jekyll sitemap URLs is routed** (0 missing); the 1003 extras are 983 assets jekyll-sitemap never lists + 16 routes explained by the reference build being stale. Loads in ~10ms. |
| **2a** | markdown-gap spike + `diff` | ✅ **done — the port is viable.** ~~90.7%~~ → **90.0% against an honest reference** (§8c): the original figure was measured against a build with highlighting disabled and was luck, not accuracy. 230 posts: 20 identical, 187 equivalent, 23 differ; 92.2% if smartypants is matched. The residue is parser-side. **Caveat: 97 of 327 posts are skipped as "contains liquid", many falsely** (§8c). |
| 2b | render pipeline: §5a layers end-to-end | 🟢 **renders** — 327 posts + 164 listings (with **pagination nav**, §5d) + **40/40 pages** + 1025 assets + **260 thumbnails** + **feed + sitemap** in **~0.4s warm** (Jekyll: ~38s). All layout kinds and both themes work; post and page chrome byte-identical to live; **zero skipped pages**. Remaining: highlighting token spans (accepted-inexact §8) and the chrome gaps below — both deferred into the §5e presentation rewrite. |
| 3 | ~~feed~~ + ~~sitemap~~ + ~~scss~~ + ~~thumbnails~~ + ~~static passthrough~~ | 🟢 **substantially done.** `atom.xml` (20 newest; `expand_urls`/`feed_images`/CDATA transforms; entry set byte-identical to reference), `sitemap.xml` (573 URLs, byte-identical set, post-date lastmods; mtime noise dropped, §4a), scss (§8b), and **thumbnails**: 260 derived images (same count as the reference `_thumbs/`) in a content-addressed `_cache/thumbs/` published at `/static/{hash}.{ext}` (§6b) — 25.3 MB of sources → 9.0 MB shipped, cold build 2.5s / warm 0.4s. Remaining: `linklint`, and the `_thumbs`-filename-identity criterion is **superseded** by §11.12 (`/static/` by design). |
| 4 | `serve`: resident db + live reload | 🟡 **v1 done** — raw `hyper` (no axum, no TLS), the `SiteDb` + rendered output held resident in memory, served with no output dir. A `notify` watcher **rebuilds the whole world** on any content change (~0.3s), bumping a version a poll-based injected script watches to reload the browser. Measured: edit → live reload in well under a second, verified both directions. `_cache/` is excluded from the watch so thumbnail writes don't self-trigger. **Deferred:** §2's incremental invalidation (rebuild only affected pages), SSE (polling suffices for one browser), and `explain`-shows-invalidations. |
| 5 | exactness iteration | `diff` matrix: no visually meaningful "differs" |
| **6** | §5e presentation synthesis | 🟡 **steps 1–3 done** — part maps (`parts.rs`, typed schemas, canonical order); the fragment binder (`binder.rs`, four-rule hole algebra, everything load-checked); **`themes/default/` is real**: shell + ten kind fragments + `theme.scss`, legacy composer deleted, `_sass` superseded. Verified as priced: **bodies by machine** (327/327 post content regions byte-identical across the cut), chrome by eye (posts/listings/pagination/tree/`/`, phone, light+dark). Dark mode = one `prefers-color-scheme` block. `.slots/` identity fills live (nav + copyright, block-arity rule exercised). Trails are §5c provenance walks — every archive level clickable. Also under the oracle en route: yearly archives (+16 routes), subdivision, `title`/`crumb` config templates. **Step 4 done**: `parts::canonical()` + fragment-lookup fallback — themes are partial by construction, a fragmentless theme IS the null theme; `PartType::Url` makes it navigable; the completeness falsifier runs over every real row on every `cargo test`. **§5e complete.** (Dark mode: proven as one CSS block, then removed — content assumes white; §5e step-3 notes.) |
| **8** | §6b content intelligence: embeddings + search | ✅ **done** — embeddings (fastembed/MiniLM, `[related]` policy, relations-as-axes, stale-while-revalidate in serve; **LSI retired**) and TF-IDF search (`search-core` shared by build and browser via `search-wasm`; `/search.bin` 195 KB/22ms; lazy `/search.js`; **Swiftype retired**). The Jekyll build's last two external services are gone. |
| **7** | content intelligence: §6d blocks | 🟡 **stage A done** — `render_doc` (one parse: whole + blocks), **the summary is a computed field** (`[views.published.fields.summary] truncate = {…}` — a derived column inheriting along `over`, nearest wins; `Doc::truncate` is mechanism, the deriver validated at load; `hero`/`lede` (q23) are future derivers), `truncated` fact → `data-truncated` → ★ (q17 settled), nth-of-type CSS deleted, concat==whole pinned as a corpus test (footnote post the sole exception). **Measured: `/blog/` 160→15.7 KB; `/blog/tags/rust/` 180→11.3 KB (93.8%).** Post pages and feed byte-identical; the double render is gone. Stage B: notes stream + sidenotes (q18), the `lol_html` rewrite stage; also `{% callout %}` widgets landed (q29). |

## 11. Open questions (to iterate on)

1. **Dependency tracking**: hand-rolled typed invalidation keys (as specced)
   vs `salsa` for automatic fine-grained tracking. Leaning hand-rolled —
   at this scale precision bugs are cheaper than framework complexity.
2. **Row version**: content hash (correct, rehash on every event) vs
   mtime+size (fast, near-correct) vs mtime-then-hash pre-check (specced).
3. **Config**: fresh `grackle.toml` (specced) vs also reading `_config.yml`
   site vars during migration.
4. **Highlighting fidelity**: coarse Rouge-class mapping (keeps `_rouge.scss`)
   vs adopting syntect classes + regenerating the CSS once. **Half-settled**:
   the wrapper/inline-code shape is done and exact (§9a); only the token spans
   remain, and §8c warns the gap is under-measured (4 of 6 highlighted posts
   are liquid-skipped, so "1 diff" is 1 of 2 compared).
5. **Page tree source**: explicit include list vs inheriting Jekyll's exclude
   list. Explicit is more database-y (schema declared, not inferred).
6. **Drafts**: replicate `_drafts` preview in `serve` from day one, or post-
   phase-3.
7. ~~`_hidden/` +14 URLs~~ — **settled** (§4a): profiles gate
   materialization; the public build emits nothing new, `/hidden/` is a
   drafts-profile view. URL parity holds.
8. ~~Adjacency gap vs skip~~ — **settled** (§4a): hidden rows don't exist in
   the public profile, so no hidden neighbour can ever be rendered.
9. ~~Colocate assets so name resolution works~~ — **settled** (§6a): buckets
   make bare names resolve for posts with no restructuring, and bubbling
   already matches how `code/legacy/*` is organised. Page bundles
   (`posts/2022/foo/{index.md,image.png}`) remain *optional* — they'd let
   new posts carry their assets side-by-side, and the two-phase rule already
   supports it the day you want it, one post at a time. No migration, no
   `_thumbs` churn.
9a. **`screenshot5/6.png`** — the only genuine collisions (`assets/2003/07/`
    vs `assets/2004/01/`, both inside the root bucket). Bare refs error; the
    path refs that exist today are fine, so this needs no action. Note the
    positional design supplies its own escape hatch if it ever matters:
    drop an `assets/` dir nearer those posts and the nearer bucket wins — no
    config, no interpolation machinery. Recommend: leave it.
10. **`noindex` the drafts profile?** `grack.com/drafts/` is publicly
    reachable today (unlinked, but rsync'd and crawlable). Given the
    canonical/indexing work, the drafts profile should probably force
    `noindex` — and with `/hidden/` landing there, that goes from hygiene to
    important.
11. **Iframe policy**: §6a resolves and rewrites `<iframe src>` for bare
    names but doesn't thumbnail. Do iframes need any sandbox/loading
    attributes injected by the same pass, or is passthrough correct?
12. ~~`static.dir` vs URL parity~~ — **settled**: Google Images isn't a
    concern, so derived assets move to `/static/{hash}{ext}` (§6b). URL
    parity remains a hard requirement for **pages**; derived assets are
    explicitly exempt and `diff` scopes its URL-set check to routable rows.
13. **Embedding model pinning.** `all-MiniLM-L6-v2` output is model-version
    dependent, so the cache key should include a model identifier
    (`_cache/embed/{model}/`, as specced) — but should a model upgrade
    silently re-embed all 327 posts on next build, or require an explicit
    `grackle reindex`? Silent is friendlier; explicit is more predictable.
14. **`<style>` auto-scoping default (§6c).** Scoping fixes a real latent
    leak on `body.multipost` index pages, but it's a behavior change on the 3
    existing posts. Default-on with `style_scope: false` opt-out (specced),
    or default-off and opt in per post?
15. ~~Template language: liquid crate vs slots vs Rust layouts~~ — **settled**
    (§5d): measured ~60 liquid constructs; only 3 are real templating, and all
    3 are already Rust components. Rule: *a template may not contain control
    flow.* The `liquid` dependency — §9a's biggest listed risk — is retired by
    not taking it. Slots turn out to already exist unnamed
    (`listing(..., pagination)`), which may mean §5b's slot system never gets
    built.
16. ~~Write our own AST → HTML renderer?~~ — **settled: no** (§9a). The
    fidelity case fails (the gap is parser-side, §8c) and every control need is
    met by AST mutation + `HtmlBlock`/`HtmlInline` escape hatches, per node type,
    incrementally. **Tripwire**: revisit if the escape-hatch list exceeds ~⅓ of
    node types.
17. ~~The ★ (§6d)~~ — **settled, signed off, built** (§6d stage A): truncation
    is a `truncated` Flag on the summary part map, stamped `data-truncated`
    by the binder, star gated in theme CSS — below the fade mask so the mask
    can never swallow it. It marks exactly the genuinely-cut summaries for
    the first time (all 5 on `/blog/` page 1; 5 of 10 in the 1998 archive).
18. **Sidenotes need a third grid column (§6d).** The post grid is
    `8.75rem | content` with the content escaping leftward — there is no right
    margin to render notes into. Is the theme change worth it, or do footnotes
    stay endnotes and the two-stream model just buy us the exact `concat` and
    the dead-anchor fix? **Under §5e this stops being an engine question**:
    the `notes` stream is placed by whichever theme claims it, with endnotes
    as the canonical fallback — a per-theme decision, not a design fork.
19. ~~Profiles vs a route-level fix for the sitemap leak (§4a).~~ — **route-level
    fix landed** (§4a): `draft`/`hidden` are on every `Route` and in
    `route_schema()`; the sitemap filter excludes them; re-probed leak-free at
    573. Profiles (the proper fix) still ride with phase 3, but the leak is no
    longer waiting on them.
20. ~~Themes are Rust (§5d)~~ — **settled, dissolved by §5e** (steps 3–4,
    built): a theme is a directory of hole-bearing fragments + SCSS, presence
    conditionals died to empty-slot-collapses, and a fragmentless theme IS
    the null theme. A third theme is `mkdir`, no recompile. The residual
    Rust in presentation is the computed head facts (`render.rs`) and the
    `light` shell — engine surface, not theme surface, by design.
21. **Tighten `diff`'s liquid skip (§8c).** 97 of 327 posts are excluded, many
    falsely (`{{ github.event.issue.number }}` in code samples is GitHub
    Actions, not Liquid). 30% of the corpus is unmeasured and the 90% is over
    an unrepresentative 230.
22. **`_site-prod` can no longer be regenerated (§5c, §8c).** `{% view %}` is
    not Liquid, so Jekyll fails the whole build; refreshing needs
    `git stash push index.html` first. Given §8c, losing the ability to refresh
    the reference is exactly the capability that caught the 17-point lie. Script
    it, or move the reference build behind a flag that stashes automatically.
23. **The `hero` part (§5e archetype test).** Card/gallery layouts need an
    image per summary; the part map has none. Front-matter `image:` with
    first-image-block fallback, thumbnailed via §6b? Recommend: yes, both, in
    that precedence — it is the same "explicit beats derived" rule as
    everywhere else. The mindstorms audit (§5) adds a third source for
    *group* heroes: a designated cover file (`cover.*`, matching the existing
    `alpharex_1.jpg` covers) beats first-item — explicit beats derived again,
    expressed positionally. ✅ **Built via the book club (2026-07)**:
    `hero` is a `Map("figure")` on `document`, sourced from the image-typed
    schema field named `cover` (beats `image`; §5b), thumbnailed with
    dimension facts. The card kinds consume the same source. **Still
    open**: the first-image-block fallback, and the mindstorms group-hero
    (`cover.*` file) — both arrive with their consumers.
36. ~~Unify `summary` and `card` into one preview kind (Matt, 2026-07)~~ —
    **settled, built (2026-07)**: a card is a view's *projection* of a
    row, and so is a summary — they differ by what the row *has* (posts:
    date/tags/blocks; books: cover/note), not by what they are: §5a's
    document argument one level down. `summary` is the one preview kind
    (its schema gained `src`/`width`/`height`/`note`, presence-driven);
    `card`/`card_list` are deleted; `card_list` folded into `listing` as
    a `featured` slot any listing may fill. The predicted main-site
    chrome cost turned out to be **zero** — the surviving kind kept the
    name `summary`, the new parts are absent on posts, and the byte
    oracle stayed clean; renaming `summary` → something projection-y is
    bundled into the parked micro-kind naming question. The unification
    forced q24 (variants) with it, exactly as its gate predicted.
    Documented as the rule, not fixed: the canonical fallback is
    all-or-nothing per subtree (canonical rendering never consults child
    fragments) — a theme opts into per-kind fragments from the parent
    down.
37. **The `board` kind: composition of views as content *(specced,
    deliberately pending)*.** The example's homepage reproduces the shape
    §5c diagnosed on the old `/` — a hand-written wrapper
    (`.home-grid`, was `.blocks-50`), authored `<h2>` labels, three
    `{% view %}` embeds — and adds a cost the example made visible: the
    arrangement is **theme-coupled** (render `/` under the recipes theme
    and `.home-grid` styles nothing, while `.slots/` identity survives
    every theme). The dissolution is the §5c move applied to page
    structure: a board is a *query over queries* —
    `[views.home] layout = "board"` declaring ordered members, each
    contributing `{label, content}`; the theme places one slot, CSS does
    the columns, labels become config the way `title`/`crumb` did. It
    would retire the last hand-written arrangement on either site's
    homepage. **Pending on purpose** (build at the second board, or the
    publish cutover, whichever first): (a) member declaration — a list of
    view names vs inline view definitions; (b) labels — declared per
    member on the board vs inherited from each member view's `title`;
    (c) whether a board can materialize a route or is embed-only;
    (d) boards-in-boards (leaning no — the §5d tripwire); (e) whether a
    board's items ride the q36 preview kind or stay opaque fragments.
38. **The link graph (§7b)** — *backlinks half built (2026-07)*: every
    rendered body (posts AND pages — a page-bodies prepass now feeds
    both the tree pass and the scan) is scanned for internal hrefs,
    inverted, and each document carries a `linked-from` relations group
    of dateless neighbors, sorted by title. The scan reads the same
    bytes that ship, so link and index cannot desync. The payoff
    predicted by the axes design landed exactly: the main theme's
    `relation.html` rendered the new axis with ZERO fragment changes
    (46 main-site pages gained it — the bluetooth series backlinks
    itself via its `post_url` cross-references; the example's carbonara
    is "Linked from: Recipes · The Typed Kitchen"). **Transclusion**
    (render row X inline by reference) remains open, waiting on a real
    consumer, with §5d's no-control-flow rule watching it.
39. **Set-scoped computed fields (§7b).** §5f fields derive from ONE
    row; the survey wants aggregates over a view's members —
    `count()`, `sum(minutes)`, date spans — for meal-plan rollups,
    subtree stats, calendar counts, term indexes. A natural §5f
    extension (functions whose source type is a member set), but it
    changes the field-inheritance story; spec alongside q31's build.
40. **Structured record fields (§7b).** `.schema.toml` wants a
    list-of-records type (`ingredients = {type="records", fields={qty=
    "string", name="string"}}`-ish) for ingredient lists, podcast
    chapters, cast lists — plus a schema.org/JSON-LD emission deriver
    fed by it. Extends §5b without changing its shape.
41. ~~i18n (§7b)~~ — **first slice built (2026-07), promoted to §6f**,
    which carries the design. Everything this entry predicted it would
    need turned out to already exist in the model: pairing is one more
    index (`by_logical`), locale-prefixed routes are the default route
    prefixed, and the `translations` axis is the §6b design absorbing
    one more member. Tag records (`[tags.id]` slug + per-locale names,
    Matt's enum-style-records direction) rode along. Still open in §6f:
    localized trails/chrome, locale-parallel listings, localized group
    keys, the prefix selector's first real corpus.
42. **Client-side faceted filtering (§7b).** Combinable facets (diet ×
    cuisine × season) can't be enumerated as static views. The
    search.bin architecture generalizes: ship a typed facet index, run
    the intersection in the client — a *client-side view*, declared in
    config like any other, materializing an index instead of routes.
43. **Media beyond image (§7b).** Audio/video schema field types (with
    duration/player facts, as image carries dimensions), podcast RSS
    enclosures as a feed variant, multi-format/srcset renditions from
    the §6b contest, and externally-hosted originals (a URL-valued
    image source that skips thumbnailing but can still carry declared
    dimensions).
24. ~~Per-view fragment variants (§5e)~~ — **settled, built (2026-07),
    forced by q36 exactly as predicted** ("needed the day one view wants
    cards while another wants summaries" — that day was the unification).
    A fragment file's stem is its *name*; the stem before `--` is its
    *kind* (`summary--card.html` binds `summary`). A view declares
    `variant = "cards"`; rendering tries `{kind}--{variant}`, falls back
    to the base fragment, then canonical — partial themes throughout. In
    fragments, `data-fragment` selects a variant for stream/map children,
    and being explicit it must resolve at load, to the right kind.
25. **Per-block facts (§5e).** A block-level directive (kramdown IAL
    `{:.full-bleed}` or similar) surviving as a `data-` attribute on the
    block, so a theme can span it. Needs a decided authoring syntax — IALs
    are kramdown, not CommonMark, so this interacts with the §8 dialect gap.
26. **Dimension facts on images (§5e).** Emit `width`/`height`/`aspect-ratio`
    on every `<img>` — grackle already knows them at thumbnail time (§6b).
    No real question except sequencing; it is a pure win and should ride
    with the thumbnail work in phase 3.
27. **Index-less directories (§5 audit).** 23 directories under `code/` and
    `writing/` have no `index.*` (`code/graphics/`, `writing/school/`, every
    `screenshots/`/`download/` dir). Two undefined behaviors: what
    `ancestors(page)` emits when the trail crosses a row-less directory
    (skip it? unlinked label from the dirname? — linking would 404), and
    whether `/code/graphics/` should exist at all. Needs a decided semantic
    plus a load-time warning. The model also offers the nice answer nearly
    free: an auto-index **view** — a `listing` of children materialized for
    each index-less directory — turning the hole into a page with zero
    authored content. Recommend: unlinked label now, auto-index view as the
    upgrade. **Forcing point arrived and the first half shipped (§6e)**:
    section trees render index-less directories as unlinked labels
    (`a:not([href])`, live on the example's `manual/advanced/`). The
    auto-index view remains the upgrade, sharing the `outline_entry`
    fragment — and q45 subsumes it exactly: an index-less directory
    is a landing view with no intro row.
28. **Mindstorms restructure vs URL parity (§5 audit).** The gallery
    restructure retires `/demos/mindstorms/alpharex_1.html` and its 16
    siblings — which are in the sitemap and carry **no `noindex`** (only the
    index page front-matters it; the step pages are front-matter-less
    passthrough, indexable today by accident). Needs `.htaccess` redirects
    or an explicit parity exemption like §11.12's — and either way, the
    accidental indexability is worth fixing before the restructure, not
    with it.
29. ~~Custom block widgets `{% callout %}` (§5d)~~ — **settled, built**
    (§5d "Custom widgets"): `[widgets]` registry in config, paired-tag
    expansion in `tags.rs` (body expanded recursively; missing end tag
    errors naming the file; unregistered names verbatim), both 2026 posts
    rewritten — all three raw shapes collapsed to `{% callout %}`, the
    `markdown="1"` kramdown idiom gone from the source, the fixture retired.
    The no-control-flow tripwire stands: no arguments, no conditionals.
30. **Pagination × subdivision (§5c).** A grouped view can be subdivided; a
    paginated one deliberately cannot *yet*. A year archive could plausibly
    paginate (`/blog/2022/page/2/`) while months subdivide off the same root
    (`/blog/2022/12/`) — the row-set semantics are coherent (subdivision
    partitions the *whole* group, not one page of it), but the parent's
    pagination URLs and its children's routes then share the year root's
    namespace. **Subdivided collections may overlap if we aren't careful**,
    and the overlap has two grades worth distinguishing:

    - **Actual collision** — two routes materialize the same URL. The whole
      route set exists at load, so this is checkable as a **hard error**
      today (the database advantage: collisions are a query, not a 404
      discovered in production).
    - **Pattern-space overlap** — the *shapes* intersect but today's keys
      don't: `/blog/2022/{page/N}` vs `/blog/2022/{month:02}/` can never
      actually collide (`page` ≠ a two-digit month), and a paged year is
      unlikely to run into a two-digit date on a low-traffic blog. This
      grade should **warn**, with an explicit config acknowledgment
      (something like `allow_overlap = true` on the child view) to silence
      it where the author has judged the risk — warn-or-declare, the same
      posture as the §4a leak checks.

    Also parked here: crumb templates for paginated views (the `Page N`
    trail entry is an engine rule for now). Punted until wanted; the config
    check errors with a pointer to this question.
31. **Computed fields want functions (§6d).** The current shape — a field
    is a table whose single deriver key names the computation
    (`[views.x.fields.summary] truncate = { max_blocks = 4 }`) — is **not
    quite right**, and known to be. It hardcodes one implicit source (the
    row's content blocks), can't compose (truncate of a rewrite? lede of a
    section?), and each new deriver grows a struct rather than a
    vocabulary. If the config grows a function/expression form, a field
    becomes `summary = truncate(content, max_blocks = 4, max_chars = 700)`
    — explicit source, named args, composable, and validated the way the
    filter language already is. Revisit rather than extend: adding a second
    deriver to the struct shape deepens the wrong groove. Inheritance
    semantics (fields flow with rows along `over`, nearest wins) are right
    and survive the change.

    **Direction settled (2026-07): extend the filter language, borrow no
    engine.** Surveyed: `liquid`/`liquid-core` rejected a third time (the
    grammar is the cheap 20%; typed load-time validation against our schema
    is the value, and liquid doesn't bring it); `minijinja` expressions are
    credible (real kwargs, zero deps) but dynamically typed — schema
    checking needs its unstable `machinery` AST — and would split the
    config into two expression languages; CEL fits conceptually but is
    dependency weight with a type system to bridge. So: add call syntax
    with named arguments to `filter.rs`, functions in a typed registry
    (name → source type, params, return type), same load-time errors naming
    the knowns; filters become expressions of type bool, fields expressions
    producing content. Constraint worth keeping: the grammar stays
    **CEL-compatible** (our filter syntax already is, accidentally), so the
    hand-rolled parser can be swapped for `cel` later without breaking a
    config file. Build at the q23 `hero` forcing point. **Spec: §5f** —
    the CEL contract, surfaces, environment typing, options-as-map-literals,
    and the divergence ledger.
32. ~~Producers hardcode routes that config owns (§9b)~~ — **settled,
    built (2026-07)**, in exactly the predicted shape and with the
    predicted small answer. Pagination: the producer takes URLs
    (`pagination(current, urls)` no longer knows what a blog is); build
    renders them from the owning view's own `routes` templates,
    locale-prefixed like the routes themselves. Tag pills: the posts
    collection may declare which view owns tag routes
    (`tags = "tag_index"`, next to `crumb`/`index`/`trail`), falling
    back to the unique tags-grouped view — ambiguity without a
    declaration is a load error, the declared view must be tags-grouped
    and routed, and its template must render from `{key}` alone
    (probed at load). `Config::tag_url(id, locale)` renders the
    template with the record's slug and the view's locale
    materialization; **no tags view = unlinked pills**, presence-driven
    like everything else. i18n forced this settlement: the hardcodes
    had already grown locale prefixes in two places. One deliberate
    visible change, accounted mechanically: pagination links now carry
    the route template's trailing slash (`/blog/page/2/`) — 66 files
    changed, every byte explained by that one substitution; the old
    slashless form was jekyll-paginate parity that made every click
    bounce through a server redirect.
33. **View-name policy in `build.rs` (§9b)** — *the serialization half is
    settled (2026-07), with better vocabulary than this entry proposed*:
    Matt's framing is that the feed is not a special pass but a
    different **shell** — the outermost serialization of the same rows.
    Views now declare `shell = "atom"` / `"sitemap"` (validated at
    load), and the template-filename match is retired; q44 carries the
    full generalization. Still open here: listing `noindex` decided by
    `view != "blog_index"`, the `blog_index` fallback layout match, and
    the sitemap filter's second evaluation (star routes carry no
    members).
44. **Built-in shell types (Matt, 2026-07)** — promoted to **§5g**, which
    carries the design: the root HTML shell all themes inherit is BUILT
    (null theme yields valid documents, `light_shell` dissolved,
    `subtheme` engine-placed), atom/sitemap view shells are built, the
    md shell is specced with `/llms.txt` as its forcing consumer.
    Remaining pending, per §5g: row-level `shell:` (splitting `layout:
    light`'s conflation), theme `head.html` extras, serializers as
    part-map consumers, a `json` shell.
34. **Three "not content" lists (§9b).** §4c's three layers govern the tree
    walk only; `slots.rs` (`SKIP`) and `serve.rs` (`is_content`) carry
    private skip lists that duplicate — and can silently drift from —
    `grackle.toml`'s `exclude`. Both walks should derive from the §4c
    layers (`store::walker` + config). The serve list has one extra
    legitimate member the others lack: `_cache/`, which a rebuild *writes*
    (watching it would loop) — that one is serve's own, and stays.
35. ~~Scope markers (§6e)~~ — **settled by building** (2026-07): (a) a bare
    file, engine vocabulary like `.slots/` — no config entry, no payload
    (options can arrive later as file contents without breaking empty
    files); (b) yes — `order:` is a page schema field, sorted
    (order, label), unset last; (c) nested `.section`s nest, nearest wins,
    pinned by test. A marker with a payload remains available the day
    depth/ordering options are wanted.
45. **Index pages vs view roots (Matt, 2026-07): the landing is one
    concept with four implementations, and none owns it.** A deep
    review, prompted by "they really seem to be fuzzy." The page that
    *stands for a set* — a section landing — exists four ways: a
    **view root** (`/blog/`, `/photos/`, `/books/` — a materialized
    route, not a row); an **index page** (`recipes/index.md`,
    `manual/index.md` — a *row*, routed by `**/index.{html,md}`); the
    **home page** (`index.html` at `/` — a row plus special cases:
    `home_url`, the engine `home` string); and **`collection.index`**
    (a raw URL in config standing for a landing in trails). Four
    mechanisms then answer "what landing is above this thing," each
    differently: posts via `trail_root` → `collection.index`; listings
    via the grouped chain's route templates; tree pages via
    `ancestors()` — a URL-walk that sees only page rows; nav by making
    the author pick a dialect per section (`view:blog_index` vs
    `/recipes/index.md`).

    **The symptoms are live, not hypothetical** (all observed on the
    example): `/books/the-typed-kitchen/` has no Books crumb —
    `ancestors()` cannot see a view root, so the club is unreachable
    upward from its own members. `/fr/recipes/red-lentil-dal/` shows
    `Accueil › Carnet de terrain › …` — home *duplicated* (the `/fr/`
    prefix makes the homepage row look like a directory ancestor) and
    Recipes *missing* (no `index.fr.md`). `/fr/recipes/` is a 404
    while `/fr/blog/` exists — locale-parallel is default-on for views
    but impossible for index pages, and `index.fr.html` visibly routes
    around it by linking the lone French recipe directly. `stem !=
    "index"` appears in three view filters — the landing is a row, so
    every query over its table must remember to exclude it or the
    listing lists itself, while view listings are excluded
    structurally by route kind: one distinction, two mechanisms, one a
    naming convention. `recipes/index.md` hand-states "serves 2, 25
    min" beside a schema that owns `servings`/`minutes`, and
    `manual/index.md` hand-lists the tree §6e derives — hand-maintained
    materialized views, the classic denormalization drift. And
    `collection.index = "/blog/"` speaks the raw-URL dialect §6a made
    a *load error* in content. The one honest seam: route collisions
    are checked cross-table, so a section at least cannot silently be
    both (`books/index.md` would error against `views.books`).

    **Diagnosis**: a landing is a **projection** (derived listing)
    plus, sometimes, **prose** (an owned row). An index page is a row
    pretending to be a view; a view root is a view missing its row's
    prose. Every symptom is one of those halves missing or
    hand-forged. The homepage already demonstrates the working shape —
    prose that *embeds* views: the row owns the words, the query owns
    the list.

    **Proposed unification: a landing URL is always owned by a view,
    and a view may take its prose from a row.** `/recipes/` becomes a
    view over `pages` like `/books/` already is. A view can declare —
    or discover by convention — an **intro row**: `index.md` in the
    matched directory supplies front matter and prose rendered above
    the derived listing (the homepage hybrid made first-class). The
    intro row stops being routed standalone and stops being listable,
    so every `stem != "index"` filter dies structurally. Then the rest
    falls out: locale-parallel applies uniformly (`/fr/recipes/`
    materializes with French rows, intro from `index.fr.md` else the
    default intro — the same suffix fallback slots use); trails unify
    (`ancestors()` is replaced by a landing chain that view routes
    participate in); `collection.index` names a **view** (closing the
    q32-adjacent pending §6f records); nav speaks one dialect —
    `view:` for every section. Home is the root landing: a view whose
    intro is `index.html` and whose listing may be empty —
    prose-only landings are the degenerate case, which also covers
    `manual/` (its "listing" is §6e's section tree, a different
    layout), and `home_url` falls out. q27's auto-index upgrade is
    subsumed exactly: an index-less directory is a landing view with
    no intro row. q37's board is the sibling question (what home's
    *listing* is, when it is several views).

    **Open before building**: (a) how a landing derives its *parent*
    for the chain — URL nesting of landing roots vs an explicit
    `parent =`; (b) front-matter authority — does the intro row's
    `title:` beat the view's `title`/`crumb` (leaning: yes, explicit
    beats derived, per row); (c) discovery — `index.md` in the
    matched dir by convention vs `intro = "recipes/index.md"`
    declared (leaning: convention, declared override); (d) what
    happens to the `**/index.{html,md}` route rule and URL parity on
    the main site, where index pages are today ordinary rows; (e) the
    interim for `manual/index.md`'s hand-list before the section-tree
    landing exists.

    **Direction settled (2026-07, Matt's shape): the engine never
    guesses the arrangement** — either the theme owns it (a slot) or
    the author does (the row places the embed); the "prose above
    listing" concatenation is dead. Three tiers. **Bare**: query +
    route + `listing`, what `/books/` is today. **Declared text**:
    the view declares `intro` (the chosen word — over preamble, lede
    taken by q31, standfirst too clever), a `LocalizedStr` rendered
    as *markdown through the view's locale-aware link resolver* (an
    intro can say `view:…` and gets strict validation), filling an
    `intro` slot on the listing layout — empty collapses (binder
    rule 2), so every existing listing is byte-identical and the
    magical view just works. **Referenced content**: the view
    declares `content = "recipes/index.md"` (mutually exclusive with
    `intro`; both = load error) — the row becomes the whole body and
    is *responsible for placing* `{% view <owner> %}`, a load error
    if it doesn't (the rows would be unreachable), scoped to views
    that have a query. The row keeps everything rows have: front
    matter, its rule-derived theme (the landing wears its section's
    clothes), suffix localization with default-locale fallback.

    **Claiming**: a referenced content row is *claimed* — no
    standalone route, and excluded from every query **structurally**,
    by ownership, killing the `stem != "index"` naming convention
    outright (forgetting the filter becomes impossible rather than a
    self-listing bug). Claiming is declared, never discovered — a
    convention would claim rows silently; declared is q5's answer
    again (schema declared, not inferred). Which makes migration
    incremental: `**/index.{html,md}` survives for unclaimed rows,
    the main site is untouched (oracle-safe), and sections lift one
    `content =` declaration at a time — the q9 page-bundle posture.
    **Home stops being special**: a *queryless landing*
    (`route = "/"`, `content = "index.html"`) whose content embeds
    other views by name — the must-place check has no rows to
    strand; q37's board hangs in this frame later, and `home_url`
    becomes "the root landing's route, per locale."

    The subtle semantic: the owning view's self-embed is
    **route-aware** — it renders *this route's slice* (page 2 of
    `/blog/` renders page 2's rows; `/fr/recipes/` the French
    partition), unlike embeds of *other* views, which keep today's
    whole-view semantics. Intro and content render on every page of
    a paginated landing: the prose is the landing's face; the slice
    is what changes.

    ✅ **Built and tasted (2026-07)**, in the settled shape, engine
    oracle-clean (the unfilled `intro` slot collapses; the fragment
    glues the intro div to the items line so the collapse leaves no
    stray whitespace — the binder deletes elements, not their
    lines). The example's `/recipes/` is a mode-B landing
    (`content = "recipes/index.md"`, the hand-list replaced by
    `{% view recipes_index %}` rendered through the RECIPES theme's
    own new listing/summary fragments — the landing wears its
    section's clothes, styled as kitchen tabs); `/books/` tastes
    mode A (`intro` with a resolved `view:blog_index` link).
    Cascade observed, all predicted: `/fr/recipes/` materializes
    (index.fr.md prose, the one French row, French chrome
    throughout); the dal trail reads `Accueil › Recettes › Dal…`
    (the duplicated home crumb died — `ancestors()` now skips
    locale-prefix homes, Home is the trail root's job — and the
    Recipes crumb appeared, because the claimed row keeps title
    and landing URL, so the ancestors walk still sees it); both
    `stem != "index"` recipe filters deleted; nav's one
    `/recipes/index.md` source link resolves per locale (source
    links gained the view-link invariant: prefix-and-check,
    fall back); sitemap gains `/fr/recipes/` and loses nothing.
    Settled by building: row front matter beats view title;
    claiming is declared-only; migration is incremental (unclaimed
    index pages behave exactly as before — main site untouched).
    One deliberate semantic change: claimed rows leave the
    backlink scan, so listed rows no longer show "Linked from:
    Recipes" — *membership is not citation*; an authored citation
    in the prose would still count the day the landing bodies join
    the scan.

    **The chain's first slice followed (Matt: "since there's a
    default page listing those books, shouldn't breadcrumbs pick it
    up?")**: `ancestors()` falls back to a materialized landing
    route when no page row owns the parent URL — the view's crumb
    (else title) at the route's locale. A book's trail now climbs
    `Home › Book of the Month › The Typed Kitchen`; mode-B landings
    never reach the fallback (the claimed row matches first, row
    title winning as everywhere). **Listing trails climb the same
    chain** (deduped by URL against the collection crumb, which
    already roots `/blog/`-style listings): when the course archives
    moved under the landing (`/recipes/courses/{key}/`, Matt's ask),
    `Home › Recipes › Dinner` fell out of the nesting with no
    declaration — and the move itself cost zero source edits, every
    `view:` link re-deriving. URL nesting IS the parent derivation
    for tree rows; what remains of (a) is only the non-nested half:
    `collection.index` naming a view, and whether a landing may
    declare `parent =` when nesting lies.

    Narrowed but open: (a)'s non-nested half (above); theme
    provenance for bare/intro landings (content rows bring their
    rule-derived theme; should a view declare one?); orphaned
    translations (`index.fr.md` with no French rows has nowhere to
    render → warn); whether mode-B prose is searchable (landing
    routes are structurally excluded — leaning keep them out until
    someone misses it);
    and, noticed while tasting: a variant fragment lacking a hole
    drops that part silently (`listing--cards.html` swallowed the
    books intro until it grew the slot) — "never ship what a theme
    hides" may want a load-time warning for schema parts no
    fragment of a theme places.
