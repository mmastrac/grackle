# grackle — a virtual database over the site, with a renderer attached

Rust replacement for the Jekyll build of grack.com. Goal: **equivalent rendered
HTML for most of the blog**, byte-identical URL set, then iterate toward
exactness using the existing Jekyll output as a golden reference.

## 0. The tour: one post, end to end

The one-sentence model: **the site is a database that happens to live in git,
and a theme is a stylesheet with opinions about where things go.** Everything
between is a pipeline of typed, checkable steps:

```
file → row → query → doc model → part map → slots → CSS → URL
```

Every step below is built and measured, with three deliberate gaps: the doc
model's `notes` stream (§6d stage B), per-theme head-fact selection (the engine
renders all head facts today), and serve's incremental invalidation (v1
rebuilds the world in ~0.4s).

**1. You write a file** — `_posts/2026/2026-07-17-espresso-grinder.md`, with
`title` and `tags` in the front matter, a footnote, and `![](burrs.jpg)`. That
is the whole authoring interface: markdown, in git, minimum front matter. No
layout declared, no URL, no path to the image.

**2. The database claims it (§1–§3).** Directories are tables, files are rows,
and every file belongs to **exactly one** table by precedence — posts, then
objects (by extension), then tree. The post lands in `blog`, `burrs.jpg` in
`objects`, everything else is `tree`. Columns fill by **one precedence law
used everywhere: nearest wins, first writer per key** — front matter beats
markers (§4b: `_posts/drafts/.draft`) beats rules (§4: a `**` catch-all). The
filename yields `(date, slug)`, the rule yields the route, and the row is
addressable at `/blog/2026/07/17/espresso-grinder/`. Everything is **checked at
load time, not discovered as a 404**: two rows on one URL, a dated route on an
undated row, a rule matching nothing — all errors naming the file and the rule.

**3. Views query it (§5, §5c).** Nobody writes
`{% for post in site.posts %}{% unless post.draft %}`. Queries are declared
once:

```toml
[sets.published]                    # a query that never lands
from  = "posts"
where = "!draft && !hidden"

[routes.blog_index]                 # a query that lands, on two paths
from     = "published"
paginate = 5
paths    = ["/blog/", "/blog/page/{n}/"]
```

**A view is a query; a route is just where it lands.** The new post enters
`published` and therefore appears in `/blog/`, the feed, the `hardware` tag
page, the July archive and the home page — the *same query*, composed, defined
in one place. Filters are parsed and type-checked at load, so `!drafts` is an
error naming the known fields, not a filter that silently matches everything.

**4. Rendering produces structure, not a string (§6d).** The body becomes a
**doc model**, not one HTML blob:

- **blocks** — the top-level sequence, addressable by *position*. A summary is
  literally `blocks[..cut]`, so listings ship 2 paragraphs rather than full
  bodies hidden by CSS (~93% of `/blog/`'s weight deleted).
- **notes** — the footnote, a second stream associated with its block by
  *identity*. Where it renders is deliberately not decided yet.
- **rewrites** — rules addressing rendered HTML by *CSS selector*.
- **facts** — typed truths: the row has a date → `og:type=article`; the summary
  was cut → `data-truncated`; `burrs.jpg` resolved (bare name → nearest sibling
  or bucket, §6a) with known dimensions → `width`/`height` on the `<img>`.

Position, selector, identity: three addressing modes, and that trio is what
"reach into the markdown" means here.

**5. A layout kind fills named parts (§5a, §5e).** The kinds are `document`,
`listing`, `feed`, `raw`. A `document` emits a **part map, not a page** —
`title`, `crumbs`, `tags`, `content`, `notes`, `neighbors` — each flat,
semantic HTML. No wrapper divs, no arrangement: the layout kind genuinely does
not know whether footnotes will become a sidebar.

**6. The theme places parts in slots (§5e).** A theme is a **directory of
data** — `theme.toml`, `shell.html`, per-kind fragments, `theme.css`. No code,
no recompile. A fragment is straight-line HTML with holes, and the hole algebra
is four rules: a hole is `data-slot`; **an empty part deletes its element**
(every `{% if %}` you'll never write); **a stream maps a fragment over its
items** (every `{% for %}`); an attribute hole is `data-slot-attr`.

```html
<article data-kind="document">
  <nav data-slot="crumbs"></nav>
  <h1 data-slot="title"></h1>
  <div data-slot="content"></div>
  <aside data-slot="notes"></aside>       <!-- absent notes ⇒ no <aside> -->
</article>
```

Unknown slot names are load-time errors, exactly like filter typos.

**7. CSS does the geometry (§5e).** Modern CSS is the declared baseline —
nesting, `:has()`, container queries, `@layer`, `aspect-ratio` — and *all*
arrangement lives there:

```css
[data-kind="document"] { grid-template-areas: "crumbs content" "tags content"; }
/* this theme wants Tufte sidenotes: claim the stream, add a column */
article:has(> [data-slot="notes"]) { grid-template-areas: "crumbs content notes"; }
```

The footnote just became a sidenote, and no layer above CSS was consulted.

**8. Build, serve, query — clients of one database (§7).**

```
$ grackle build     # materialize every route   (~0.4s; Jekyll ~38s)
$ grackle serve     # resident db: save → invalidate → browser reload
$ grackle query 'posts where "rust" in tags limit 5'
$ grackle explain /blog/2026/07/17/espresso-grinder/
```

### Day two: every change has exactly one home

| you want | you touch |
|---|---|
| a new post | one markdown file |
| hide a subtree from search | `touch code/legacy/.noindex` |
| a "recent Rust posts" box | a `[sets]` entry: `where = '"rust" in tags'` |
| a photo-gallery page | a view `variant` + a `card` fragment + grid CSS |
| a new look, dark mode included | copy a theme directory, edit HTML + CSS |
| one weird table in one post | a `<style>` block there — scoped, compiled (§6c) |
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

| Origin | Identity | Primary index | Source |
|---|---|---|---|
| `posts` | source path | `(date, slug)` unique | `_posts/**` |
| `tree` | source path | path hierarchy | site root |
| `objects` | source path | `by_name` (non-unique) | by extension |

**One store, three origins** *(2026-07-21)*. These were three tables; they are
one `SiteDb.rows` and three lists of keys (`post_ix`, `page_ix`, `object_ix`).
Objects went last and cost nothing, because q51 had already written every
index to gate on a row's PROPERTIES rather than on which vector it arrived in:
`by_key`/`by_slug`/`by_tag` ask `post_ix` membership, and `by_logical` asks
`rendered` — whose comment already read *"a static file has no logical
identity to pair a translation on"*. Not one index changed.

What the objects table had been doing, and what took each over: **routing** →
the same property-driven `RouteKind` arm every other row uses; **`by_name`** →
an index on `SiteDb`; **the narrower vocabulary** → the collection's, not the
table's; **a separate view flow** → `build_row_view`'s, once membership became
a filter.

**Membership is a filter now**, which is q51's move applied one table further:
an object view's base is `collection == "<the objects collection>"`, ANDed onto
the view's own predicate. The two halves are parsed against *different*
schemas on purpose — the author's filter still type-checks against
`object_schema`, so `where = "draft"` on a gallery stays the load error §5b
wants, while the membership clause names a column only the full row schema
has. The hazard the merge introduces is the mirror image: with one store, a
content row could leak into a gallery. Pinned by a test that was verified to
fail when the filter is removed.

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

[[collections]]
kind   = "posts"
source = "_posts"
# Filename → (date, slug) extraction, tried in order.
# Second form covers the legacy MM-DD-YYYY posts; no match ⇒ undated row.
filename_formats = ["{year}-{month}-{day}-{slug}", "{month}-{day}-{year}-{slug}"]

  [[collections.rules]]
  match    = "drafts/**"
  defaults = { draft = true }
  route    = "/drafts/{slug}/"          # undated: route must not use {year}

  [[collections.rules]]
  match    = "hidden/**"
  defaults = { hidden = true }          # routed normally; excluded from lists

  [[collections.rules]]
  match    = "**"                       # everything else, default flags
  defaults = { layout = "post" }
  route    = "/blog/{year}/{month:02}/{day:02}/{slug}/"

[[collections]]
name   = "pages"
kind   = "tree"
source = "."

  [[collections.rules]]
  match = "**/index.{html,md}"
  route = "/{dir}/"

  [[collections.rules]]
  match = "**/*.{html,md}"
  route = "/{dir}/{stem}/"

  [[collections.rules]]
  match = "**/*"
  route = "/{path}"                     # static passthrough

[[collections]]
name       = "objects"
kind       = "objects"
extensions = ["png", "jpg", "jpeg", "gif", "webp", "svg"]
bucket     = "assets"                   # §6a: bucket dir NAME, not a path

  # Named routes: pin an object to a stable URL regardless of where it lives
  [[collections.rules]]
  match = "assets/branding/logo-v3-final.png"
  route = "/logo.png"

  [[collections.rules]]
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
- **URL-set parity** with the reference build is a hard requirement
  (`grackle urls`, which is the instrument this line claimed `grackle diff`
  was for years — diff compares post *bodies* and never looked at the URL set
  at all) —  set not shifting. See §4a for the one intentional exception.

The route map doubles as the **reverse index for serving**: URL → row is a
lookup in the same structure, so `serve` needs no output directory at all.

### Several collections, one table *(built 2026-07-19)*

A collection is a *source*, not a table. `_posts` and `_drafts` are two
sources of one corpus — same row shape, same schema, same views — so
every `kind = "posts"` collection contributes rows and the posts table is
indexed **once, over all of them**. Indexing per collection would have
built each index over a fragment: `by_url` could not have seen a
collision between two sources, and `order` would have restarted at every
one.

Before this, a second posts collection silently did `db.posts = table`
and the last one won. Nothing warned; the first collection's rows simply
vanished.

Drafts ride this: `_drafts` is a source whose rule sets `draft = true`,
and the `!draft` filters the views already carry keep them out of the
feed, the listings and the search index. They are ordinary rows in every
other respect — they materialize `/drafts/{slug}/` routes and take part
in the link graph — because **nothing publishes from grackle yet**, and
inventing draft-specific suppression now would be guessing at what §4a's
profiles (q6, q10) should decide later. The gate is the cutover, not the
loader.

## 4a. Profiles: a projection, not a different database *(v1 built 2026-07-19)*

A profile changes **three things and no others**: which rows the views
admit, the absolute URL the output is addressed under, and a marker
themes may style on. It never changes what *loads* — the database is
identical under every profile, which is what makes two projections
comparable, lets one resident db answer for several, and keeps the
inspector able to say "included in drafts, excluded from default".

```toml
[profiles.drafts]
noindex = true

  [profiles.drafts.views.published]
  where = "!hidden"          # relax the one filter that hides drafts
```

`build` uses the default projection — the config exactly as written —
because that is what publishes. `serve` defaults to `dev`, which needs
no declaration: undeclared, it changes nothing. Any other name must be
declared, so a typo is a load error naming what exists
(`unknown profile "drafst" — declared: dev, drafts`), and the key set is
closed (`unknown field 'baseurl', expected one of 'url', 'noindex',
'views'`) because a profile that can override anything is a config merge,
and config merges drift.

**Selection is the view's job.** Relaxing `published`'s filter carries
every listing, archive and feed with it, because they all read
`published`. Measured: the default projection has no draft in its feed,
blog index or search index; the drafts projection has all four.

**Presentation costs no engine code.** The root shell stamps
`data-profile` beside `data-subtheme`, so a dev banner is a theme CSS
rule on `[data-profile="dev"]`. Themes opt in; a theme that ignores it is
unaffected.

**What this deleted.** Two hardcoded draft behaviours: `search_docs`
filtered `!draft && !hidden` in Rust while the search *view* declared the
same filter (q33(e)'s second evaluation), and `post_trail` gave a draft
the crumb `Drafts → /drafts` with the URL written literally in the trail
builder — a profile's address assumption living in engine code.

**Punted, deliberately.** `baseurl` is not a profile key: today it
prefixes assets but not routes, so setting it per profile would relocate
the stylesheet while leaving every canonical URL pointing at the real
site. Making it a true route prefix is the other half of the address
axis. Also not here, and not ported from Jekyll's five configs:
`exclude` (§4c owns what is content), per-profile `defaults` (collection
rules own those), analytics (a site fact conditioned on a profile is how
you reinvent the config merge), and the `_config-fast.yml` profile that
skipped `code/` and `writing/` — it existed because Jekyll took 38s, and
a 0.4s build deleted its reason to exist.

### What the corpus actually holds

- `_hidden/` holds **14 real dated posts** that **nothing has ever built** —
  not Jekyll (no config references it) and not grackle (it is not a
  collection source). They are tracked writing, not published content.
- `_drafts/` holds **4 undated drafts**, moved there from `_drafts_temp`
  on 2026-07-19 and loaded since (§4, several sources one table).
- **No post sets `hidden:`.** Page rows can and one does — the example's
  `demos/pane.html` — since pages gained the flag family on 2026-07-19.

### Flags reach the row, the route and the head

`draft` and `hidden` are carried onto every `Route` and exposed in
`route_schema()`, so a star view's filter can see them. `noindex` reaches
the head. Both families cascade from markers and rules exactly as any
other default does (§4b).

Pages carried **none** of this until 2026-07-19 — `Page` had no flag
fields at all, so a page's `hidden:` evaporated and its `noindex:` never
reached the head. `demos/mindstorms/index.html` had declared
`noindex: true` in its front matter for years and shipped without a
robots meta. The claim "only posts can be flagged", true when the route
flags were added, is no longer.

### The sitemap leak, and why route-level flags exist

Worth keeping because it is why the flags live on routes at all. Probed by
adding two posts dated newer than anything real — one draft, one hidden — the
flagged rows landed **in the sitemap** (573 → 575) even though `published`,
`latest` and `/blog/` correctly excluded them. A section titled "add no public
URLs" was emitting the most public URL there is.

This was **grackle's divergence, not Jekyll's**: `publish.sh` builds drafts as
a *separate site*, so Jekyll's main sitemap never saw them. Routing drafts into
the main build created the exposure. The fix was the route-level flag plus the
sitemap's own filter, and it re-probed clean at 573 with both probes present.
Given this project began with *"I'm having trouble with Google crawling this
site"*, it was precisely the wrong failure mode to ship.

Profiles are the general answer the probe pointed at, and what shipped is
narrower than the original sketch and better: a profile overrides an existing
query's `where`, so selection stays the view's job and a profile invents no
queries of its own.

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
[[collections.rules]]
match    = "hidden/**"
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

> **Vocabulary** *(2026-07-19)*. This document says **view** for the shared
> concept, as SQL does — a query, materialized or not. Config splits it:
> **`[sets]`** never lands, **`[routes]`** does, and `path` is what tells
> them apart (§5c has the key census). A collection rule keeps `route` — it
> makes one URL per row, not per query.

```toml
[routes.tag_index]
from     = "posts"
group_by = "tags"                       # one output row-group per tag value
path     = "/blog/tags/{key}/"
layout   = "tag_index"

[routes.yearly_archive]                  # new with grackle: /blog/2010/ was a
from     = "posts"                       # 404 between /blog/ and /blog/2010/01/
group_by = "date.year"
path     = "/blog/{year}/"
layout   = "yearly_archive"

[routes.monthly_archive]
from     = "yearly_archive"             # subdivision (§5c): GROUP BY year, month;
group_by = "date.month"                 # {year} comes from the parent's key
path     = "/blog/{year}/{month:02}/"
layout   = "monthly_archive"

[routes.blog_index]
from     = "posts"
where    = "!hidden && !draft"
paginate = 5
paths    = ["/blog/", "/blog/page/{n}/"]

[routes.feed]
path     = "/atom.xml"
from     = "posts"
where    = "!hidden"
limit    = 20
template = "atom.xml"                   # rendered as a bare liquid page

[routes.sitemap]
path  = "/sitemap.xml"
from  = "*"                          # all routable rows
where = 'dir || ext == "html" || ext == "pdf"'
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

`where`/`group_by`/`limit` are deliberately tiny — a predicate language over
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
where = '!draft && !hidden'
where = 'year >= 2020 && "rust" in tags'
where = '!(draft || hidden) && description'
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

Walking the two big tree sections against this model mostly confirms the shape:
`code/graphics/raytracer/` is already a page bundle (index.md + sibling
screenshots + a zip + a sub-page), exactly §6a's measured case, and the oddballs
(front-matter-less `README.md`s, 1996-era `enel555.html`, the extensionless
`nnet` binary, download tarballs) all land correctly under existing passthrough
rules. Two things are worth stating.

**Curated indexes are content, not views.** `code/index.md` is a hand-authored,
hand-ordered project list with foreign keys reaching across tables into posts.
It must *stay* authored — a content-first system keeps "a human chose this
list" distinct from "a query derived it", and the model already does: it is a
`document`.

**The gallery is a restructure the tree already knows how to express.** Today
mindstorms is 451 zero-padded JPGs flat in one directory, with 17 hand-written
HTML pages encoding disjoint ranges (`alpharex_2.html` owns 0074–0124) — the
grouping exists *only* in the HTML. Positionally restructured, the tree encodes
it (`demos/mindstorms/alpharex/part-2/…`) and the view is ordinary config:

```toml
[routes.mindstorms]
from     = "objects"
match    = "demos/mindstorms/**"
group_by = "dir"
variant  = "gallery"
path     = "/demos/mindstorms/{key}/"
```

The three gaps this audit found are **all built** (2026-07), forced by the
example site's gallery (§7a), which also killed the phase-1 gate — views now
dispatch on the base collection's *kind*, never its name:

1. **Objects have no schema** → `object_schema()`:
   `path`/`dir`/`name`/`stem`/`ext`/`url` + `size`, so `from = "objects"`
   filters type-check with the usual errors. Dimensions stayed out of the
   *filter* schema on purpose — they are render-time facts from the thumbnail
   pass, not load-time columns.
2. **View scoping needs `match`, not a bigger filter language** → a `match`
   glob on views, reusing rule globs; the filter language stays
   typed-fields-only.
3. **`order_by` does not exist** → built, then **half-retired 2026-07-21**: an
   object is a `Row` now, so it has a path; paths order, and that is a contract
   rather than the luck of the corpus's zero-padding. An object view takes the
   same ordering rule as every other view.

Still open for mindstorms specifically: `group_by` over object paths (one
gallery route per directory), the group `hero` (q23), and the URL-parity
question the restructure raises (q28) — the 17 range pages are in the sitemap
today, indexable by accident. Doing nothing also works: with no front matter
the current pages are pure passthrough, so the gallery is an opt-in
restructure, one robot at a time. The `page-break-inside: avoid` on every step
page (these are building instructions meant for printing) becomes a
`@media print` block in theme CSS, and the repeated inline `<style>` becomes
the first real second use case for §5b's `.style.scss` overlays.

## 5a. Presentation, from first principles

> Superseded in the build by §5e, which carries the model as shipped. What
> stays here is the layer cut §5e rests on, and the diagnosis that produced it:
> **six Jekyll layouts and six includes implemented about three concepts**, and
> the conflation was measurable — three listings with three hand-written
> queries whose filters *disagreed* (`monthly_archive` excluded hidden+draft,
> `tag_index` and `blog/index` only draft), two document layouts carrying two
> drifted breadcrumb implementations, and a shell branching on `multipost`,
> `hide_sidebar`, `paginator`, `page.date` and `noindex` because `{{ content }}`
> bubbled *upward* and forced the outermost template to know every inner case.

Four layers, each changing for its own reason and at its own rate:

| Layer | Owns | Changes when |
|---|---|---|
| **Schema** | what fields a row *has*, typed | the content model changes |
| **Rendering** | body markdown → semantic HTML fragment | an author writes |
| **Physical layout** | arranging rows + fields into `main` | the information architecture changes |
| **Visual theme** | the shell around `main` — chrome, `<head>`, CSS | the design changes |

Jekyll has no schema layer, and conflates the other three.

### Layout kinds: there are three

Not "what this site has" — what a site of this shape *needs*:

| Kind | Input | What it was in Jekyll |
|---|---|---|
| **document** | one row, full content + relations | `post.html`, `page.html` |
| **listing** | N rows, summarised | `tag_index`, `monthly_archive`, `blog/index` |
| **feed** | N rows, serialised | `atom.xml`, `sitemap.xml` |
| **raw** | one row, content *is* `main` | the 6 pages using `layout: default` |

`raw` is not a wart: `index.html` builds its own `<article>` with its own `<h1>`
and a grid. It wants the shell and nothing else. Naming it stops it from being
"the layout that means no layout".

A **view** (§5) supplies the query, the filter and the key; a layout kind
supplies the arrangement. `tag_index`, `monthly_archive` and `blog_index` are
then *the same layout* with different views — and their filter disagreement
cannot recur, because there is one filter, in one place, type-checked.

`document` unifies `post`/`page` because the difference is **schema-driven, not
layout-driven**: a row with a `date` has temporal neighbours; a row in a tree
has ancestors. The layout asks the schema what relations exist; it does not
branch on "am I a post".

### `<head>` is computed, then selected

The schema yields typed **head facts** — `title`, `description`, `canonical`,
`robots`, `og`, `jsonld` — and each tier renders the subset it wants. A row
with a `date` yields `og:type=article` + `BlogPosting`; one without yields
`website`. That's a *fact about the row*, not a branch in a template, and it
deletes all five of the old shell's if-chains. (§5g settles who owns the head:
the engine, never a theme — which is what makes the row tiers a real
distinction rather than a spelling.)

### Theme is per row; layout kind is inferred

**Theme is chosen per row** (unusual, but it is what this site does): `theme:`
in front matter or a rule default (§5b), rather than a site-wide setting.

**Layout kind follows from what a row *is***: a post or page → `document`; a
view with `group_by`/`paginate` → `listing`; a feed/sitemap view → `feed`; a
row that opts out → `raw`. So Jekyll's `layout:` front matter (37 `page`, 8
`post`, 6 `default`, 2 `light`) collapses into "which theme" plus "did you opt
out of the document wrapper" — and `page` vs `post`, the most common
distinction on the site, stops being a choice at all. (The residue of that word
on both rows and views is q33.)

### Schema drives rendering, not just display

Per-collection fields fall into three kinds, and the distinction *is* the layer
boundary:

| Kind | Read by | Example |
|---|---|---|
| **content field** | layout | `title`, `date`, `tags` |
| **render directive** | the renderer | `toc: true`, `style:` (§6c) |
| **layout hint** | the layout | `wide` |

One declaration then drives filters, `<head>` generation, layout requirements
and validation — so "layout `document` requires `date`, but collection `pages`
has no `date` field" becomes a load-time error like every other constraint
(§4). §5b's `.schema.toml` is where that declaration ended up living.

### The renderer emits hooks, and that is not a layering violation

`{% image right foo.png %}` is the author saying "this floats right". The
renderer emits `class="image image--right"`; the theme decides what that means.
The rule is that a class is a **contract**, never a CSS implementation detail.

### What this cost: chrome parity

Redesigning layouts changes the chrome HTML, so `diff` cannot verify it. That
was affordable and the budget was spent once, on §5e: **bodies verified by
machine** (327/327 post content regions byte-identical across the cut),
**chrome verified by eye**. URL parity was untouched throughout — routes are
§4, independent of presentation.

## 5b. Tree overlays: styles, slots and schema declared by position

> **Status.** The **schema leg is built** (`schema.rs`, forced by §7a's
> recipes and books): `.schema.toml` declares typed fields
> (string/int/bool/list/**image**) for its subtree, resolution accumulates
> nearest-wins like markers, and a governed row's extra front matter is
> *validated* — undeclared key or wrong type is a load error naming the file
> and the knowns. Image-typed fields feed the thumb pass and the `hero` part.
> Ungoverned rows stay as tolerant as ever. **Per-row themes are built**
> (a `theme:` field cascading via rule defaults — §5a's "theme is chosen per
> row", with a theme registry and per-theme stylesheets), including
> **subselection**: `theme: "recipes:spicy"` renders through `recipes` with the
> tokens space-joined into a `subtheme` shell part, and CSS subselects via
> `[data-subtheme~="spicy"]` — zero new engine machinery. The **`.style.scss`
> leg remains unbuilt**, and the `.slots/` leg was **absorbed by §5e**: a slot
> fill needs no templating, because "an empty part deletes its element" *is*
> the conditional it wanted.

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

## 5c. A view is a query; a route is where it lands

§5 declared views as generators: each one had a `path`, and routes were the
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
post list?" — `blog_index` and `tag_index` excluded drafts, `monthly_archive`
and `/` excluded hidden *and* drafts, and **the feed excluded only hidden, so
it shipped drafts.** Nobody decided this; it accreted. Transcribing it
faithfully into `grackle.toml` also transcribed a bug (`monthly_archive`
written `!draft`, dropping the `!hidden` its template had), and no diff could
catch it, because there was nothing to catch it with: the corpus has 0 drafts
and 0 hidden posts, so the flags are pure potential energy.

So: **one named set**, and everything composes over it.

```toml
[sets.published]          # query only: no route, no layout
from  = "posts"
where = "!draft && !hidden"

[routes.blog_index]  from = "published"  paginate = 5  paths = [...]
[sets.latest]        from = "published"  limit = 3
```

Fixing all five was provably free — build output stayed byte-identical, because
nothing is filtered today. It stops being free the first time a draft exists,
which is the point.

### Three shapes, one concept

| shape | route | layout | example |
|---|---|---|---|
| named query | — | — | `published` |
| embeddable | — | ✓ | `latest` |
| materialized | ✓ | ✓ | `blog_index` |

`path` is optional — its presence is what makes an entry a `[routes]` rather
than a `[sets]`. `from` may name a collection, `*`, or **another query — but
only a query-only one.** That restriction is the whole reason composition stays
simple: allowing `over = "blog_index"` would raise "is `paginate = 5`
inherited?", and every answer surprises someone. Compose over things with
nothing to inherit. Cycles, unknown names, and composing over a materialized
view are all load-time errors naming the view.

### Members: the match this deleted

Each route carries `members`: the rows it materializes, decided once by the
declared query. Before it existed, `build.rs` re-derived them in a `match` on
the view *name* — re-implementing `where`/`group_by`/`paginate` in Rust,
including a hardcoded `per = 5` beside a `paginate = 5` it never read. That is
exactly how `blog_index` and its config could silently disagree. The renderer
now iterates `members` and matches only on the *layout kind*: layout kinds are
code, view names are the user's.

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
grackle.toml   [sets.latest] from="published" limit=3 layout="link_list"
  ↓ source/views.rs   routeless + ungrouped → one row set → db.views["latest"]
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
§8a exists because a stale reference lied to us by 17 points. To refresh it,
stash the change first.

### Grouping is one operation *(generalized 2026-07)*

"Isn't group_by just the same thing as tag?" (Matt) — yes, and the question
deleted two-thirds of the mechanism. `group_keys` had three hardcoded specs
(`tags`, `date.year`, `date.month`); they were one operation — **group by a
typed schema field**, read through the same `filter::Row` access filters use —
instantiated three times. A `List` field multi-keys (one group per item),
scalars single-key, `Null` means absent from the partition (an undated row
under a year grouping ≡ a course-less recipe under a course grouping). The date
specs survive as aliases for the `year`/`month` fields the filter schema always
had. Proven the strong way: the main site's three groupings are
**byte-identical through the general path**. Every grouping exposes `{key}`
plus a param named after the field; group chains are load-checked against the
base schema; and grouped views work over any base. Residue, kept knowingly:
`month_name` is a display derivative special-cased on the `month` field until
§5f formatters give it a home.

### Subdivision: `from` a grouped route refines its partition *(built 2026-07)*

A grouped view is a partition of its base; a grouped view **`from` a grouped
view is a finer partition of the parent's groups** — GROUP BY year, month,
expressed compositionally:

```toml
[routes.yearly_archive]
from     = "published"
group_by = "date.year"
path     = "/blog/{year}/"
title    = "{year}"

[routes.monthly_archive]
from     = "yearly_archive"          # subdivision: year key comes from here
group_by = "date.month"
path     = "/blog/{year}/{month:02}/"
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
   next to the query it names. Grouped params render through §6f's enum
   records: `{key}` wears the record's localized *name*, the URL wears its
   *slug*, keys and params keep the id.

**URLs are derived values, all the way down** *(q32, settled 2026-07)*:
producers take URLs and never construct them. Pagination links render from
the owning view's own `paths` templates (locale-prefixed like the routes
were); tag pills render from the tags-owning view's template
(a posts collection's `tags = "<route>"`, falling back to the unique
tags-grouped view — ambiguity is a load error, no tags view means unlinked
pills); slugs apply at exactly one seam per base kind (`route_value`).
i18n forced this settlement: the hardcodes had already grown locale
prefixes in two places. One deliberate visible change rode along:
pagination links gained the route template's trailing slash — 66 files,
every byte one substitution. The collection's own `crumb`/`index` fields
are the last non-derived names in a trail (q46 proposes dissolving them
into §5h's landing chain).

Composition rules, enforced at load: `from` may name a set
(unchanged) or a **grouped, unpaginated** view — and the composer must then
be grouped itself, because subdivision is the only defined meaning; a
non-grouped view over a grouped one is an error. **Pagination × subdivision
is deliberately punted** (open question 30): a year *could* paginate while
months subdivide off the year's root, but `/blog/2022/page/2/` and child
routes then share the year root's URL namespace, and that conflict deserves
real thought rather than a rule chosen in passing.

### The split the section title always implied *(built 2026-07-19)*

"A view is a query; a route is where it lands" was a sentence in this document
and one `[views]` section in config, where the only way to tell the two apart
was whether `route` happened to be present. It is now the shape: **`[sets]`**
for a query that never lands, **`[routes]`** for one that does.

Measured across both sites' 23 queries before deciding: `path(s)`, `title`,
`crumb`, `shell`, `template`, `content`, `intro`, `featured`, `paginate` and
`group_by` NEVER appear without a route; `from`, `where`, `match`, `order_by`,
`limit`, `layout` and `variant` appear in both. Ten keys are meaningless
without a URL. `group_by` is the one worth saying out loud: grouping exists to
produce one route per key, so a grouped query with nowhere to land has no
meaning.

**One keyword, not two.** A draft had `under` for subdivision; dropped, because
`Config::query` already derives selection-from-subdivision from what the name
refers to and errors on every ambiguous case. `from` names a collection, a set
or a route, and what it names decides what it means.

**One namespace, now enforced** — which exposed a latent collision: the
resolver tried views *before* collections with no guard, so `[views.blog]`
beside `[collections.blog]` silently shadowed the collection and made it
unreachable by name. A name now lives in exactly one of the three.

**Profiles split the same way**, and say more for it: relaxing
`[profiles.drafts.sets.published]` patches a QUERY, relaxing
`[profiles.drafts.routes.search]` patches a LANDING.

## 5d. Templating: there is almost none, so don't build for it

The recurring question — a real template language, or §5b's slots, or hardcoded
Rust layouts — is a false trichotomy. It dissolves once you count what the
site's templates actually contain. Classifying ~60 liquid constructs across
`_layouts/`, `_includes/`, `blog/index.html`, `atom.xml` and `index.html`:
**17 were a query** (`for post in site.posts` + `unless post.draft`), **22 were
a schema fact** (`if page.date`, `if noindex`, `if multipost`), **12 were
argument passing** (`assign post = page` exists solely because `article.html`
wants its variable called `post`; `capture margin_html` exists because Liquid
has no parameters), and **8 were real display iteration** — of which only
**three** are genuinely "loop over a list and emit markup": breadcrumbs, tag
pills, pagination nav. All three are components.

The site does not have templating. It has a database and four presentation
layers, and Liquid was the only vocabulary available to say so.

### The rule

> **A template may not contain control flow.**
> Needs a loop → it is a view. Needs a conditional → it is a schema fact, or a
> different layout kind.

This is a **tripwire**, not an aesthetic. Every `{% if %}` you want is a missing
schema field; every `{% for %}` is an unnamed query. The census is the
evidence: it holds for ~57 of 60, and the 3 exceptions are components.

It also preserves the discipline the rest of the design has. `filter.rs` is a
*typed* expression language with load-time checking and "did you mean"
suggestions. A template language throws that away — untyped, runtime-resolved,
`{{ post.titel }}` silently rendering nothing. The ethos here is load-time
errors, not 404s; Liquid is the opposite by construction.

`/` was the existence proof, and it was the hardest page on the site: HTML,
typed holes, **zero control flow**, matching the reference exactly. Its
nine-line counter loop became `where` + `limit`.

**So `liquid` was retired by never being taken** — §9a had listed it as the
biggest dependency risk. `tags.rs` is a targeted expander, not a liquid
implementation, and the whole vocabulary it needs is:

| construct | uses at the port | note |
|---|---|---|
| `{% image %}` | 194 | §6a |
| `{{ site.baseurl }}` and its `prepend:` form | 12 | whole shapes, not a filter pipeline |
| `{% view %}` | 1 | §5c |
| `{% include %}` | 1 | parameterless only |
| *(`{% post_url %}`)* | *51* | **retired 2026-07-20** — a foreign key into `by_name`, dissolved into §6a's plain source links |

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

A widget is the block-level sibling of `{% image %}`: a named expansion, not
control flow, so it stays inside the rule above and needs no template engine.

The concrete motivation was a real bug. The callout boxes on the 2026 posts
were authored as raw HTML wrapping `<div markdown="1">` — a **kramdown**
feature ("parse my inner content as markdown, then drop the attribute") that
comrak has no concept of: it does not recognise `<callout><div …>` as a block,
so the `<div>` opens *inside* a paragraph and the box collapses. The pre-widget
fix was hand-normalising the source into a form both parsers accept, which
works but pushes a formatting rule onto the author and leaves a Jekyll-ism in
every post.

A widget dissolves it instead. The author writes `{% callout %} … ordinary
markdown … {% endcallout %}`, and it expands *before* markdown into the wrapper
the theme styles, body spliced in with blank lines around it — so comrak parses
it as markdown with no `markdown="1"` and no lazy-continuation trap. The raw
HTML and the kramdown dependency both leave the source entirely.

The rule the registry enforces: still no arguments, still no control flow. An
argumentful or conditional widget is the tripwire that says "you want a
template — you don't." A widget is also just another producer of an
`HtmlBlock`, so §6d's block-splitting and rewrites see through it unchanged.

**The rule's two former weaknesses are both closed.** Pagination was the best
stress test of "component, not template" — a genuine range loop plus a
three-way conditional — and fell out as ~40 lines of Rust, semantically
identical to the reference nav. And "themes are Rust", the one weakness this
rule looked likely to break on, was dissolved by §5e: themes became directories
of data.

## 5e. The presentation synthesis: parts fill slots, CSS does the geometry

**Status: built, and the synthesis is real.** Layout kinds emit part maps
(`parts.rs`): named, typed parts — `Text`/`Html`/`Url`/`Stream`/`Map`/`Flag` —
in canonical order, names and types asserted against a per-kind `schema()`,
producers never touching `Site` (URLs are root-relative; `baseurl` is
presentation). The fragment binder (`binder.rs`) is a strict parser plus the
hole algebra plus complete load-time validation. `themes/default/` is a real
directory — shell + kind fragments + `theme.scss` — and `_sass` is superseded
by it. `parts::canonical()` renders any part map with no fragments at all, and
`Fragments::render` falls back to it for any kind a theme declines to arrange,
so **themes are partial by construction**: a theme with no fragments IS the
null theme and needs no directory, and a new theme can start from one fragment
and grow.

Verified exactly as §5a priced it: **bodies by machine** (all 327 post content
regions byte-identical across the cut), **chrome by eye**. What the cut retired,
each a defect this document had named: two breadcrumb markup shapes (one
`crumb` fragment now), two document shapes (one fragment; `[data-tree]` is two
CSS declarations), `body.multipost` (summary styles select on
`[data-kind="summary"]` context), and the Rust shell — with it, the whole
"themes are Rust" weakness (§5d) and the five presentation seams this section
was written to close.

Three things the build added or corrected:

- **The completeness falsifier runs on every real row, in the test suite.**
  Every part's bytes must survive into the canonical rendering — if a part can
  vanish, no fragment can put it back — checked over the actual corpus (327
  posts, 180 listings including pagination maps, every tree-page shape) on
  every `cargo test`.
- **`PartType::Url`.** The null theme should be navigable, which forced the
  admission that url-shaped scalars are a *type*, not a naming convention.
- **Dark mode landed as pure CSS, then was deliberately removed** — a
  custom-property palette plus one `prefers-color-scheme` block, zero engine
  involvement: the proof this section promised that CSS does the lifting. It
  was backed out to unconditionally light because the *content* assumes a white
  background in a lot of places (screenshots, diagrams, legacy pages). The
  palette vars stay; a dark value set is one block away once the content can
  take it.

Two smaller notes worth keeping. The **placeholder-link rule** earns its keep
everywhere at once — disabled pagination arrows, the current page tile and
inert crumb tails are all `<a>` without `href`, styled via `a:not([href])`,
with `aria-current` on an attribute hole so the CSS gap picker and a11y share
one part. And **identity slots are live**: `.slots/nav.md` and
`.slots/copyright.md` at the root mean no theme file contains the site's words
— proven the day a one-line edit to `copyright.md` moved the year across 500+
pages with no theme file touched.

Slots, incidentally, already existed in five places under five names — shell
regions, tree-filled `.slots/` files, `Option<&str>` function parameters, note
placement, `{% include %}`. The synthesis is that **a slot is a named, typed
hole: layout kinds produce fills, themes produce placement, and nothing else
exists.**

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
   variant hook — q24, settled), not a requirement. The loop lives in the
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

The §8a two-shapes tension dissolves: one `document` kind, one markup, and
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

The model's bet is that *all geometry* lives in theme CSS, so "can it do layout
X" decomposes into "can modern CSS express X" (a browser question) and "does
the part schema carry what X's CSS needs" (the engine's only obligation).

Auditing the archetypes — document with margin or sidenotes, album gallery,
Pinterest masonry, magazine full-bleed, timeline, dense index — surfaced **four
genuine gaps, and every one resolved to "add a part or fact", never to control
flow.** That is the model behaving as designed: the gallery archetype didn't
demand a template feature, it demanded a schema field. Three are built:

- **The `hero` part** (q23) — a `Map("figure")` on `document`, sourced from the
  image-typed schema field named `cover` (beats `image`, §5b), thumbnailed with
  dimension facts; the card preview consumes the same source. Still arriving
  with their consumers: the first-image-block fallback, and the *group* hero (a
  `cover.*` file beside the group).
- **Per-view fragment variants** (q24) — below.
- **Dimension facts on images** (q26) — ✅ **closed 2026-07-21.** Gallery
  figures, heroes and card previews took theirs from the thumb pass; body
  images followed when `{% image %}` learned to emit `width`/`height` at
  expansion (442 of 468 site-wide; the 26 without are external affiliate
  pixels, never thumbnailed, so nothing to measure). The engine had measured
  them all along — `build.rs` projected `Thumb.dims` away one line after
  computing it, so `tags::Ctx` structurally could not emit what was known.
  Paired with theme CSS: dimension attributes against a `max-width`
  constraint with no `height: auto` render squashed rather than reserved.

The one still open is **per-block facts** (q25): full-bleed needs one block to
escape the content column — a block-level directive becoming a `data-`
attribute on that block, which slots straight into §6d's block stream.

**The one honest limit is masonry.** True Pinterest packing with strict reading
order is the single archetype CSS cannot fully express yet — native masonry is
still settling in the working group and is not Baseline. Interim: CSS `columns`
(reading order runs down columns) or row-span tricks fed by the dimension facts
above. When native masonry lands it is one declaration in one theme file, zero
engine work — the engine's only job was to have shipped the facts.

The meta-point, worth stating as the completeness criterion: **§5e turns "can
we do layout X" from an engine question into a browser question, and the
engine's obligation becomes crisp — every part or fact a plausible theme could
need must be in the schema.**

### Variants and the one preview kind *(q24 + q36, built 2026-07)*

Two settlements that arrived welded together, because the second forced
the first:

- **One preview kind** (q36): a card is a view's *projection* of a row,
  and so is a summary — they differ by what the row HAS (posts:
  date/tags/content blocks; books: cover/note), not by what they are.
  `summary` is the one kind (schema gained `src`/`width`/`height`/`note`,
  presence-driven); `card`/`card_list` are deleted; `card_list` folded
  into `listing` as a `featured` slot any listing may fill. The
  main-site chrome cost was zero — the byte oracle stayed clean.
- **Fragment variants** (q24): a fragment file's stem is its *name*; the
  stem before `--` is its *kind* (`summary--card.html` binds `summary`).
  A view declares `variant = "cards"`; rendering tries
  `{kind}--{variant}`, falls back to the base fragment, then canonical —
  partial themes throughout. In fragments, `data-fragment` selects a
  variant for stream/map children; being explicit, it must resolve at
  load, to the right kind.

Documented as the rule, not fixed: the canonical fallback is
**all-or-nothing per subtree** — canonical rendering never consults child
fragments; a theme opts into per-kind fragments from the parent down
(this is why the recipes theme needed `document.html` before its
`crumb.html` was consulted).

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
| `where =` | `bool` over the row schema | built — the §5 language, already CEL |
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
[sets.published.fields]
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
  equivalent to omitting `where` — recognised before the parser runs, not
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
[routes.search]
from  = "*"
path  = "/search.bin"
shell = "search"
where = '(kind == "post" || kind == "page") && !draft && !hidden'
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

### Row shells: a row picks its own wrapper *(q44, built 2026-07-19)*

A row declares `shell:` andpicks its own wrapper: **`none`** (the body IS the output — no
skeleton, no theme), **`light`** (engine skeleton, canonical parts, no
theme chrome) or **`html`** (the theme). Closed vocabulary, checked at
load and named with the file, because a typo'd shell would otherwise
render the wrong tier silently — the failure this document keeps
finding. Absent, the legacy `layout:` still chooses, so nothing
migrated and the main site is byte-identical.

`none` is the one that adds a capability rather than a spelling: an
imported artifact can now carry front matter *and* emit itself. Before,
adding front matter to a full HTML document nested it inside a second
`<html>`; the only way to ship it verbatim was to have no front matter,
which meant it was not a row at all — no title, no metadata, invisible
to every query. The example's `demos/pane.html` is the occupant: 521
bytes of its own document, with a `title` the database can see.

A `shell = "none"` row's content is raw HTML, so everything downstream
reads it as such. Adding one exposed a search bug that predated it:
`strip_tags` dropped tags but kept what sat between them, so `<style>`
and `<script>` content became searchable terms — the shipped index
carried `rgba`, `fafafa` and `ffffff` from §6c's three styled posts.
**Fixed 2026-07-19** in `search-core` (raw-text elements skip their
content): those terms are gone, the index lost 1.2 KB, and prose
survives — two posts discuss `margin` in their text and stay findable
by it. The example's pane row is `hidden`, which is the honest way to
keep an imported artifact out of the index; that flag only started
working on pages the same day (§4b). Also open: the
atom/sitemap serializers becoming true part-map
consumers (a feed entry IS a document-parts subset), and a `json` shell
when something wants one (though a script shell now covers the
experiment: the payload already is JSON — `cat` is a json shell).
Versioning the script-shell payload schema rides with the first
non-experimental consumer.

### Row tiers: where a row leaves the pipeline *(settled 2026-07-19)*

The tiers are not alternatives to something else — they are **exit points on
one pipeline**.

| tier | head | body | skeleton |
|---|---|---|---|
| object | — | bytes off disk | none |
| `none` | — | rendered parts, emitted verbatim | **none** |
| `light` | minimal — 85 B (title, charset), 118 B when `noindex` | canonical parts, no theme | engine |
| `html` | full — 739 B (og:\*, canonical, author, css, favicons) | theme fragments | engine |

Two questions sound like they dissolve the row `shell:` field. Both are
answered by that framing.

**"Aren't `shell: none` rows just objects?"** They emit their bytes verbatim,
which is what an object does — but that is the *last* step and the only one
they share. A `shell: none` row enters the pipeline completely: tag expansion,
object resolution, thumbnailing and the content-addressed asset pipeline all
run, with load-time enforcement throughout (measured by putting
`{% image %}` in the example's pane row: a bad path **fails the build**, a good
one ships a `/static/` URL). **Objects are what that URL points at.** They
never enter the pipeline; their bytes come off disk, they are selected by
extension, and `object_schema()` has no title, flags, locale or tags.
**"Object" means no schema participation; `shell: none` exists to get schema
participation without a wrapper** — opposite requirements that happen to agree
on the final step.

**"Isn't `shell: light` just `theme: light`?"** No, and the reason is the
`<head>`. A theme chooses BODY chrome; the head is computed from the schema
(§5a) and **no theme may write it** — the root shell exists to enforce exactly
that. So the head is the one thing theme selection cannot vary, and the head is
precisely what separates `light` from `html`: 85 bytes against 739.
`theme: none` fails for a sharper second reason: **the null theme still emits a
valid document**, deliberately, so a `theme: none` that emitted no skeleton
would reintroduce the bug the root shell was built to kill. `shell: none` may
emit no document *because the row promises its body already is one*.

**One correction, because it makes the question fair:** `light` is not "the
null theme", though this document twice said so. §5e's null theme is a
**theme** with no fragments — full computed head, stylesheet link, through
`Theme::Default`. `light` is a **tier** — it bypasses the theme registry and
takes `light_head`. They agree on "no body chrome" and differ on the head.
There is no `themes/light` directory.

### Why exactly these tiers *(2026-07-19)*

**Two bits, and one incoherent corner.** The real choice is two independent
questions — does the row have **database identity** (front matter present, so:
schema, content rules, link graph), and does the **engine construct its
document** (`shell`). That 2×2 has only three corners: no-identity ×
engine-builds is *incoherent*, because building a document means computing a
`<head>`, a head is computed from schema, and schema is what identity *means*.
There would be nothing to wrap. It is also mechanically unreachable. So the
2×2 collapses to a three-state chain, and the chain is a **result rather than a
modelling choice**: identity is a *precondition* for the other bit.

**The guarantee ladder.** Read upward, each tier is what the engine *promises*
about the bytes: **object/static** — nothing, the bytes are yours; **`none`** —
the content rules ran, document validity is *your* promise; **`light`** — a
valid document, minimal facts; **`html`** — a valid document, the full computed
head, a theme. This answers `theme: none` in one line: **a theme cannot lower a
guarantee it did not make.** The root shell is engine-owned precisely so the
validity promise sits *above* the theme layer. It also names the honest risk:
`none` is the only tier where a malformed row yields a broken page with nothing
to catch it.

**The escape hatch, and its tripwire.** q16 established the discipline: an
escape hatch per layer, with a tripwire on how often it is taken. grackle has
one per layer — raw HTML through markdown, the null theme under the binder,
`{% %}` widgets under the no-template-language rule, `render.unsafe_` under the
AST — and **`shell: none` is the shell layer's**. So: *if a meaningful share of
rows need `none`, the tier vocabulary is wrong and the engine is failing to
build documents people actually want.* Today it is 1 row of the example's 21
and 0 of the main site's 227. Re-read that ratio each time an imported artifact
lands, not the tier list.

### One word, two axes *(named 2026-07-19)*

`shell` names two unrelated things: **row `shell:`** (`none | light | html` —
the wrapper tier above) and **view `shell =`** (`atom | sitemap | search` plus
`[shells]` script shells — the outermost serialization). The value domains are
disjoint, neither validator accepts the other's words, and they are read in
disjoint passes, so **no row ever meets a view's shell as a shell**. A naming
collision, not a design flaw — nothing can drift because the two never meet.
What it costs is the sentence a reader spends deciding which is meant. If ever
renamed, the row-level one is the **tier** and the view-level one keeps
`shell` — but that touches a documented config surface for one row's benefit.

## 5h. Landings: a view owns the URL, a row may own the words *(q45, Matt's shape; built 2026-07)*

### The disease this cured

"The page that stands for a set" had **four implementations and no
owner**: view roots (`/books/` — a route, not a row), index pages
(`recipes/index.md` — a row routed by `**/index.*`), the home page (a row
plus special cases), and `collection.index` (a raw URL in config). Four
mechanisms answered "what's above me" four ways, and the symptoms were
live: no Books crumb on book pages, a duplicated `Accueil` on French
trails, `/fr/recipes/` a 404 while `/fr/blog/` existed, three
`stem != "index"` filters guarding queries against their own landing,
and hand-maintained listing prose drifting from the schema it duplicated
("serves 2, 25 min" written beside a database that knew). Diagnosis: a
landing is a **projection** plus, sometimes, **prose** — an index page
is a row pretending to be a view; a view root is a view missing its
row's prose. Every symptom was one half missing or hand-forged.

### The rule: the engine never guesses the arrangement

Either the theme owns the arrangement, or the author does. Three tiers:

- **Bare**: query + route + `listing`. What `/photos/` is.
- **Declared text**: the view declares `intro` — a `LocalizedStr`
  rendered as *markdown through the locale-aware link resolver* (a
  `view:` link in an intro gets strict validation; no browser-agreement
  bypass — config prose has no directory) — filling an `intro` slot on
  the listing layout. Empty collapses (the fragment glues the intro div
  to the items line, because the binder deletes elements, not lines —
  oracle-clean). Per-key intros come from §6f's enum records: a grouped
  route whose leaf value declares `intro` gets *that* prose, beating the
  view's — the course archive introduces the course.
- **Referenced content**: the view declares `content = "recipes/index.md"`
  (mutually exclusive with `intro`). The row becomes the whole body and
  **must place `{% view <owner> %}` itself** — a load error otherwise
  (the rows would be unreachable), scoped to views with a query. The
  self-embed is **route-aware**: page 2 renders page 2's rows,
  `/fr/recipes/` the French partition (a sentinel goes through tag
  expansion; the slice substitutes after the markdown pass, so embed
  HTML never meets the parser). Embeds of *other* views keep whole-view
  semantics. Intro and content render on every page of a paginated
  landing: the prose is the landing's face; the slice is what changes.

### Claiming

A referenced content row is **claimed**: no standalone route, and out of
every query **structurally** — by ownership, not the `stem != "index"`
naming convention, which died with it. The row keeps everything rows
have: front matter (its `title:` beats the view's — explicit beats
derived, per row), its rule-derived theme (the landing wears its
section's clothes), its directory (slot fills resolve nearest-wins from
there), suffix localization with default-locale fallback. It keeps its
*title and the landing's URL per locale*, so the ancestors walk still
crumbs it and source-path links resolve to the landing (source links
gained the view-link locale invariant: prefix-and-check, fall back).
Claiming is **declared, never discovered** — a convention would claim
rows silently — which makes migration incremental: unclaimed index pages
behave exactly as before, the main site is untouched, sections lift one
`content =` declaration at a time. Load checks: the path names a row,
one owner per row, intro XOR content, materialized views only,
must-place. One deliberate semantic change: claimed rows leave the
backlink scan — **membership is not citation**.

### The chain: URL nesting is parent derivation

`ancestors()` answers "what's above this URL" in two steps per level:
a rendered **page row** at the parent URL (mode-B landings match here,
row title winning), else a **materialized landing route** — the view's
crumb-else-title at the route's locale. Locale-prefix homes are skipped
(`/fr/` is not a directory; Home is the trail root's job — this killed
the duplicated `Accueil`). **Listing trails climb the same chain**, so
`Home › Recipes › Dinner` fell out of moving the course archives under
`/recipes/`, with zero source edits (every `view:` link re-derived).

"Materialized landing route" is tested as **`params` empty, page ≤ 1**,
and the first half of that is a q46 correction worth keeping visible.
It was `key` empty — which reads right and is wrong: group keys land in
`params`, but a *paginated* view also stamps a synthetic `"page 1"` key
on its first route, so `/blog/` was invisible to the climb. Nothing
noticed, because `/books/` and `/recipes/` don't paginate and `/blog/`'s
crumb was arriving from config. A duplicated fact had been holding a
broken derivation upright — which is the general argument for q46, found
by doing it.
**Theme rides the same logic one level up**: a tree-backed listing
whose members unanimously wear one theme *name* wears it too (subtheme
tokens are one row's dress and never lift; mixed or theme-less members
keep the default; posts and objects carry no theme, so the main site
is untouched by construction). The landing's language switcher derives
from the owner's materialized routes — a fallback-prose landing is
still the French landing.

### The collection stops naming itself *(q46, settled and built 2026-07)*

`collection.crumb`/`index` are **gone**. They stated, in the collection,
what the collection's landing view already declares: `crumb = "Blog"`
beside `blog_index`'s `title = "Blog"`, `index = "/blog/"` beside its
route. Two spellings of one fact, kept in agreement by hand.

The dissolution is the chain doing its job. `trail_root` is now Home and
nothing else; every crumb between Home and the current page comes from
the climb, for rows and listings alike. A post at `/blog/2022/12/16/x`
finds the `/blog/` landing on the way up and wears the *view's* name.

What made this safe is that the two producers cannot collide: the climb
matches ungrouped landing routes only, so it steps past `/blog/2022/`
and `/blog/2022/12/` entirely, leaving them to `trail` — whose chain
renders from a row's own group keys and is genuinely non-derivable from
a URL. `trail` stays for exactly that reason.

Locale falls out instead of being handled. The old code built
`/fr` + `index` by string concatenation and hoped the result existed;
the climb *finds* `/fr/blog/` as a materialized route, or doesn't, and
either way says something true. §6f's dangling-crumb edge closed with
no code of its own.

**Accounted, byte for byte.** Both sites rebuilt: every post trail, tag
archive, year and month archive, feed and sitemap identical. The whole
visible change is one line on `/blog/` (and `/fr/blog/`) — the crumb is
the same word in the same place, now inert instead of linking to the
page you are already on. The listing had been naming itself twice, once
as a self-link from the collection config and once as its title;
`page 1` had suppressed the ordinary inert tail that every other listing
renders, so removing the duplicate *revealed* the convention rather than
breaking it.

### Honest edges, pending

- An explicit `parent =` for when URL nesting lies: unneeded so far.
- Orphaned translations (`index.fr.md` with no French rows has nowhere
  to render) should warn.
- Mode-B prose is not searchable (landing routes are structurally
  excluded); keep until someone misses it.
- A variant fragment lacking a hole drops that part **silently**
  (`listing--cards.html` swallowed an intro until it grew the slot) —
  "never ship what a theme hides" wants a load-time warning for schema
  parts no fragment of a theme places.
- Home and the manual haven't lifted yet — home is the queryless
  landing (`route = "/"`, `content = "index.html"`, no rows to strand;
  q37's board hangs in this frame), the manual waits for the section
  tree to be a landing's listing. The example search's one remaining
  `stem != "index"` filter survives exactly until they do.

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

[[collections]]
name = "blog"
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

⚠️ **Specced, not built** *(measured 2026-07-21)*. Everything above describes
a design, not the code. `thumbs::one` joins `{% image %}`'s literal argument to
the site root, so a bare name resolves to `root/burrs.jpg`, misses, and fails
the build with `{% image %} source not found` — it fails loudly, which is why
nobody has been bitten, but it does not bubble and it does not consult a
bucket. `[objects] bucket` is parsed by config and **read by nothing**; both
sites declare it. `by_name` is built every load and read only by
`query stats`. All 194 corpus invocations pass a path, so the unbuilt branch
has never been reached.

An earlier version of this section claimed "bare names work for posts today,
with no restructuring and no bucket configuration at all". They do not. This is
the same class of drift as §9b Round 3's *declared-and-ignored* `layout` names,
and the third instance found in one week — after `grackle diff`'s URL-parity
claim and the heading anchors "the real pipeline strips". The tour's own
worked example (§0 step 4, `burrs.jpg` resolving to a sibling) is aspirational
for the same reason.

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

The day the example grew locale prefixes, slugged tag routes and templated
pagination, hand-typed URLs in content became lies waiting to happen — URLs are
DERIVED values here. Matt's rule closes the gap, and it is this section's
principle finishing its job: **authored links reference what the database
owns.**

1. **A link to a row references its source file** — relative to the linking
   file (`carbonara.md`) or root-relative (`/recipes/carbonara.md`) — and the
   engine renders the URL, exactly as `{% post_url %}` always did for posts. An
   unknown source is a build error naming the file, with a closest-match
   suggestion.
2. **A link to a view uses `view:` syntax** — `view:gallery`,
   `view:recipes_by_course/dinner` — rendered through the owning view's route
   template (tag slugs applied, multi-level chains keyed positionally),
   locale-aware, and verified against the route set: a typo'd key errors
   LISTING the keys that exist.
3. **`[links] policy`** grades enforcement. `strict` — **the default since
   2026-07-20** — errors on raw internal URLs, answering with the correct form
   (`"link the source instead: /recipes/carbonara.md"`), and on links matching
   no source or route at all. `loose` resolves the new forms but leaves both
   alone, and survives for importing an unconverted corpus.

**What flipping to strict caught, which is the argument for it.** Two engine
gaps: **a link to a directory means its index** (`saturn/` is
`saturn/index.md` — the oldest convention on the web, and the resolver did not
know it, so strict called 35 good links dangling), and **`javascript:` is code,
not a path** (a bookmarklet href was read as a relative source path). Two real
defects: `/blog` without a trailing slash was not the canonical URL and nothing
said so, and `/demos/dress` and `/demos/adventure` did not resolve, so the rows
they pointed at never registered the citation — **25 pages gained a
`Linked from` section** that had been silently dropped. A lenient default was
costing real backlinks, quietly.

✅ **Raw HTML joined the net, 2026-07-21** (§6d stage B). `.html` page bodies,
`.html` slot fills and raw-HTML landings never met comrak, so the resolver
never saw their links; a narrow `lol_html` pass now walks them. It caught the
`/blog` above in `index.html` and `_includes/social.html` — the two files
strict had never been able to read — and closed the example's `index.fr.html`
residual.

Resolution is a comrak AST pass over Link nodes (`render_doc_with`), per-row,
against a `LinkSpace` built once per build (source→URL over all three tables,
the route set, and URL→suggested-form for the strict errors). The byte-oracle
rule that made it safe on a 20-year corpus: **the engine rewrites only where
the browser would get it wrong** — a relative link whose source-resolution and
URL-resolution agree (the `downloads/foo.zip` idiom, 27 files' worth) ships
byte-identical; `.md` references and cross-dir links get the engine's answer.

**`.slots/` fills render THROUGH the resolver, per consuming page**: fills store
raw source and render at page time with the page's locale, so one `nav.md` of
`view:`/source links serves every locale — `view:blog_index` is `/blog/` on an
English page and `/fr/blog/` on a French one — and `nav.fr.md` (the row suffix
convention, no config) exists only to translate labels.

Pending here: the closest-match suggester is stem-exact, not fuzzy; strict for
the main site rides the publish-cutover migration.

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

Derived assets are exempt from URL parity (q12), and the current scheme was
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

What this replaced: Jekyll's `lsi: true`, a dominant chunk of the 90-second
build, recomputed from scratch every time because Jekyll had no
content-addressed cache for it. LSI's related-posts were mediocre and `diff`
cannot judge relatedness (§8 lists them as knowingly-inexact), so this is the
one place the port is deliberately *better* rather than equivalent, at no
parity cost. Embeddings never publish — build-time only, no URL, no bytes
shipped.

⚠️ **The embedded body is RAW MARKDOWN, so link syntax is semantic signal.**
Measured 2026-07-20, retiring `{% post_url %}`: rewriting 56 links to
file-relative form changed no rendered href at all, and still reshuffled
`Related` on **37 of 327 posts** (one lost the section outright), because
`{{ site.baseurl }}{% post_url … %}` and `../2010/….md` are different text.
Every rendered byte outside the relations blocks was identical. So
related-posts are a function of markdown *syntax*, not of prose: a reflow, a
typo fix or an `{% image %}` swap silently reshuffles them. The fix is to
embed the rendered plain text; it is deferred because it reshuffles every list
once. **Until then, do not read "Related changed" as evidence that a refactor
changed meaning.**

### TF-IDF search index — the searcher is the same code, compiled to wasm *(built 2026-07)*

**The architecture upgraded mid-design.** Instead of a JSON index consumed by a
hand-written JS searcher — whose stemmer would be a drift-prone port of the
Rust one — the search core is **one crate** (`search-core`: stem, tokenize,
index build, rank) used by both ends. `grackle build` calls it to ship
`/search.bin` (postcard, not JSON: the format is private to the two ends of the
same crate; `grackle query search` is the inspectable surface), and the
identical code compiles to WebAssembly (`search-wasm`, a ~90 KB cdylib behind a
raw no-bindgen ABI: `alloc`/`init`/`search`). **Symmetry by construction**: the
browser stems queries with the same compiled function that stemmed the corpus,
and the stemmer is free to stay simple because it cannot desynchronize.

The page ships an icon and nothing else. Clicking it injects `/search.js`
(3.6 KB loader — bytes and pixels only; every search decision is in the wasm),
which fetches the blob and index and answers per keystroke. The **last query
token is a live prefix** over the sorted term map ("bluet" finds bluetooth) —
real search-as-you-type, cheap in Rust, awkward in the JS it replaced.

Measured (327 posts): 7,125 terms, 29,793 postings, **195 KB index built in
22ms** per build. No TF disk cache — tokenizing the corpus is single-digit ms,
so the spec'd cache would be machinery without a cost to pay for. Postings
capped at 40/term, scores TF·IDF quantised u16, title/tag hits boosted 5×,
stopworded, years searchable. First-click payload ≈ 288 KB (js + wasm + index),
all cacheable; every page's default payload stays **zero JS**.

The wasm blob and its loader are **engine assets** (`grackle/assets/`, embedded
via `include_bytes!`, emitted when a site declares a search view) — they must
version with `/search.bin`'s format, so they cannot be theme-committed; a theme
owns only the trigger and the overlay CSS. Rebuild with
`cargo build -p grackle-search-wasm --release --target wasm32-unknown-unknown`
and copy to `grackle/assets/search.wasm`. The index itself is a declared SHELL
(`shell = "search"`, §5g), so the searchable set is a query over the route
schema, spanning tables.

Embeddings answer *"what is this like"* (fuzzy, build-time, 500 KB of f32);
TF-IDF answers *"where does this word appear"* (exact, shippable, no model at
runtime). Two tools, one cache discipline.

This retired **Swiftype**: the header search used to be
`javascript:document.getElementById('st-launcher-tab').click()`, a launcher for
a third-party service, which is also why the layout carried
`data-swiftype-index` attributes and a `<meta class="swiftype">` tag. Those
left the shell with the chrome cut. The site ships **zero** JS by default, so
search loads lazily on interaction only.

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
`markdown::render_doc` parses once and yields both the whole render (posts and
the feed use it unchanged — post pages verified byte-identical) and the
top-level block sequence, where the old pipeline rendered every post twice.
**The summary is a computed field on the view's rows** — a derived column, not
a rendering attribute:

```toml
[sets.published.fields.summary]
truncate = { max_blocks = 4, max_chars = 700 }
```

`Doc::truncate` is mechanism only (blocks kept until a budget runs out, block
granularity, at least one always kept, `max_chars` counting visible text); the
*deriver* (`truncate`) discriminates the field definition and is validated at
load. **Fields flow with rows through `from` composition** the way filters do:
declared once on `published`, every listing composed over it inherits the
column; redeclaring the name overrides, nearest wins. The deriver's fact
(`truncated`) rides along, feeding `data-truncated` — which is where the ★ gets
its vocabulary (q17), gated in theme CSS rather than inferred from the DOM.
Listing previews consume the field named `summary` by convention; no summary
field in the chain means rows ship whole. `hero` (q23) and `lede` are more
derivers producing more columns.

Two wrong altitudes were corrected in one session getting here: the cut rule
started as engine code — policy belongs in config — and then as a view
*attribute*, when a summary is a property of the rows, not of the view's
rendering. **Marked not-quite-right (q31)**: deriver-as-struct-key is a stopgap
shape; if the config grows *functions*, a field wants to be an expression
(§5f), and this gets revisited rather than extended.

**Stage B, partly built (2026-07-21).** The **rewrite stage exists, narrowly**
(`rewrite.rs`, lol_html): `a[href]` resolution for rows whose source *is*
HTML — `.html` page bodies, `.html` slot fills, raw-HTML landings — which is
the one job the AST pass structurally cannot do. It is deliberately not the
rule table below: three of that table's five original use cases moved to the
comrak node, q26's dimensions became the fourth at expansion time, and neither
site wants an authored rule, so the selector language waits for its second
consumer. Still deferred: the **notes stream**, which needs its consumer —
sidenotes want a third grid column (q18).

One asymmetry the narrow stage carries, scoped to where it is unavoidable: a
raw-HTML body has `{% view %}` expanded INTO it, so on pages with an embed the
rewriter meets engine-derived URLs beside authored ones and cannot tell them
apart (comrak never had to — it sees an embed as an opaque HtmlBlock). There,
a URL already naming a materialized route is left alone rather than answered
with strict's "link the source instead"; a page with no embed gets strict
whole. The landing path needs no exemption, because its embed is still a
sentinel when links resolve — and generalizing that sentinel to every
`{% view %}` expansion is what would retire the asymmetry.

Markdown is currently an opaque blob: `content` goes into a `<section>` and
nobody can touch it. Two mechanisms open it up, and they are **not
alternatives** — they solve different problems and compose.

| | addresses by | serves | example |
|---|---|---|---|
| **Blocks** | position | *layout* — placing parts of the content | summary takes the first 2 paragraphs |
| **Rewrites** | CSS selector | *transformation* — changing content in place | wrap every `<table>` in a scroll container |

### Blocks, and the 93% that justified them

Markdown renders to a **sequence of top-level blocks** (paragraph, heading,
code, list, table, html) rather than one string. A layout kind then takes what
it needs: `document` takes all of them, `summary` takes the first few, a future
`lede` slot takes `blocks[0]`.

The justification was measured, not aesthetic. The site used to truncate
summaries in **CSS**, so every listing shipped complete post bodies and hid
most of them: of `/blog/`'s 140,884 bytes, 134,635 were post bodies and
**131,071 were shipped then `display:none`'d — 93% of the page**. (There was a
*second*, independent truncation too: a `max-height` clip with a fade mask, so
only ~7 lines were ever visible. The DOM cut was far more generous than the
visual one.)

With blocks the summary simply never emits blocks 3..n. **Measured after:
`/blog/` 160 KB → 15.7 KB, `/blog/tags/rust/` 180 KB → 11.3 KB (93.8%)**, and
the CSS truncation rules are deleted. Corpus-wide the saving averages 74.5%;
`/blog/` scores higher because the posts it shows are the long ones.

**326 of 327 posts satisfy `concat(blocks) == markdown_to_html(src)` byte for
byte** — comrak's `format_html` takes any node, so blocks are a loop over
`root.children()`, not a parser change, and a summary is then a literal
*prefix* of the document, which the harness can prove rather than eyeball. This
is now a corpus test rather than an assumption. **The single mismatch is
footnotes** — see below.

Blocks stay **internal to layout kinds** — they are not exposed to templates. A
template iterating an AST is a trap (Hugo keeps `.Content` a string for exactly
this reason); templates get *slots* and *rewrites*, addressed by name and
selector rather than by walking a tree.

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

> **Status: both axes are built** (`outline.rs`, against the example site).
> **Path axis**: `.section` is engine vocabulary like `.slots/` (a bare file,
> no config), the scan rides the same `.gitignore` defence as markers, `order:`
> front matter landed on pages, the root's index leads, index-less directories
> appear as unlinked labels (q27's semantic), nested `.section`s resolve
> nearest-wins, and `aria-current` rides the attribute hole; trees derive once
> per section per build — only `current` moves per page. **Heading axis**:
> `toc:` rows carry their outline, extracted *from the rendered block bytes
> themselves* (id and text read out of the shipped `<h2 id=…>`), so link and
> target cannot desync — pinned by a sync test; nesting tolerates level jumps;
> the h2–h3 window is hardcoded v1 policy pending the §5f `outline()` deriver.
> One recursive `outline_entry` kind serves both axes through one theme
> fragment — the unification this section bet on.

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

> The selector is the **second** instance of a mechanism this document now
> names once: a path carries properties, an extractor pulls them out, and
> what remains is the row's identity (q51). `filename_formats` is the
> first. The prefix selector is the one that proves it is a *path*
> mechanism rather than a filename one — it reads a directory component,
> so `fr/recipes/dal.md` and a hypothetical `2026/01/01/hello.md` are the
> same shape. What differs is only whether the extractor *strips* what it
> found: locale must (a row and its translation pair on `by_logical`,
> which needs the locale gone from the identity), dates need not.

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
name  = { en = "Dinner", fr = "Dîner" }
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
  `intro` (the value's own landing prose — §5h mode A per key: the
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

### Honest edges, named now

- **A localized post's trail is complete**: `Accueil(→ /fr/) › Carnet(→
  /fr/blog/) › 10 January 2026`. "Home" is **existence-checked** — it links the
  locale's own homepage when a translated index exists (`index.fr.html` →
  `/fr/`), else the site root. The inert date tail shows the whole date when
  the collection declares no archive chain.
- **Localized tree pages walk URL ancestors**, and the duplicate home crumb on
  `/fr/…` URLs is **cured** (§5h: `ancestors()` skips locale-prefix homes;
  Home is the trail root's job). A section crumb appears in French exactly when
  the section's landing has a French variant. The collection no longer names
  its own index (q46), so the French crumb is *found* by climbing to `/fr/blog/`
  rather than built by prefixing a configured URL.
- **`.slots/` fills localize by the same suffix convention** (`nav.fr.md`
  beside `nav.md`), and their view links resolve per consuming page's locale.
- **Locale-parallel views are built and DEFAULT-ON.** Every materializing
  row-query view — grouped archives, paginated and plain listings,
  members-backed shells like the feed — partitions per declared locale: that
  locale's rows, the locale-prefixed route (default locale unprefixed),
  title/crumb/trail resolved at the route's locale. **A locale with no rows
  materializes nothing**: the partition is real, not mirrored — the example
  gets `/fr/blog/` ("Carnet"), `/fr/atom.xml` (French entries only),
  `/fr/blog/tags/meta/`, `/fr/courses/dinner/` and `/fr/books/`, but no  `/fr/photos/`. Opt-out is `locales = "default"`. Exempt by design: **star
  views** never multiply (they query the finished route set and filter on
  `locale` — one sitemap spans all locales), **object views** carry no locale
  (declaring `locales` there is an error), and **embedded views** follow their
  embedding page (pending).
- **Still locale-free, and known**: `month_name` in group params (computed at
  route build), `pretty_date` ("10 January 2026" on a French page), the search
  overlay's strings (client-side, in `/search.js`, pending search being
  locale-aware), and `site.title` (not yet a `LocalizedStr`). Localized group
  *keys* are q40-adjacent.
- The markers walk uses **physical** paths — irrelevant for the suffix
  selector, a known caveat for the prefix selector, which is built and tested
  but not yet exercised by a corpus.

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
- **`grackle urls --against _site-prod`** — URL-set parity (§4). A **missing**
  URL is a link that used to resolve and now 404s, and exits non-zero; an
  **extra** is usually just content published since the reference was built,
  and is reported only. Derived assets are exempt per q12 — the thumbnail
  scheme moved from `/_thumbs/{md5}-600-600` to `/static/{hash}.{ext}` on
  purpose, and without the waiver a correct build reports 262 missing URLs,
  which is the fastest way to teach someone to ignore the check. The reference
  is any directory of built output, so it works equally against a tree rsynced
  down from the live server — which is what lets it outlive the Jekyll build
  that produced the first one.
- **`grackle diff --against _site-prod`** — golden comparison: normalized
  HTML diff (whitespace/attribute-order-insensitive) per post body, with a
  summary matrix (identical / equivalent / differs / missing). Bodies only —
  chrome was never in that measurement (§5a), and the URL set is `urls`, above.

## 7a. The example site: the falsifier for site-independence *(started 2026-07)*

grackle has been developed against exactly one corpus, and §9b shows the
cost: `"blog"` hardcodes, view-name policy, and a phase-1 gate survive
*because nothing can contradict them*. The design already knows this
argument — a boundary with a single implementation is untestable, which is
why `light` exists (§5a) and why the null theme runs as a falsifier (§5e).
**A second site is the same move one level up**: the falsifier for
site-independence.

`grackle/examples/field-notes/` is that site — self-contained (own
`grackle.toml`, own theme, own `.slots/`, own `_cache/`), invisible to the
main corpus (the `grackle/**` exclude already covers it), built and served
like any site:

```
grackle --config examples/field-notes/grackle.toml serve --port 8081
```

It is deliberately a **kitchen sink**: each section exists to force a
parked feature, in parallel rather than in sequence.

### The second example is a yardstick, not a showcase *(2026-07-20)*

`examples/minimal/` is the opposite site: two posts, one page, and the
smallest config that produces a working blog. It exists to be **measured**.
Every line in it is a line a newcomer must write before anything appears,
so the count is the number to watch — it should fall as defaults land, and
a rise wants a reason.

It was built by starting from `[site]` alone and adding until the build
produced something, which is how both of the following surfaced. Measured
at introduction: **27 non-blank, non-comment lines**, of which roughly half
are identical on any site anyone would build — the three tree rules, the
`!draft && !hidden` set, the post layout default.

Two traps it found immediately, both now closed:

- **A config with no collections built successfully and emitted nothing.**
  No error, no warning, an empty output directory. That is the first config
  anyone writes. It is now a load error naming what is missing and showing
  the shape of a collection.
- **The site published its own `grackle.toml`.** The config is input to the
  build, but it sat in the tree like any other file and routed through the
  passthrough rule. Sites were papering over it with an `exclude` glob —
  which a newcomer has no way to know they need. The engine now excludes
  the config **by identity**, and field-notes' `*.toml` exclude went away
  with it.

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

### The gap clusters, and the questions they opened

The survey's job was to generate questions, not to track them. **§11 owns their
status; this table owns the evidence** — which real sites drove each gap, which
is the one thing that never goes stale. (An earlier version of this section
restated each design and its state, and had gone stale on two of them within
the month: exactly the shadow-copy disease §9b names.)

| gap | driven by | carried in |
|---|---|---|
| **The link graph** — backlinks, then transclusion | andymatuschak, maggieappleton, gwern | q38 (backlinks built as a relations axis; transclusion is the harder half) |
| **Set-scoped computed fields** — aggregation over a view's members, where §5f fields are row-scoped | meal-plan rollups, paulstamatiou's subtree counts, diataxis term indexes | q39 |
| **Structured record fields** — a list-of-records type, plus schema.org emission | ingredient lists, podcast chapters, cast lists | q40 |
| **i18n** — a translation axis on rows | docs.astro, solar.lowtechmagazine (12 languages) | **§6f — built.** The one classic SSG feature the model lacked outright |
| **Client-side faceted filtering** — combinable facets can't be enumerated as static views | recipe sites, digital gardens (diet × cuisine × season) | q42 |
| **Media beyond image** — audio/video field types, RSS enclosures, srcset renditions, externally-hosted originals | sive.rs's 250 interviews, two podcast sites, fasterthanli.me, macwright's CDN | q43 |
| **Per-row scoped assets** — §5b's unbuilt `.style.scss` leg plus its script sibling | ciechanowski's per-article JS/CSS pairs | §5b |

Two of these resolve without becoming questions. The *interactive-widget* half
of ciechanowski (stateful WebGL islands as the site's identity) stays honestly
out: raw HTML passthrough plus per-row assets carries the delivery, and the
engine never models the widget. And **external/live data** — trending ranks, HN
counts, live solar charge — is not expressible from a git tree; the honest
answer is an ETL that *writes* git-tracked data before the build, after which
`order_by` works on it normally. Kottke's "vintage post today" is the benign
case: a date-seeded deterministic pick is fine for a daily build. The model's
answer is "commit the data".

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

## 7c. The inspector: the database explaining itself *(built 2026-07-19)*

`grackle serve` reserves `/__debug/` and answers it from the binary: an
`index.html`, `debug.css`, `debug.js` and a `site.json`. Serve-only by
construction — a build emits none of it, and the prefix is a **closed
namespace**, so a miss inside it 404s rather than falling through to a
site page that would otherwise shadow the tool.

**The payload is deliberately not `grackle export`.** The export is the
database as the database sees it; this is the database as someone
diagnosing it needs to see it, and the two differ in exactly two ways.
It carries what the export skips — route `members` and the row flags are
`#[serde(skip)]` there, and they are precisely what answers *what picks
this up* and *why is this missing*. And it resolves members to **URLs
rather than indices**: an index only means something beside the table it
indexes, so emitting URLs lets the client join everything to everything
without knowing which table a view ranges over. The payload rides in the
serve snapshot, rebuilt with the site, so it can never describe a
database the served pages didn't come from.

**Four lenses, and the cardinality picks the form.** Measured on the main
corpus: 838 of 1575 routes are objects, posts are 1:1 with theirs, and
**7 views produce 183 routes**. So trees and tables for the big
homogeneous sets, and no node-graph anywhere — 1575 routes as a
force-directed hairball would teach nothing, and 3 tables whose
relationships are all derived make a poor ER diagram.

- **tree** — the same corpus in its two shapes, source and URL, side by
  side. The difference between them *is* the route template: `_posts/`
  is flat with 327 files, `/blog/2022/12/20/…` is four levels deep.
- **rows** — a table per table, typed columns, flags visible.
- **views** — every declared query and its fan-out.
- **diagnose** — anomaly first, inventory second. The top question is not
  "show me my rows" but *why isn't this page showing up*, and every
  answer to that is an exception: no route, claimed, draft, hidden,
  noindex, no title, an undated post, a view route with no members.

  The bar for a finding: **it must be able to be wrong.** An undated
  *draft* is not a finding — undated is what a draft is, and four
  permanently-correct entries teach you to skim the list. An undated
  *publishable* post is, because the cost is silent and threefold: no
  year or month archive membership, a trail that stops at the collection,
  and last place in every ordering.

Star views (`over = "*"`) carry no `members` — they range over routes and
the render passes re-evaluate their filter — so the payload evaluates it
the same way rather than showing an empty list and implying the search
index is empty. `/search.bin` reports 327 members and `/sitemap.xml` 589,
the latter matching the emitted sitemap exactly, which is the check that
the evaluation reproduces the pass rather than approximating it.

The centrepiece is the **provenance strip**: source → route → the views
that picked it up. A generic database viewer structurally cannot show
it, because here the row and the URL are not the same object — a claimed
row has no route (§5h), a translated row has two (§6f), and a view route
has 66 members and no row at all.

Between the two trees is a **gutter** that draws the current selection's
correspondence: an arrowhead into each side and a line joining them, one per
pair. Two states make it useful rather than decorative — a target scrolled out
of its pane turns its head **up or down** (the arrow stops meaning "over there"
and starts meaning "scroll"), and a target inside a *collapsed* branch has no
element at all, so the connector points at the nearest rendered ancestor and
goes dashed: it names the folder to open instead of pointing at nothing.

Two things it taught immediately. **A node can be both a route and a parent** —
`/blog/` is `blog_index`'s own route *and* the ancestor of every archive
beneath it, and the first cut conflated "has children" with "is a folder",
which made every landing impossible to select. The twisty owns expansion, the
label owns selection. And **route order is lexical** (`sort_by(url)`, for
determinism), which is right for the sitemap and wrong for reading:
`/blog/page/10/` sorts before `/blog/page/2/`. The client owns display order
with a numeric-aware comparator; the engine keeps its determinism.

## 8. Known-inexact from day one (accepted, iterate later)

| Area | Why | Plan |
|---|---|---|
| Code highlighting spans | Rouge ≠ syntect token boundaries/classes | 🟡 **half done.** Wrapper divs + inline-code classes emitted via the AST pass (§9a) — rouge-cause diffs 45 → 1. Still missing: Rouge's pygments token spans (`<span class="c1">`) for the ~12% of blocks with a real language. Under-measured: 4 of 6 highlighted posts are liquid-skipped (§8a) |
| kramdown edge syntax | IALs `{:.x}`, `markdown="1"`, footnote markup | comrak `smart` + extensions first; triage real diffs per-post; hand-normalize stubborn 20-year-old posts. **`markdown="1"` found in the wild** (2 posts, the callout boxes): comrak drops the `<div>` into a `<p>` and the box collapses; one post hand-normalised, the other left raw as the widget's test fixture, because `{% callout %}` widgets (§5d, open q29) are the real fix — they retire the raw-HTML idiom entirely |
| Related posts | LSI is unreproducible *and* unwanted | **Superseded** (§6b): embeddings replace it outright. Deliberately not equivalent — this is an improvement, and `diff` can't judge relatedness anyway |
| Feed body HTML | `feed_images`/`expand_urls` operate on rendered HTML | ✅ **done** (regex port, §render). `expand_urls` makes root-relative `href`/`src` absolute; `feed_images` injects `align`/`width` on float images — both byte-verified against the reference. `<content>` bodies still carry the markdown gap (§8a), feed-only, low stakes |

### Heading anchors: kept, deliberately *(2026-07-21)*

comrak injects an `<a class="anchor">` inside every heading; kramdown does
not. **226 of them across 44 posts, and we keep them** — each carries an
`aria-label`, which is a heading affordance the Jekyll site never had and we
want. Recorded because it is a real, permanent divergence from the reference,
and because of how it was found: `markdown.rs` claimed for a month that "the
real pipeline strips it in the AST pass". Nothing ever did. Only
`diff::normalize` strips them, so the body oracle had been measuring parity
with the difference removed. §8a's rule caught this one late — *agreement is
not evidence unless it can disagree* — and it is the third item in the case
for retiring that oracle.

## 8a. The markdown gap, and what measuring it taught

The kramdown→comrak gap was the one risk that could sink the port. It is a
number: **90.0% usable**, 92.2% if smartypants is matched, and the residue is
**parser-side**.

**Method.** Posts that are both liquid-free *and* untouched since the reference
build (a naive comparison would have measured content drift and blamed
comrak). comrak configured to kramdown's defaults: `auto_ids`, smartypants,
tables, strikethrough, footnotes, description lists, raw HTML passthrough.
Normalisation folds only invisible differences: whitespace, entity spellings,
self-closing style. Of 230 posts: 20 identical, 187 equivalent, 23 differ.

The residue is `10 inline/prose · 5 list · 4 link · 3 table · 1 code block`,
and spot-checking says every one is **parse**-stage:

- `Windows ‘95` vs `’95` — kramdown renders a decade abbreviation with an
  *opening* quote, comrak with an apostrophe. comrak is typographically right
  (`'95` is an elision) but kramdown is the target, and a corpus that opens in
  1998 says "Windows ‘95" a lot. Fixable in an AST/text pass.
- `<li>text</li>` vs `<li><p>text</p></li>` — kramdown decides looseness **per
  item**, CommonMark per **list**. A dialect difference.
- Raw HTML in prose: a literal `<solution>` written as text is auto-closed by
  kramdown, left open by comrak.

**Zero heading, zero footnote, zero image diffs** — the four node types we have
opinions about are not where we lose. The 90/92% ceiling is a *parser*
ceiling, which is what decides the renderer question (§9a): if we ever chase
it, we fork comrak's parser, not its formatter.

### The reference build lied by 17 points

The single most important measurement lesson of the project, and it very nearly
went unnoticed. The original headline was **90.7%**, measured against a
`_site-prod` built five days before the config turned Rouge on:

```
6437c22  2026-06-05  Code formatting
-  syntax_highlighter: nil
+  syntax_highlighter: rouge
```

The reference had highlighting **switched off**. It emitted bare
`<pre><code>` — exactly what our comrak emitted. We were not close; we were two
builds agreeing because both had Rouge disabled. Rebuilt against the *current*
config, "usable" fell to **72.6%** with 45 rouge-cause diffs; implementing the
Rouge shapes took it back to 90.0% with 1. Our output never changed. Only the
yardstick did — and the final 90.0% landing on the original 90.0% is a
coincidence: the first was luck, the second is earned.

**The rules this buys:**

1. **A reference build is an input, and inputs have versions.** Rebuild it from
   the *current* config before quoting any number derived from it, or the
   number is about a site that no longer exists.
2. **Agreement is not evidence unless it can disagree.** Both this and the
   later `latest` check (§5c) matched for reasons unrelated to correctness. A
   test that cannot fail is not measuring.
3. **Read deltas, not tallies.** `classify_cause` is a ±window keyword
   heuristic and over-attributes: `identd` was filed under "link" when the
   actual delta was `‘95` vs `’95`, because a link happened to be nearby.
4. **A 100%-fail result is a harness bug until proven otherwise.** The first
   run reported a meaningless 100% differ, twice over: `extract_body` took the
   *last* `</section>` and swallowed the whole page — and its unit test passed
   only because the test pre-sliced its input, *a test that hid the bug it was
   meant to catch*. The other half counted the layout's `<a class="fullpost">`
   as a markdown difference, which is a category error.

### Retiring the body oracle *(Matt's call, 2026-07-21)*

The body diff is **no longer a cutover gate**. `grackle urls` gates the URL
set; everything else is verified by eye. Three things drove it, and they
compound:

1. **The reference is a wasting asset.** 48 of 327 posts have been edited since
   it was built, and the edits are deliberate migration work — `{% post_url %}`
   rewritten file-relative, raw URLs converted for strict links, callouts
   rewritten as widgets. §8a's method filters to posts "untouched since the
   reference build", so the comparable set shrinks every time the corpus moves
   toward grackle. Two posts now carry `{% callout %}`, which Jekyll cannot
   render at all, so the reference cannot be fully regenerated even with the
   `git stash` dance (q22).
2. **The harness hides real differences.** `diff::normalize` calls
   `strip_comrak_anchors`, so the 90% figure is computed with comrak's 226
   injected heading anchors removed. They ship; the reference has none; the
   measurement structurally cannot see them. That normalizer was right for the
   question it was written to answer (do the slug algorithms agree?) and wrong
   as a parity gate.
3. **The remaining gap is a parser ceiling**, ~92%, and §9a already decided we
   will not fork comrak's parser to chase it.

What survives: `diff` stays as an *investigative* tool, and the 97-post blind
spot below stays worth knowing when reading any number it prints. What ends is
treating its matrix as the thing that says "safe to publish".

### The 97-post blind spot *(open — q21)*Related and still true: **`_site-prod` can no longer be regenerated** (§5c) —
`{% view %}` is not Liquid, so Jekyll fails the whole build and refreshing the
reference needs `git stash push index.html` first (q22). Losing the ability to
refresh the reference is exactly the capability that caught the 17-point lie.

### Two SCSS findings worth keeping

- **`grass` rejects a nested `@import` that libsass accepts.**
  `_sass/_post.scss` has `pre > code { @import "rouge"; }`; grass errors with
  "this at-rule is not allowed here". The site is legal input that grass will
  not take. Fixed by resolving `@import` textually before handing grass the
  flattened source, so the site's sass is untouched. grass's
  "dart-sass-compatible" reputation needs this caveat.
- **grass and sassc agree**: 2232 selectors against the live build's 2231 — a
  one-rule formatting difference, not a semantic one.

## 9. Crate layout *(as built; the original sketch is in git history)*

A cargo workspace of six members under `crates/`. The split is one
dependency direction, and Cargo is what enforces it: **`grackle-db` depends
on nothing in the workspace.**

```
grackle/
  Cargo.toml                 virtual manifest + [workspace.dependencies]
  grackle.toml               the site (root = "..")
  crates/
    db/                      THE QUERY ENGINE — domain-free by construction
      filter.rs    the typed predicate language + functions (§5)
      table.rs     Table<R>: rows of one type, keyed, queried
      view.rs      View = filter + order + limit; Table resolves one
      index.rs     the two index shapes (unique, multi)
      key.rs       Key: a row's identity, stable across loads
      template.rs  {name} substitution with zero-padding (§4)
    model/                   THE DATA MODEL — everything grack.com-specific
      lib.rs       Row, Object-as-Row, Route, SiteDb, the three schemas
    source/                  THE LOADER — config + filesystem -> database
      config.rs    grackle.toml; the ONE over-chain walker
      store.rs     FsStore: front-matter split, tree walk, .gitignore (§4c)
      markers.rs   marker scan + nearest-wins defaults (§4b)
      schema.rs    .schema.toml per-subtree field declarations (§5b)
      filename.rs  filename formats: stem -> (date, slug)
      views.rs     views become routes: row sets, grouping, subdivision
      load.rs      one walk of the site, and the rows it produces
    grackle/                 THE BINARY — render, serve, report
      main.rs      CLI (query / export / build / serve / routes / diff)
      build.rs     render_site: the passes; build = write map to disk (§7)
      parts.rs     part maps: typed schemas, producers, canonical() (§5e)
      binder.rs    fragment parser + hole algebra + load-time checks (§5e)
      slots.rs · theme.rs · render.rs · markdown.rs · tags.rs
      outline.rs · trails.rs · links.rs · embed.rs · thumbs.rs
      serve.rs · debug.rs · diff.rs
      assets/      include_bytes! payloads (debug UI, search.js/wasm)
    search-core/             stem/tokenize/index/rank — build AND browser
    search-wasm/             the same core behind a raw no-bindgen wasm ABI
```

`model -> db`. `source -> model, db`. `grackle -> all`. Nothing points back.

### Why `db` is a crate and not a module

Because the boundary is worth paying for. The engine's query half kept
absorbing domain: `config` validation called `db::row_schema()`, the row
schema sat beside the loader that filled it, and "what a filter can name"
drifted from "what a row answers" twice (§q51). Splitting it means the
compiler refuses the shortcut — `grackle-db` cannot name a `Row`, so a
filter feature cannot quietly become a blog feature.

What it holds is a mini database, not a bag of helpers:

- **A row is whatever answers `filter::Row`** — a name in, a typed value
  out. That one contract is why `Table` serves posts, objects and routes
  without knowing what any of them are.
- **A key, not a position.** Every index, membership list and query result
  names rows by `Key` (an `Arc<str>`, compared by value). Positions are only
  meaningful inside one load — sort the table and every one is wrong, which
  cost two real bugs before this landed. A key survives a rebuild, which is
  what `serve`'s incremental story will need.
- **Functions are the filter language's extension point.** `under(path, x)`
  and `glob(path, x)` are entries in a table; a field, a literal and a call
  are one thing (an operand), so a call goes anywhere a field does and is
  type-checked by the same pass. `glob` compiles its pattern once, at parse
  time.
- **A view is a value**: filter + order + limit, resolved by `Table`.

### The ordering rule *(2026-07)*

`path` ascending, unless the view names a column; `path` is the last
tiebreak either way. The engine used to assume every corpus was a blog and
sort newest-first — a tree is a list of files, paths order, and that is the
contract. A collection whose rows carry dates says `order_by = "-date"`.
Adjacency is the exception and says so: `neighbors_in` reads *position in a
sequence*, so "later post" is the entry before, and its default stays
newest-first.

~17.5k lines across the workspace, ~216 tests. The sketch this replaced
imagined `store/watch/snapshot` and `db/{posts,tree,views}` submodule trees, a
`render/liquid.rs`, and axum+SSE serving; reality is six crates, liquid never
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
- **`lol_html`, taken narrowly (2026-07-21).** Not for the rule table §6d
  sketches — for the one job an AST pass structurally cannot do: resolving
  links in rows whose source *is* HTML. `expand_urls`/`feed_images` stay small
  regexes, and the selector language waits for a second consumer.
- **`salsa` declined** — hand-rolled typed invalidation keys suffice at 327
  posts (open question 1).

The bar for a new dependency: taken for a measured reason, and recorded
here only when the decision itself is interesting.

### Why we do **not** write our own AST → HTML renderer

The tempting conclusion from §6d (footnotes) and §8 (Rouge shapes) is to own the
formatter. The measurements say no.

**The fidelity argument fails.** §8a: the residual gap is parse-stage —
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
field flowing through `from` (no rendering attribute, no engine policy).
The predictor of this health is the discipline itself: **everything
declared is load-checked** (filters, fragments, fields, widgets, slots), so
a responsibility placed in config *stays* there — code can't quietly
reinterpret what it would first have to type-check.

### The one recurring disease

§5c named it: *the config declared `where`/`group_by`/`paginate` and the
renderer ignored all of it*. That was cured for row membership (`members`), and
three more pockets have since closed — producers hardcoding routes config owns
(q32), the feed pass selecting its view by `template == "atom.xml"` (cured by
shells), and the sitemap predicate being evaluated three separate times (star
routes carry `route_members` now, resolved once, *after* the route sort — which
is where the real bug was: `sitemap` counted against a route list that did not
yet contain its own route).

Two pockets remain, each the renderer re-deriving something config owns:

1. **`build.rs` holds policy keyed on a view name** (→ q33). One spot: `view !=
   "blog_index"` decides which listings get `noindex`. It wants to be a view
   attribute — a schema fact like every other.
2. **Three definitions of "not content"** (→ q34). §4c legislated the three
   layers for the tree walk, but `slots.rs` carries a private `SKIP` list
   duplicating half of `grackle.toml`'s `exclude`, and `serve.rs::is_content`
   carries a third. Add an exclude to config today and the watcher still
   rebuilds on it and the slots walk still descends it.

Neither is urgent — both are invisible until a config value changes out from
under its shadow copy. But that is also the §5c lesson: the drift is only ever
invisible *until* it isn't.

### Accepted asymmetries, named so they don't read as leaks

- The CLI's `query search` indexes raw markdown where build indexes rendered
  HTML — documented at `search_docs`; a deliberately cheap smoke query, not an
  inconsistency to fix.
- `render.rs` has become "head facts + escaping + XML serializations" — its doc
  admits it. If stage B touches the feed anyway, the serializations can move
  out; renaming for its own sake isn't worth a commit.
- `post_trail` is still single-posts-table; a second posts collection remains
  future work.
- `default` survives as the conventional theme name, and search assets live in
  the default theme.

### Round 2 *(2026-07-18, after landings, records, links and i18n)*

Taken at Matt's ask after the biggest feature month yet. Verdict: **the
boundaries held under load** — landings, claiming, enum records, the
link language and locale-parallel views each landed in exactly one
owner, and the example config *shrank* while gaining features (two
filters, a hand-list and every raw course URL deleted). The disease
inventory:

5. **The landing pass re-shapes rows the bare passes already shape.**
   posts→summaries (with truncation), tree→cards, objects→figures each
   exist twice in `build.rs` — once in the bare listing/gallery/card
   passes, once in §5h's embed construction. "The route's slice as
   parts" wants to be one function with two callers; until it is, a
   truncation change can silently disagree with the landing embed. Same
   pattern smaller: `route_value` (the slug seam) is verbatim twice in
   `views.rs`.
6. **`build.rs` is the gravity well** (~1,800 lines): rendering passes
   plus the whole trail family (`trail_root`, `ancestors`,
   `listing_title_and_trail`, `post_trail`, `home_url`) plus the
   intro/prose family. ✅ **The trail family left** — `trails.rs`, lifted
   verbatim and proved byte-identical before q46 touched it, which is
   what let the dissolution's diff be all semantics. The intro/prose
   family and the rendering passes are still there; item 5's
   "route's slice as parts" is the next tenant to evict.
7. **Semantic drift in the main config**, folded into q33's remainder:
   `layout` on listing views is a presence flag wearing dead kind names
   (`"tag_index"` selects nothing since §5e), and `template` no longer
   templates — it *claims* a legacy file, which is §5h's claiming
   vocabulary wearing an old name.

Born from this audit: **q46** (dissolve `collection.crumb`/`index` into
the landing chain — the last non-derived trail names), ✅ **settled and
built**; §5h carries it. It paid an unbudgeted dividend: the duplicated
config had been propping up a broken climb (paginated landings were
invisible to `ancestors`), which no test could have caught while both
sources existed. The example's
residuals are all scheduled, not leaked: the manual's hand-list waits on
§6e-as-landing-listing, home waits on the queryless landing + q37's
board, andthe search view's one `stem != "index"` dies when home and manual lift.

### Round 3 *(2026-07-21, after the crate split)*

The engine became a workspace (§9), and the split was itself the audit: a
boundary you have to declare to Cargo is one you cannot half-hold. Four
durable lessons, none of which a re-read had produced:

1. **An awkward dependency is usually a mislabelled layer.** `config`
   validation calling into `db` looked like a cycle to break; once `db` sat
   *below* config rather than beside it, the call became legal and the
   "prerequisite refactor" evaporated.
2. **`pub` in a library crate silences dead-code detection.** Anything
   genuinely internal to a crate should stay private or it rots invisibly —
   which is why `index`'s primitives are private and only `Table` reaches them.
3. **A declared-and-ignored key is invisible until you delete it.** grack.com
   named three layouts that matched no fragment; the theme fell back to
   canonical rendering and the render pass discarded the name anyway. Two views
   both declaring `layout = "listing"` rendered structurally different HTML.
   Swapping all three for `listing` changed not one byte, which is how they
   were found. Cured: the passes dispatch on `layout` against a closed
   vocabulary, and an unknown one is a load error.
4. **A test fixture is not identity.** Six fixture helpers had never set `rel`,
   so a table holding four fixture rows held *one*. A keyed store makes
   identity something fixtures have to MEAN, not something they inherit from
   being in a `Vec`.

### Since, and what is left *(2026-07-21)*

Three merges landed, each removing a distinction that was never real:

- **The two row flows are one.** `build_views`' posts flow and
  `build_tree_view` are `build_row_view`. What made it possible was saying
  eligibility as a predicate — the tree flow filtered `rendered && !claimed`
  and the posts flow filtered neither, and both are no-ops on the posts side —
  so they now describe the eligible SET rather than which table it came from.
  `limit` landed in one place with it, and pagination started working for tree
  views, which had bailed `"not supported yet"` on no stronger grounds than
  never having been written.
- **The base table is a filter.** `post_ix` vs `page_ix` was the last place a
  view's table chose its code path; it is now `collection == "posts" || …`
  built from config and ANDed onto the view's own filter. That is what
  `published` needed — a set could not span tables while the base was an index
  list.
- **The last positional assumption is gone.** `RouteKind::Post` was decided by
  `i < n_posts`; it is set membership now. Keys retired all three such
  assumptions.

`Kind` branches: 26 → 20, and none is a flow. What is left: the objects
dispatch, the loader choosing which collection reads which way, config
validation, and presentation policy.

### Still owed

- **The objects dispatch.** `build_object_view` stays separate, and the
  reason is worth recording so it is not "merged" thoughtlessly: it is a
  different table, a narrower schema *by design* (§5b — `where = "draft"` on
  a gallery should be a load error), and object rows are `rendered: false`,
  which the row flow's eligibility predicate excludes. Folding it in would
  mean passing table, schema AND eligibility as parameters — at which point
  the parameters are the two functions. What was stale has been deleted;
  `group_by`/`paginate` still bail there because that function does not
  implement them.
- **The single tree** (§3's endgame: one table, views as partitions) has not
  started. Measured obstacles: `store.rs` skips `.`/`_` names, so `_posts` is
  invisible to the tree walk by convention rather than config; six tracked
  underscore directories would need explicit excludes; and
  `filename_formats` is per-collection where it would have to be per-rule.

## 10. Phasing (each phase has a checkable exit)

Phases 0–4, 6 and 8 are **done**; 7 is at stage A; 5 is the open one.

| Phase | Deliverable | State |
|---|---|---|
| 0 | FsStore + posts table + `query` | ✅ 327 rows, URL set matches the Jekyll sitemap exactly; loads in ~3.5ms warm against a 200ms budget |
| 1 | route mapping, `export`, `routes` | ✅ ~1579 routes across posts/pages/objects/views; **every one of the 556 Jekyll sitemap URLs is routed**, 0 missing (the extras are assets jekyll-sitemap never lists) |
| 2a | markdown-gap spike + `diff` | ✅ **the port is viable** — 90.0% against an honest reference, 92.2% if smartypants is matched; the residue is parser-side (§8a) |
| 2b | render pipeline end to end | ✅ 327 posts + listings with pagination + 40/40 pages + 1025 assets + 260 thumbnails + feed + sitemap in **~0.4s warm** (Jekyll: ~38s). Zero skipped pages |
| 3 | feed, sitemap, scss, thumbnails, passthrough | ✅ entry sets byte-identical to the reference; 25.3 MB of sources → 9.0 MB shipped. `linklint` retired 2026-07-21 — strict links validate at build time and fail the build, in markdown and raw HTML alike, which is what a post-hoc crawl was for || 4 | `serve`: resident db + live reload | 🟡 **v1** — raw hyper, resident render map, no output dir; a watcher rebuilds the world in ~0.3s and a polled script reloads the browser. Deferred: §2's incremental invalidation, SSE |
| **5** | **exactness iteration** | **exit criterion changed, 2026-07-21** (Matt): **URL parity by machine, the rest by eye.** `grackle urls` gates the URL set — the half that protects 20 years of inbound links — and the body diff stops being a gate. See "Retiring the body oracle" below || 6 | §5e presentation synthesis | ✅ complete — part maps, binder, real theme directory, canonical fallback, completeness falsifier on every `cargo test` |
| 7 | §6d blocks | 🟡 **stage A** — one parse, summary as a computed field, `data-truncated`. Stage B: notes stream + sidenotes (q18) and the rewrite stage |
| 8 | §6b embeddings + search | ✅ LSI and Swiftype both retired — the Jekyll build's last two external services |

## 11. Open questions (to iterate on)

Only OPEN questions live here; a settled question moves its design into
the section that carries it and leaves one line in the ledger below, so
`qNN` references elsewhere in this document always resolve. Numbers are
never reused.

1. **Dependency tracking**: hand-rolled typed invalidation keys (as specced)
   vs `salsa` for automatic fine-grained tracking. Leaning hand-rolled —
   at this scale precision bugs are cheaper than framework complexity.
   Rides with serve v2 (the phase-4 deferral).
2. **Row version**: content hash (correct, rehash on every event) vs
   mtime+size (fast, near-correct) vs mtime-then-hash pre-check (specced).
4. **Highlighting fidelity** — *half-settled*: the wrapper/inline-code
   shape is done and exact (§9a); only the token spans remain (coarse
   Rouge-class mapping vs syntect classes + regenerated CSS). §8a warns
   the gap is under-measured: 4 of 6 highlighted posts are liquid-skipped,
   so "1 diff" is 1 of 2 compared.
6. **Drafts**: replicate `_drafts` preview in `serve` from day one, or
   post-phase-3.
11. **Iframe policy**: §6a resolves and rewrites `<iframe src>` for bare
    names but doesn't thumbnail. Do iframes need any sandbox/loading
    attributes injected by the same pass, or is passthrough correct?
13. **Embedding model pinning.** `all-MiniLM-L6-v2` output is
    model-version dependent, so the cache key includes a model identifier
    (`_cache/embed/{model}/`) — but should a model upgrade silently
    re-embed all 327 posts on next build, or require an explicit
    `grackle reindex`? Silent is friendlier; explicit is more predictable.
14. **`<style>` auto-scoping default (§6c).** Scoping fixes a real latent
    leak on `body.multipost` index pages, but it's a behavior change on
    the 3 existing posts. Default-on with `style_scope: false` opt-out
    (specced), or default-off and opt in per post?
21. **Tighten `diff`'s liquid skip (§8a).** 97 of 327 posts are excluded,
    many falsely (`{{ github.event.issue.number }}` in code samples is
    GitHub Actions, not Liquid). 30% of the corpus is unmeasured and the
    90% is over an unrepresentative 230.
22. **`_site-prod` can no longer be regenerated (§5c, §8a).** `{% view %}`
    is not Liquid, so Jekyll fails the whole build; refreshing needs
    `git stash push index.html` first. Losing the ability to refresh the
    reference is exactly the capability that caught the 17-point lie.
    Script it, or move the reference build behind a flag that stashes
    automatically.
23. **The `hero` part — the remainder.** Built via the book club (§5e:
    image-typed `cover` schema field beats `image`, thumbnailed, dimension
    facts). Still arriving with their consumers: the first-image-block
    fallback, and the mindstorms *group* hero (a `cover.*` file beside the
    group — explicit beats derived, expressed positionally).
25. **Per-block facts (§5e).** A block-level directive (kramdown IAL
    `{:.full-bleed}` or similar) surviving as a `data-` attribute on the
    block, so a theme can span it. Needs a decided authoring syntax — IALs
    are kramdown, not CommonMark, so this interacts with the §8 dialect
    gap.
26. **Dimension facts — the remainder.** Parts-side images (figures,
    heroes, cards) carry `width`/`height` from the thumb pass (§5e). Post
    *bodies* still don't: `{% image %}` output gains dimensions when the
    §6d rewrite stage exists (its seam), killing layout shift site-wide.
28. **Mindstorms restructure vs URL parity (§5 audit).** The gallery
    restructure retires `/demos/mindstorms/alpharex_1.html` and its 16
    siblings — in the sitemap, carrying **no `noindex`** (indexable today
    by accident). Needs `.htaccess` redirects or an explicit parity
    exemption like q12's — and the accidental indexability is worth fixing
    *before* the restructure, not with it.
30. **Pagination × subdivision (§5c).** A grouped view can be subdivided; a
    paginated one deliberately cannot *yet*. A year archive could
    plausibly paginate (`/blog/2022/page/2/`) while months subdivide off
    the same root — the row-set semantics are coherent, but parent
    pagination URLs and child routes then share a namespace. Two grades:
    **actual collision** (two routes, same URL — checkable as a hard error
    today, the database advantage) and **pattern-space overlap** (shapes
    intersect, today's keys don't — should warn, silenced by an explicit
    `allow_overlap = true`: warn-or-declare, the §4a posture). Also parked
    here: crumb templates for paginated views (the `Page N` trail entry is
    an engine rule for now). The config check errors with a pointer to
    this question.
33. **View-name policy in `build.rs` (§9b)** — the serialization half
    settled as shells (§5g); what remains is exactly what the 2026-07-18
    audit re-flagged: (a) listing `noindex` is decided by
    `view != "blog_index"` — a site policy living in engine code as a
    string match; views should declare `noindex = true` (the tag/archive
    views would, matching Jekyll). (b) The `"blog_index"` layout-presence
    fallback dies when the view declares a layout. (c) `layout` on the
    main site's listing views is a presence flag wearing dead names
    (`"tag_index"`, `"monthly_archive"` select nothing since §5e) —
    rename to `listing` for truth, byte-identical. (d) `template` no
    longer templates — it *claims* a legacy file from the tree, which is
    §5h's claiming vocabulary wearing an old name. (e) The sitemap
    filter's second evaluation (star routes carry no members).

    (f) **Row `layout:` is the same disease on the row side — a Jekyll
    word that survived as a flag** *(measured 2026-07-19)*.
    `Some("page") | Some("post")` is a single match arm, so those two words
    are one value; the `_layouts/*.html` they name have been unread since
    §5e; the field sits in the post and page filter schemas and nothing
    filters on it. Census of the four tiers a row can land in — main site's
    227 page rows / example's 21:

    | tier | selected by | main | example |
    |---|---|---|---|
    | verbatim bytes | front-matter absence | 187 | 1 |
    | `light` tier | `layout: light` | 2 | 0 |
    | chrome, no furniture | `default`/absent | 1 | 2 |
    | chrome + furniture | `page`/`post` | 37 | 18 |

    So **55 files declare the common case in order that 3 may declare an
    exception**, and omitting the field silently drops a row's furniture
    (probe row: 0 crumb/relation/neighbour elements against a sibling's 3, no
    error). The `default` tier's three occupants are all homepages, which §5h
    landings absorb.

    What dissolves is the *spelling*, not the distinction: the tiers are shell
    levels, so they belong under the row `shell:` vocabulary
    (`none`/`light`/`html`, §5g) rather than under a layout name. `light` is a
    real tier with two occupants, **not** the null theme — §5g's "Row tiers"
    carries the measurement that separates them.

34. **Three "not content" lists (§9b).** §4c's three layers govern the
    tree walk only; `slots.rs` (`SKIP`) and `serve.rs` (`is_content`)
    carry private skip lists that can silently drift from `exclude`. Both
    walks should derive from the §4c layers. Serve's one extra legitimate
    member — `_cache/`, which a rebuild *writes* — stays its own.
37. **The `board` kind: composition of views as content (§5c-adjacent,
    specced, deliberately pending).** A board is a *query over queries* —
    `[routes.home] layout = "board"` declaring ordered members, each
    contributing `{label, content}`; the theme places one slot, CSS does
    the columns. It would retire the last hand-written arrangement on
    either site's homepage, and §5h gives it its frame: home is the
    queryless landing, and the board is what its *listing* becomes.
    Pending on purpose (build at the second board or the publish cutover):
    (a) member declaration — names vs inline definitions; (b) labels —
    per-member vs inherited from each view's `title`; (c) routable or
    embed-only; (d) boards-in-boards (leaning no — the §5d tripwire);
    (e) whether board items ride the q36 preview kind or stay opaque.
38. **Transclusion (§7b).** Render row X inline by reference. The
    backlinks half of the link graph is built (page-bodies prepass, href
    scan, `linked-from` relations axis — zero fragment changes to render
    it); transclusion waits on a real consumer, with §5d's
    no-control-flow rule watching it.
39. **Set-scoped computed fields (§7b).** §5f fields derive from ONE row;
    the survey wants aggregates over a view's members — `count()`,
    `sum(minutes)`, date spans — for meal-plan rollups, subtree stats,
    calendar counts. A natural §5f extension (functions whose source type
    is a member set), but it changes the field-inheritance story; spec
    alongside q31's build.
40. **Structured record fields (§7b).** `.schema.toml` wants a
    list-of-records type (`ingredients = {type="records", fields={qty=
    "string", name="string"}}`-ish) for ingredient lists, podcast
    chapters, cast lists — plus a schema.org/JSON-LD emission deriver.
    Extends §5b without changing its shape. §6f's enum records
    (`[records.<field>.<id>]`) took the *value-domain* half; this is the
    *row-field* half.
42. **Client-side faceted filtering (§7b).** Combinable facets (diet ×
    cuisine × season) can't be enumerated as static views. The search.bin
    architecture generalizes: ship a typed facet index, run the
    intersection in the client — a *client-side view*, declared in config
    like any other, materializing an index instead of routes.
43. **Media beyond image (§7b).** Audio/video schema field types (with
    duration/player facts, as image carries dimensions), podcast RSS
    enclosures as a feed variant, multi-format/srcset renditions from the
    §6b contest, and externally-hosted originals (a URL-valued image
    source that skips thumbnailing but can still carry declared
    dimensions).
47. **Listing views render no language switcher (§6f).** The
    `translations` axis is a ROW relation, so a row and a mode-B landing
    (whose claimed row carries it) get the switcher; a plain listing view
    does not. Measured: `/fr/blog/` and `/fr/books/` emit zero
    `data-axis="translations"` blocks, so a French reader who arrives at
    a listing has no way back. The parallel routes to link already exist
    — locale-parallel views are default-on, and §5h computes a landing's
    switcher from its owner's materialized routes — so this is that
    computation applied one level out, plus the question of whether an
    axis belongs to a route at all when nothing about it is a row.
48. **`type:` as row data, not presentation** *(Matt's shape)*. A row
    should declare *what it is*, not how to draw it — `type: recipe`,
    with config mapping the type onto presentation, which is §4b's rule
    (the config says what a marker means; the tree says where). It would
    be rule-defaulted like `theme:`, per-row only for exceptions, so most
    files carry nothing. **The test it must pass: a type is real only if
    something other than the renderer consumes it** — a cross-tree filter
    (`type == "recipe"` instead of the `match = "recipes/**"` glob),
    q40's JSON-LD emission (schema.org maps onto it directly), or
    non-positional schema selection. Held deliberately: neither site
    needs one today, subtree position already implies type wherever a
    `.schema.toml` sits, and a type that only picks a theme is q33(f)
    with a better name. q40's build is the moment to decide.
49. **Where a row's metadata comes from when the file can't carry front
    matter** *(measured 2026-07-19)*. Two halves, in precedence order.
    **Derive first**: the artifact usually already states this, and
    reading it costs nothing and never drifts — 14 of 57 raw HTML files
    carry a real `<title>` ("Colossal Cave Adventure", "Online
    Psychologist") that the database currently ignores, leaving 39
    user-facing passthrough rows titleless; and all 838 object rows could
    carry bounds and format from their own headers, which is a cleaner
    source than today's thumb pass and is what q26 wants for
    `{% image %}` in bodies. Measured and rejected as speculative for
    *this* corpus: EXIF (0 of 200 jpegs — stripped long ago) and PDF
    metadata (3 files). **Then declare**: a per-file sidecar,
    `.p01.png.toml` — leading dot so §4c's dotfile layer already excludes
    it and the §6f stem parser never sees it, full name so it stays
    unambiguous under §6a's deliberately non-unique object names. It is
    the file-scoped member of a family that already exists at directory
    scope (`[markers]`, `.schema.toml`, `.section`, q23's `cover.*`), it
    is the *fallback* for rows that cannot carry front matter rather than
    an alternative for rows that can, and an orphaned sidecar should be a
    load error naming its missing subject. Open: precedence against
    markers and rule defaults; whether a sidecar makes a passthrough row
    `rendered`; and how much of the sidecar half is real at all, since
    its only current consumer is alt text for 838 images that nobody has
    committed to writing.

    **What this must NOT do is infer page-vs-component from an
    absence.** The heuristic is tempting and measures well — `<title>`
    presence predicts 55 of 57, document completeness 56 — and both are
    wrong, in the way this document keeps finding things wrong.
    `demos/1996/mystery.html` has a `<TITLE>` and no `<html>` because
    1996 made those tags optional: a complete page the completeness test
    calls a fragment. `demos/css-glass-pane/index.html` is a real
    880-byte demo with no title and no `<h1>` at all. Reading a title
    that exists is derivation; concluding from its absence that a page is
    a component is guessing, it fails toward *not rendering something*,
    and §5h already has the rule — the engine never guesses the
    arrangement. The two exceptions are exactly what the sidecar half is
    sized for.
50. **Transplanting an imported page** *(Matt's case)*. Import a raw HTML
    page, then lift the *meat* out of it and render that through the
    theme. No mechanism today: add front matter to a full document and
    the whole `<!doctype html><html>…` becomes the row's content, which
    the shell then nests inside another document. Two operations, and
    they should not fuse into one word the way `layout: light` fused
    shell and theme:

    **Extraction** — where is the meat? `<body>`'s children, or a
    selector-scoped region. This is a *parse* instruction, not a
    presentation one, and it rides with machinery already scheduled: the
    same HTML parse as q49's derive half (reading `<head>` for a title is
    one step from reading `<body>` for content) and §6d stage B's
    selector-driven rewrite stage.

    **How much chrome the transplant then wears** is q44's row `shell:`
    (`none`/`light`/`html`) — already a real axis with real occupants,
    not something this question needs to invent.

    Left open by the split: a transplanted page arrives with its own CSS
    and structural assumptions, so `light` may be the honest destination
    for most imports and `html` the ambitious one — and a theme that
    wants to render a transplant with *less* furniture can only say so
    today by **omitting the hole** for a part, which the binder does not
    flag. That makes a deliberate omission byte-identical to a forgotten
    one (§5h's `listing--cards` footgun), and §5h wants a load-time
    warning for the forgotten case — which cannot be built until the two
    are distinguishable. **How does a theme say "I deliberately do not
    place this part"?** Settle that and a `light`-style theme is a theme
    file rather than an engine feature.

51. **One row type: a path carries properties, a route decides where it
    lands** *(Matt's shape; all but the last step built)*.

    Matt: *"Why don't we merge kind=post and kind=tree. The filename format
    becomes an automatic way to assign a property from a filename. Then the
    ROUTE determines where a file lives. In a tree it uses the file's full
    path. In a blog, the date overrules the file's full path."*

    **The spine.** A file's path carries *properties*. A route template spends
    them. What is left over is the row's identity. `posts` and `tree` are not
    two kinds of thing — they are two habits about which property a route
    spends. The mechanism already exists three times (`filename_formats`, and
    both i18n selectors — §6f names the pattern), and the prefix selector
    settles its shape: it reads a *directory component*, so this is a path
    mechanism, not a filename one.

    **What still blocks the merge**: two disjoint route-token suppliers,
    neither offering the other's.

    | | tokens a route may spend |
    |---|---|
    | posts (`db.rs`, inline) | `year`, `month`, `day`, `slug` |
    | tree (`path_tokens`) | `path`, `dir`, `stem`, `name`, `ext` |

    So `_posts/rust/hello.md` **cannot** route to `/rust/hello/` — a post
    cannot see its own folder — and `writing/2019-thing.md` cannot route by
    year, because nothing parses a tree file's name. Merging is: one supplier
    offering path tokens always, plus whatever an extractor produced. The
    validation it needs is already written and only reachable from the posts
    path (*"this route asked for a property the path did not produce"*);
    generalizing it is moving it, not inventing it.

    **Built, 2026-07-19 → 07-21**, every slice byte-identical on all three
    sites: collection naming from the source directory; the flag family onto
    `Page` (closing a leak where `draft: true` on a page published the row and
    listed it in `sitemap.xml`); `theme`/`shell`/`fields`/`images`/`order`
    onto `Post`; `date`/`tags` onto `Page`, so `group_by = "date.year"` over
    the tree materializes and chronology became a question about the row's
    properties rather than which struct held it; then one `Row` type and the
    consumer collapse. No row holds a body — `store::read_body` is the single
    answer, at no measurable cost (the build is render-bound, not I/O-bound).

    Three rules those slices bought, each learned from a silent failure:

    - **A `.schema.toml` may not declare a base field name.** Base fields
      answer first, so such a declaration parsed, validated, and was
      unreachable. Now a load error naming the file.
    - **Ordering belongs to the SET, not the table.** `posts.order` carried
      three things at once — the sort, undated-last, and a **default-locale
      filter** — and any view that read it inherited all three without saying
      so. They now live in three stated places, and adjacency reads a
      collection's declared `adjacency` set, so "previous in `published`"
      skips drafts by construction rather than by the accident that drafts are
      undated.
    - **Adding a field to a row type is not done when the field exists.** It
      is done when every consumer that hardcoded its absence has been found —
      and the compiler cannot find them, because the old code still
      type-checks. Step 3 shipped two fresh bugs exactly this way, both
      byte-identical *because* the consumer ignored the new field. **For an
      additive capability, byte-identical is necessary and proves nothing.**

    **What is left: one table** — folding `PostsTable` and `TreeTable` into
    `SiteDb.rows`. ~45 of the remaining branches are bookkeeping ("which `Vec`
    do I index"), which makes it look like pure deletion. Two reasons it is
    not: **indices shift** (the tree loader's `by_logical`/`by_url`/claim
    checks are built against its own 0-based vector, so both loaders should
    return bare `Vec<Row>` with indexes rebuilt once over the concatenation),
    and **a membership predicate is genuinely needed** (a posts view ranges
    over the whole posts *table* across every posts collection, with
    `published` narrowing by FLAG rather than by source; a precomputed
    `post_rows: Vec<usize>` is the cheaper shape). Everything else is
    substitution.

52. **Relations declared per collection, with exclusions** *(Matt's
    direction, 2026-07-20; shape B recommended)*.

    Matt: *"Each collection should define its own relations in the config
    tree — prev/next/similar/etc, as well as the source for it. We should also
    be able to add compound operations. For example, a related post for
    published is `relation(published) - prev(published) - next(published) -
    links_to(*)`."*

    The motivating case is exact: a **Related** list should not re-show the
    post you already link to in the body, nor the two the Later/Earlier links
    already point at. Today it can and does — `similar` ranks over the whole
    posts table and knows nothing about the other axes.

    **Where this comes from.** Relations are hardcoded: five of them, in
    `parts.rs`, unconditional, ranging over whatever table the code reached
    for. That was already wrong (adjacency crossing two dated collections was
    measured and fixed in q51). Matt's rule is that **every relation states its
    reach** — and the reach is a **SET, not a collection**: `prev(published)`
    rather than `prev(posts)`, so adjacency drops drafts by construction.

    ### Four shapes, weighed

    **A. Set algebra in a string** (Matt's spelling,
    `select = "similar(published) - prev(published) - …"`). Compact and
    general, but it is a **second expression language** beside §5f's CEL
    subset, and it **repeats definitions**: `- prev(published)` restates what
    the `earlier` relation already says, so changing one silently desyncs the
    other — the §5c disease in a new place.

    **B. Structured fields, exclusions by NAME** *(recommended)*.

    ```toml
    [collections.relations.earlier]
    of    = "prev"
    over  = "published"
    label = "@earlier"

    [collections.relations.related]
    of      = "similar"
    over    = "published"
    exclude = ["earlier", "later", "links_to"]
    limit   = 4
    label   = "@related"
    ```

    No new grammar; every part is a key, load-checked like every other key.
    **`exclude` names other declared relations**, so it cannot drift from
    their definitions — say "not whatever Earlier shows" and it stays true
    when Earlier changes. `of` names the ranking operator, so *it* supplies
    the order and `limit` applies after exclusion. What it gives up is
    arbitrary algebra; the motivating case is one ranked source minus some
    exclusions, so grow it when something real wants a union.

    **C. Relations as sets.** Rejected: a set is row-independent by
    construction and a relation is row-relative. Making sets know "the current
    row" would break what makes them composable.

    **D. Keep relations engine-defined, declare only reach and exclusions.**
    The fallback if B proves too big — but a new relation still needs engine
    code, and `related_excludes` is an ad-hoc key rather than a mechanism.

    ### Decisions inside B

    - **Operators are the closed set; compositions are open.** The engine owns
      `prev`, `next`, `similar`, `links_to`, `linked_from` and the tree
      family; a typo in `of` is a load error naming them. The *names*
      (`related`, `later`) become site vocabulary.
    - **Which reopens the `Axis` enum**, closed earlier the same day precisely
      because the axis string is a theme contract. Axis strings become
      site-defined and labels move out of `ENGINE_STRINGS` into each
      relation's `label`. Themes already cope — the `relation` fragment
      renders axes it has never heard of — but the reversal should be
      deliberate rather than discovered in a diff.
    - **A collection that declares no relations gets the conventional five**,
      so `examples/minimal` does not regress by ~12 lines on a config tracked
      at 27. **Overriding is per NAME, not wholesale**: field-notes wants to
      change exactly one thing (`related` gaining `exclude` and `limit`), and
      wholesale replacement made it restate all five plus both on the tree
      collection — twenty-eight lines to customise one. Removing a default
      then needs a spelling (`enabled = false`), a far smaller wart.
    - **Labels are `@references`**, reusing `[i18n.strings]`, not five inline
      per-locale maps per collection.
    - **`linked_from` stays global** (`over = "*"`) and must now say so: a
      page linking to a post is a real backlink, and anchoring it would break
      it.

    ### The tree family belongs here too *(Matt, 2026-07-20)*

    `parent`, `children`, `ancestors`, `siblings` and `descendants` are
    relations. They exist today as two special-purpose consumers rather than a
    vocabulary: `trails::ancestors` climbs URLs for breadcrumbs,
    `outline::section_tree` walks paths for section navigation. §3 already
    claims `children(page)` as a derived relation; no such function exists.
    With them the operator set is four families:

    | family | operators | keyed on |
    |---|---|---|
    | **path** | `parent`, `children`, `ancestors`, `siblings`, `descendants` | the row's position in the tree |
    | **order** | `prev`, `next` | a sequence |
    | **metric** | `similar` | embedding distance |
    | **graph** | `links_to`, `linked_from` | the link graph |

    **The load-bearing separation: an operator supplies ROWS; what consumes
    them decides presentation.** A breadcrumb trail is ordered ancestry
    rendered as a path; a relations footer is a labelled group of neighbours;
    a section tree is a recursive nav. All three could select through one
    vocabulary and still render through their own fragments — which is a
    second argument for B, where `of` names an operator and presentation lives
    in separate keys, over A, where a select-string implies a rendering.

    **Sequencing caution.** Trails and section trees work and are
    byte-verified. Rewriting them onto this mechanism is a large refactor with
    real regression risk and no user-visible gain. Add the operators to the
    vocabulary so new relations can use them; leave the two working consumers
    alone until something needs them unified. **The vocabulary is the
    deliverable, not the rewrite.**

    Feasibility: `links_to` is free (`backlinks_map` computes the forward
    direction and inverts it). Migration is byte-identical either way — both
    sites declare their relations to keep current output, or take the
    defaults.

53. **Axes: alternative forms of a row** *(Matt, 2026-07-20)*.

    Matt: *"I'm starting to reconsider whether translation is a relation —
    it's really just an alternative of the current document. What if we split
    axis and relation. There might actually be other axes for entries — for
    example, a wikimedia-style HTML page for objects. The images have
    thumbnails on an axis."*

    | | **relation** (q52) | **axis** |
    |---|---|---|
    | points at | *other rows* | *other forms of THIS row* |
    | examples | prev, next, similar, links_to, parent, children | translations, thumbnails, serializations, an object's description page |
    | renders as | a labelled group in the body | `<link rel="alternate">` in the head, plus an inline affordance |
    | needs a reach? | yes — which set does it range over | no — the row determines its own members |

    **The last row is how this was found.** Under q52, `translations` was the
    one relation needing no `over`, and the first draft wrote a justification
    for the anomaly instead of reading it. It is not an anomaly: a relation
    asks *which other rows*, an axis asks *which other forms of me*, and there
    is nothing to range over. This document already used the word that way —
    §5g calls the md twin *"a second, orthogonal axis"*.

    **The mechanical definition: one row, several routes, keyed by a
    variant.** Which unifies four things, three already built by three
    mechanisms never seen as the same shape:

    | instance | variant | status |
    |---|---|---|
    | locale-parallel routes (§6f) | the locale | built |
    | thumbnails (§6b) | the size, content-addressed | built |
    | the md twin (§5g) | the serialization | specced |
    | an object's description page | "the page about this" | **inexpressible today** |

    The last is Matt's Wikimedia case and the one that shows the gap: an
    object is bytes at a URL with no way to have a page *about* it. The web
    models all of this already — `rel="alternate" hreflang`, `srcset`,
    `rel="alternate" type=…` — which is a good sign the cut is real. **Axes
    are `rel="alternate"`.**

    ### What the split immediately reveals

    **Nothing emits `hreflang` or `rel="alternate"`.** Grepped: not in the
    engine, not in either theme. A French page never announces its English
    twin to a crawler. Translations render as a body footer group that CSS
    hoists into a corner chip, and nowhere else — exactly the blind spot the
    misclassification produced, because relations live in the body and nobody
    thought to look in the head. A real SEO defect, small and self-contained,
    and the obvious first thing to build off this entry.

    ### What it costs, and what is open

    **`data-axis` becomes misnamed** — relations stamp it into markup and both
    themes key CSS on it; under the split it should be `data-relation`. Small,
    but a theme contract, and the third revision of that vocabulary in one day
    (closed into an enum in the morning, reopened by q52, renamed here). The
    churn is fine; doing it by accident would not be.

    Open inside this: whether an axis member gets a full row or a projection
    (a thumbnail is not a row, a translation is, a file-description page is
    generated — probably an axis member is a ROUTE plus enough facts to render
    an alternate link); where axes are declared, given thumbnails come from
    the image pipeline and locales from `[i18n]`, so unifying the declaration
    may be more disruption than the idea is worth; and whether the md twin
    rides this (plainly yes, which answers the shape if not the priority).

### Settled ledger

One line per retired question; the named section carries the design.

| q | settled as | carried in |
|---|---|---|
| 3 | fresh `grackle.toml`, no `_config.yml` migration — settled by building both sites | §4 |
| 5 | three explicit not-content layers (gitignore / dotfile / declared exclude) | §4c |
| 7 | profiles gate materialization; `/hidden/` is a drafts-profile view, URL parity holds | §4a |
| 8 | hidden rows don't exist in the public profile, so no hidden neighbour renders | §4a |
| 9, 9a | buckets + bubbling resolve bare names with no restructuring; the two genuine collisions stay unreferenced — leave | §6a |
| 12 | derived assets move to `/static/{hash}`; URL parity stays hard for pages, exempt for derived assets | §6b |
| 15 | no template language: a template may not contain control flow; `liquid` retired by not taking it | §5d |
| 16 | no custom AST→HTML renderer; AST mutation + escape hatches per node type (tripwire: ~⅓ of node types) | §9a, §8a |
| 17 | truncation is a `truncated` Flag → `data-truncated` → theme-CSS ★ | §6d |
| 18 | sidenotes are a theme decision: the `notes` stream placed by whichever theme claims it, endnotes canonical | §5e, §6d |
| 19 | route-level fix landed (`draft`/`hidden` on every Route; sitemap filter); profiles still ride phase 3 | §4a |
| 20 | themes are directories of fragments + SCSS; a third theme is `mkdir` | §5e |
| 24 | fragment variants: `{kind}--{variant}` → base → canonical; `data-fragment` resolves at load | §5e |
| 27 | index-less dirs render as unlinked labels in section trees; the auto-index view is a landing with no intro row | §6e, §5h |
| 29 | `{% callout %}` widgets: `[widgets]` registry, paired-tag expansion, no arguments, no conditionals | §5d |
| 31 | expressions extend `filter.rs` as a strict CEL subset, no borrowed engine; build at the q23 forcing point | §5f |
| 32 | producers take URLs — pagination/tag routes render from the owning view's templates | §5c |
| 35 | `.section` is a bare marker file; `order:` is a page field; nested sections nest, nearest wins | §6e |
| 36 | one preview kind: `summary` (presence-driven), `card`/`card_list` deleted, `featured` slot on listing | §5e |
| 41 | i18n: locale axis, `by_logical` pairing, translations axis, locale-parallel default-on, enum records | §6f |
| 44 | shells: root HTML shell engine-owned; atom/sitemap/search built-in; script shells as the bench; md specced; row tiers are pipeline exits (`none` is the shell layer's escape hatch, not an object and not a theme) | §5g |
| 10 | the drafts profile forces `noindex` site-wide — one profile key, not a per-row flag | §4a |
| 45 | landings: a view owns the URL, a row may own the words; claiming, the chain, theme provenance | §5h |
| 46 | `collection.crumb`/`index` dissolved — the URL climb is the sole source of a landing crumb, `trail` keeps the subdivision chain | §5h |
