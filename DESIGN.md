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

**1. You write a file** — `_posts/2026/2026-07-17-espresso-grinder.md`, with
`title` and `tags` in the front matter, and `![](burrs.jpg)`. Every file belongs
to exactly one table by precedence — posts, then objects (by extension), then tree.
The post lands in `blog` at `/blog/2026/07/17/espresso-grinder/`.

**2. Views query it.** Queries are declared once, never in loops:

```toml
[sets.published]
from  = "posts"
where = "!draft && !hidden"

[routes.blog_index]
from     = "published"
paths    = ["/blog/", "/blog/page/{n}/"]
```

**A view is a query; a route is where it lands.** The new post enters
`published` and appears in `/blog/`, the feed, tag pages, archives, and home.

**3. Rendering produces structure, not a string.** The body becomes a doc model:
blocks (addressable by position), notes (footnotes), rewrites (CSS selectors),
and facts (typed truths: `data-truncated`, image dimensions).

**4. A layout kind fills named parts.** Kinds are `document`, `listing`, `feed`,
`raw`. A `document` emits a part map — `title`, `crumbs`, `tags`, `content`,
`notes` — each flat, semantic HTML.

**5. The theme places parts in slots.** A fragment is straight-line HTML with
holes: `data-slot="title"`, `data-slot-href="url"`. An empty part **deletes its
element** (every `{% if %}`); a stream **maps a fragment over items** (every
`{% for %}`).

**6. CSS does the geometry.** Modern CSS (nesting, `:has()`, `@layer`) arranges
parts. A margin column is four lines of CSS. The footnote becomes a sidenote
without touching any layer above CSS.

**7. Build, serve, query — clients of one database.**

```
$ grackle build     # materialize every route (~0.4s)
$ grackle serve     # resident db, save → invalidate → browser reload
$ grackle query 'posts where "rust" in tags'
$ grackle explain /blog/2026/07/17/espresso-grinder/
```

### Day two: every change has exactly one home

| you want | you touch |
|---|---|
| a new post | one markdown file |
| hide a subtree from search | `touch code/legacy/.noindex` |
| a recent Rust posts box | a `[sets]` entry with a filter |
| a new look, dark mode included | copy a theme directory, edit HTML + CSS |

The rule: **want an `if` → you're missing a fact; want a `for` → you're missing a
view** (both are design bugs, caught at load time).

## 1. Core idea

grackle is a **virtual, on-disk database**: the filesystem is the storage
layer, and grackle maintains a live, queryable view over it.

- **Tables** are directories (posts, page tree). **Rows** are files.
- Rows are **virtual**: hydrated lazily, in stages
  (stat → front matter → body → rendered HTML), each stage cached.
- **File watchers are the replication stream**: fs events become row
  upserts/deletes that advance the database revision and invalidate exactly
  the cached derivations that depended on the changed rows.
- **Queries/views** are demand-driven and memoized against the current revision.
- `build` = materialize one consistent snapshot to disk (AOT).
- `serve` = keep the database resident; render pages on demand per HTTP request.

## 2. Storage engine

```
FsStore
  ├─ table mapping: directory ↔ table (from config)
  ├─ row identity: source path, always (§3; (date, slug) is an INDEX)
  ├─ row version:  content hash (mtime+size as a fast pre-check)
  └─ event ingest: notify watcher → debounced batch → one transaction
```

- **Hydration stages per row**, each pull-through cached:
  1. `stat` — existence, version
  2. `head` — front matter only (cheap; sufficient for indexes, lists, routing)
  3. `body` — raw content
  4. `rendered` — liquid → markdown → HTML (pre-layout)

  Index/list queries only force stage 2. Only an actual page render forces stage 4.

- **Revisions & snapshots (MVCC-ish).** Each event batch produces a new revision.
  Readers (HTTP request, `build` run) pin an immutable snapshot, so mid-render
  edits never tear output — `build` renders the entire site from exactly one revision.

- **Debounced transactions.** Editor save-storms coalesce into one revision, one
  invalidation pass, one reload ping.

- **Invalidation** is tracked per derived value as a set of typed keys:
  `Row(path)`, `Index(blog.order)`, `Index(blog.tags)`, `Template(name)`, `Config`.
  A post body edit invalidates that post's `rendered` and pages embedding it,
  but not tag pages (which read stage-2 fields only) unless front matter changed.
  Template/SCSS edits invalidate by `Template(...)` key.

## 3. Tables

**Row identity is always the source path**, for both table kinds. `(date, slug)`
is a *unique index* over posts, not the primary key — drafts have no date in
their filename. Identity = path keeps every row addressable; dated-ness is a property, not an identity.

| Origin | Identity | Primary index | Source |
|---|---|---|---|
| `posts` | source path | `(date, slug)` unique | `_posts/**` |
| `tree` | source path | path hierarchy | site root |
| `objects` | source path | `by_name` (non-unique) | by extension |

**One store, three origins.** These were three tables; they are one `SiteDb.rows`
and three lists of keys. Objects went last because q51 had already written every
index to gate on a row's PROPERTIES rather than which vector it arrived in.

**Membership is a filter now.** An object view's base is `collection ==
"<the objects collection>"`, ANDed onto the view's own predicate. The two halves
parse against *different* schemas on purpose — the author's filter type-checks
against `object_schema`, so `where = "draft"` on a gallery is a load error, while
the membership clause names a column only the full row schema has.

- **Posts**: ordered rows, reverse-chronological over the dated set. Secondary
  indexes: `by_slug`, `by_tag`, `by_year_month`, adjacency (`next`/`previous`).
  Undated rows (drafts) are absent from the chronological indexes and sort last.
- **Tree pages**: hierarchical. Derived relations: `ancestors(page)` (breadcrumbs),
  `children(page)`.
- **Objects**: binary assets, selected by extension. `by_name` is non-unique
  (multiple `screenshot5.png` can exist), so resolution is a query that can fail.

### Membership is disjoint

A file belongs to **exactly one** table, resolved by precedence:

1. **posts** — under a posts collection's `source`, and a `.md`
2. **objects** — matches a configured extension
3. **tree** — everything else

This removes a whole class of ambiguity: without it, `assets/x.png`
would match both the objects table and the tree's passthrough.

## 4. Schema: rules (defaults + routing)

One mechanism covers both "everything in this folder is a draft" and "this
path shape gets this URL": an **ordered list of rules** per collection. Each
rule is `match` (glob) plus `defaults` (front-matter values) and/or `route`.

Rules supply column values for the subset of rows they match, like a `DEFAULT`
clause scoped by a predicate.

```toml
[site]
url     = "https://grack.com"
title   = "grack.com"
author  = "Matt Mastracci"

[[collections]]
kind   = "posts"
source = "_posts"
filename_formats = ["{year}-{month}-{day}-{slug}", "{month}-{day}-{year}-{slug}"]

  [[collections.rules]]
  match    = "drafts/**"
  defaults = { draft = true }
  route    = "/drafts/{slug}/"

  [[collections.rules]]
  match    = "**"
  defaults = { layout = "post" }
  route    = "/blog/{year}/{month:02}/{day:02}/{slug}/"

[[collections]]
kind   = "tree"
source = "."

  [[collections.rules]]
  match = "**/index.{html,md}"
  route = "/{dir}/"

  [[collections.rules]]
  match = "**/*.{html,md}"
  route = "/{dir}/{stem}/"

[[collections]]
kind       = "objects"
extensions = ["png", "jpg", "jpeg", "gif", "webp", "svg"]
bucket     = "assets"

  [[collections.rules]]
  match = "assets/branding/logo.png"
  route = "/logo.png"

  [[collections.rules]]
  match = "**"
  route = "/{path}"
```

### Named object routes

Objects are routed by ordered rules. The default `**` → `/{path}` keeps every
original where it is, which matters because `{% image %}` emits
`<a href="/assets/…/foo.jpeg"><img src="{thumb}"></a>` — the thumbnail
links to the original at its literal path. Under the default rule all of those
keep working untouched, so named routes are purely additive.

### Resolution order

**First writer wins, per key.** Rules are evaluated top to bottom; a rule may
only set keys nobody above it set. Specific rules go first, the `**` catch-all
last.

Worked example, `_posts/drafts/foo.md`:

| Source | Contributes | Result |
|---|---|---|
| rule 1 `drafts/**` | `draft=true`, route `/drafts/{slug}/` | both taken |
| rule 3 `**` | `layout="post"`, route `/blog/...` | `layout` taken; **route already set → ignored** |

→ `draft=true`, `layout="post"`, URL `/drafts/foo/`.

**Front matter in the file always beats every rule.** Rules are defaults;
an explicit `permalink:` in the file wins outright.

### Constraints (checked at transaction time, not discovered as 404s)

- **Route collisions** → error, naming both rows. *Two rows may not share a URL.*
- **One row, two routes** → error, naming the file and both URLs. *The dual, and the stronger statement:* **a row renders at exactly one route.** The legal counts are 0 (claimed by a landing view, q45 — the view owns the URL — or on-demand and unreferenced), 1 (everything else), and **N only along an axis** (q53). An axis is the sole mechanism permitted to break it; anything else producing a second route onto one row is a bug, and now says so at load.
- **Undated row routed by a dated template**: error naming the file and rule.
- **Dead rule** (matches zero rows) → warning.
- **URL-set parity** with reference builds — maintained via `grackle urls`.

### Several collections, one table *(built 2026-07-19)*

A collection is a *source*, not a table. `_posts` and `_drafts` are two
sources of one corpus — same row shape, schema, views — so every `kind =
"posts"` collection contributes rows and the posts table is indexed **once,
over all of them**. Before this, a second posts collection silently overwrote
the first with no warning.

Drafts ride this: `_drafts` is a source whose rule sets `draft = true`,
and the `!draft` filters the views already carry keep them out of feeds and
listings. They are ordinary rows in every other respect — they materialize
`/drafts/{slug}/` routes and take part in the link graph. Nothing publishes
from grackle yet; inventing draft-specific suppression now would be guessing
at what profiles (q6, q10) should decide later.

## 4a. Profiles: a projection, not a different database *(v1 built 2026-07-19)*

A profile changes **three things and no others**: which rows the views admit,
the absolute URL the output is addressed under, and a marker themes may style on.
It never changes what *loads* — the database is identical under every profile.

```toml
[profiles.drafts]
noindex = true

  [profiles.drafts.sets.published]
  where = "!hidden"          # relax the filter that hides drafts
```

`build` uses the default projection. `serve` defaults to `dev`, which changes
nothing. Any other name must be declared, so a typo is a load error naming
what exists.

**Selection is the view's job.** Relaxing `published`'s filter carries
every listing, archive and feed with it.

**Presentation costs no engine code.** The root shell stamps `data-profile`,
so a dev banner is a theme CSS rule on `[data-profile="dev"]`.

### What the corpus actually holds

The site holds 14 dated posts in `_hidden/` that no build version references, 4 undated drafts in `_drafts/` moved there 2026-07-19, and no posts with `hidden:` set (one page row does: `demos/pane.html`).

### Flags reach the row, the route and the head

`draft` and `hidden` carry onto every `Route`, exposed in `route_schema()`, so views filter them. `noindex` reaches the head. Both cascade from markers and rules as any default does (§4b). Before 2026-07-19 pages carried none of this; `demos/mindstorms/index.html` declared `noindex: true` but shipped without the robots meta tag.

### The sitemap leak, and why route-level flags exist

Worth keeping because it explains why the flags live on routes. Probed by
adding two posts dated newer than anything real — one draft, one hidden — the
flagged rows landed **in the sitemap** even though `published`, `latest` and
`/blog/` correctly excluded them. A section titled "add no public URLs" was
emitting the most public URL there is.

This was **grackle's divergence, not Jekyll's**: `publish.sh` builds drafts as
a *separate site*, so Jekyll's main sitemap never saw them. Routing drafts into
the main build created the exposure. The fix was the route-level flag plus the
sitemap's own filter. Given this project began with "I'm having trouble with
Google crawling this site", it was precisely the wrong failure mode to ship.

Profiles are the general answer the probe pointed at: a profile overrides an
existing query's `where`, so selection stays the view's job.

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
    .hidden                 ← every post in here, and below, is hidden
code/legacy/
  .noindex                  ← the whole legacy subtree
```

This is the **same principle as buckets**: positional resolution, nearest wins.
Add a marker → it applies from there down. Delete it → the default lifts.
`mkdir`/`touch` is the interface; the config never names a path.

### Resolution

Walk up from the row's directory to the root, accumulating **first-writer-wins
per key** — so the *nearest* marker shadows a shallower one.

Full precedence for any default:

| Source |
|---|
| **Front matter** |
| **Markers** (nearest ancestor) |
| **Rules** (first-writer) |

A marker file is never routed and exempt from dotfile skips. The marker scan
must carry `.gitignore` globs itself since it walks dotfiles; this yields a
35× speedup: ~205ms to ~6ms over ~1500 files.

### What this replaces

The `drafts/**` and `hidden/**` rules in §4 become unnecessary when the tree
declares itself. The rules mechanism stays for what it is genuinely better at:
routes and patterns that cut *across* the tree. Markers own per-subtree defaults.

### `noindex` and the layout chain

Markers make `noindex` computable from the tree — but not completely: a
layout can also set it (`tag_index.html` has `noindex: true`). Posts' `noindex`
is complete (marker + front matter); view routes still need the layout chain
and stay out of the route schema until phase 2, on the same reasoning: a field
we cannot populate correctly is worse than no field (q33(e)).

## 4c. What counts as content: three layers

`gitignore = true` (default). Three mechanisms, each doing what only it can:

| Layer | Covers |
|---|---|
| **`.gitignore`** | build artifacts: `_site*`, `_log*`, `vendor`, `_cache`, `.jekyll-cache` |
| **dot/underscore skip** | `_posts`, `_layouts`, `_sass`, `_includes`, `.git` |
| **`exclude`** | `docker/`, `scripts/`, `TODO`, `*.sh`, `Gemfile`, `*.yml`, `*.toml` |

Tracked content files are excluded by the dot/underscore skip and `exclude`;
`.gitignore` handles untracked build artifacts.

### Where `.gitignore` actually earns its keep

The marker scan, which structurally cannot use the dot/underscore skip (markers
are dotfiles under `_posts`), depends entirely on `.gitignore` for performance,
achieving a 35× difference: ~205ms to ~6ms over ~1500 files.
## 4d. The base config: `extends`, and the empty file

`crates/source/assets/base.toml` is compiled in with `include_str!`, exactly as
`parts.toml` and the base theme are, and every config inherits it unless it
says `extends = "none"`. Same word as `theme.toml`'s, because it is the same
operation — a union merge where the child wins — and one word means one thing
to learn.

### Three shapes, three rules, none of them new

The merge is the strongest evidence the cut is real: config decomposes into
exactly the three resolution rules this engine already has, and needed no
fourth.

| shape | rule | already the law for |
|---|---|---|
| `[[collections]]` | merge by **source**; the site's rules **prepend** | §4 first-writer-wins — the site's rule is nearer, so it writes the route and the base's `**` fills the rest |
| registries of definitions — `[sets.*]`, `[routes.*]`, `[markers]`, `[widgets]`, `[shells]`, `[profiles]`, `[records.*.*]`, `[i18n.strings.*]` | **shadow by name**, whole entry | a theme fragment shadowing the base's file of the same name (§5e) |
| settings bags — `[site]`, `[i18n]`, scalars | **per key, child wins** | front matter over rule defaults (§4) |

The registry rule is the one worth stating aloud: **your `[routes.feed]`
replaces the base's entire**, so you never have to know what the base put in a
table to predict what overriding it does. Merging into one would mean a site's
feed silently inheriting a `limit` it never wrote.

Collections key on **source, not name**, and that was found by walking
field-notes through the merge: it names its posts collection `notes` over
`_posts`, and a name-keyed merge would have left the base's `posts` collection
in place beside it — two collections reading one directory, every post twice.
Source is the physical thing; `name` is a label.

### What may live in the base

Two rules, and the second is the one that took work.

1. **It must be what a site would otherwise re-derive.** If a site would have
   to *undo* it, it is the site's. (The base theme's re-derive/undo test, same
   words, and it is now the third time that line has done real work.)
2. **It may not mint a URL the author did not ask for**, unless the absence of
   that URL would be a bug on any site. `/atom.xml` and `/sitemap.xml` qualify.
   Tag pages do not — plenty of sites never tag. `/search.*` does not — it is a
   real payload.

Rule 2 needed a mechanism, not just discipline: **an inherited route with no
members does not materialize.** A site with no `_posts/` never asked for an
empty `/blog/` or a feed with no entries. A route the *site* declared still
materializes empty — declaring it is asking, and an empty listing you wrote is
a fact about your content rather than a stray page. Which view is whose is
recorded before the merge blurs them (`View::inherited`).

### `/` is offered, not taken: `default_content`

`/` is the one URL every site has its own opinion about, and the base ships it
anyway — because the alternative was an empty config with no homepage. What
makes that safe is a new key, Matt's shape: **`default_content` claims a row if
that row exists.** The base's `[routes.home]` carries
`default_content = "index.{md,html}"`, and three outcomes fall out, each
leaving exactly one thing at the URL:

| the tree says | `/` is |
|---|---|
| no `index.*` | the route: the ten newest posts |
| `index.md` **placing `{% view home %}`** | the row, owning the arrangement — an ordinary q45 mode B claim from there on |
| `index.md` **without the embed** | the row, by its own tree route; the offered route **stands down** |

The third row is what makes this safe to inherit, and it was found by
breaking: the first build of grack.com under the base config failed with
*"view home: content index.html never places `{% view home %}`"*. grack.com's
homepage is hand-built and says nothing about `[routes.home]` — a route it
never wrote must not change how it renders. **An explicit `content` is a
promise the engine holds you to; a defaulted one is an offer the row may
decline.** Once the offer can be declined, the must-place check (§5h) stays
exactly as strict as it was, because a defaulted claim only exists where the
row already took it up.

This is not the engine guessing the arrangement (§5h's rule). Both outcomes are
*declared*, in `base.toml`; which one applies is a fact about the tree — the
same shape as every marker in §4b.

### The site icon is a URL, not a key

The second thing every site has an opinion about. `site.icon` is the first of
`/favicon.{svg,png,ico,webp,gif}` that any row occupies, under `baseurl`, and
the base declares what to do with it —

```toml
[html.head.link]
icon = 'site.icon'
"shortcut icon" = 'site.icon'      # the legacy alias, and deletable
```

Four things fall out, none of them new machinery:

- **Dropping `favicon.svg` at the root is the whole interface.** No key, no
  registration.
- **An icon that lives elsewhere is pinned by a named object route** (§4) —
  `match = "brand/icon-v3.png"`, `route = "/favicon.png"`. Resolution keys on
  the *published URL*, so the feature that already existed for logos covers
  this one for free. Had it keyed on a filename it would have needed its own
  bucket rule, and §6a's non-unique `by_name` would have had to answer a
  question with no non-arbitrary answer.
- **Nothing publishes it explicitly.** The `<link>` is a citation, and
  `materialize_referenced` publishes what the chrome cites — the case that
  function's own doc comment already named ("the shell's favicons … are cited
  by chrome that no body contains") and had no producer for.
- **No icon means no tag**, in the head and in the feed alike: `site.icon` is
  empty, and empty deletes its element (§5e rule 2). This is what makes it
  safe to inherit, and it is the same sentence as `default_content` above —
  the base states both outcomes and the tree picks one.

The legacy `rel="shortcut icon"` is a **line in `base.toml`, not engine code**,
which is the point of the head being config: a site that has stopped caring
about browsers that old deletes it, and the duplicate `href` — the whole reason
that spelling exists — costs nothing on a site with no icon.

### Absent `layout:` means a document

A prerequisite, and a defect in its own right. `Some("page") | Some("post")`
selected the document part map and everything else fell to the raw body, so a
row that omitted `layout:` lost its furniture **with no error** — which is
precisely why every config had to carry `defaults = { layout = "post" }`. The
key names `_layouts/*.html` files nothing has read since §5e (q33(f)).
Absent now means `document`; `layout: default` remains the escape hatch, and it
is the one value that always said what it meant. The base config therefore ships
no `layout` default, and the 122 `layout: page` rows on grack.com are now
saying nothing.

### `extends = "none"`, and what it does not turn off

Three floors exist now, on three substrates: the part vocabulary
(`parts.toml`), the base theme, the base config. **`none` drops only the
third.** Different substrates, different opt-outs — one switch for all three
would be a worse story than three honest ones.

Two live users, which is what keeps the escape hatch real:

- **`examples/raw`** is the base config *printed*: the same content tree as
  `minimal`, everything spelled out. A test holds the two to the same URL set,
  so the printed copy cannot drift from the compiled one without going red.
  It is also the answer to "what am I inheriting" that a person can read and
  argue with; `grackle config --effective` is the exact one.
- **`theme-preview/`**, whose shape the base did not anticipate: six posts
  collections, one per theme, and no site-level blog. Every `kind = "posts"`
  collection feeds the one posts table (§4), so the base's `published` swept
  all six into one `/blog/`. One line instead of shadowing five routes.

That second case also explained something this document had read as
redundancy: theme-preview's sets restate `collection == "…"` beside
`from = "<that collection>"` because **`from` a posts collection does not scope
to it** — it ranges over the whole posts table. Not a merge problem; a
composition wart the merge made visible.

### What it cost

The base config merge is inert on sites that already declared everything, verified by test with mutation checks on concatenation order, keying, and registry depth.

### Honest edges, named now

- **The base config is a compatibility surface.** A base route added in 1.1
  mints URLs on every stock site. Policy, stated now: **base-config changes
  that mint URLs are breaking changes.** The base theme has the same exposure
  and no policy yet.
- **`grackle config --effective`** — printing the merged config with
  provenance per key — is what makes this inheritance rather than magic, and it
  ships (MERGE.md B3). The merge itself records which writer supplied each
  atom, so the output cannot disagree with the load; `examples/raw` stays the
  readable copy beside it. It is `explain`'s "which rule wrote which key" one
  level up.

## 4e. The flag family is not engine vocabulary

The audit: **`draft`, `hidden` and `noindex` are ordinary declared bools that `base.toml` ships in `[schema]`; the
engine's own row schema no longer mentions them.** `extends = "none"` genuinely removes them, and `where = "!draft"` on such a site is a load error
naming the knowns — which is exactly what `examples/raw` and `theme-preview`
now declare `[schema]` to avoid.

What remains is two spellings, both narrow and both named above: a view's
`noindex = true` copied onto its routes, and `Site.noindex` as the drafts
profile's record of itself. Neither is schema; both dissolve when a view can
declare arbitrary route fields.

| where | what | verdict |
|---|---|---|
| `relations.rs` | the engine composing `"!candidate.draft && !candidate.hidden"` onto a defaulted pool | **deleted** |
| `load.rs` | `Cascaded`, a struct of **seven named fields**, the only keys a marker could set | **four**, then none — the four left (theme, shell, layout, toc) are what the engine still READS by name, and MERGE.md C1 declares them in the base's `[schema]` so they cascade typed like everything else. `Cascaded` survives as the typed read. |
| `model/lib.rs` | `Row.draft/hidden/noindex`, `Route.draft/hidden`, both schemas | **deleted** — declared fields, carried in `Row.fields` and `Route.fields` |
| `debug.rs`, `main.rs` | the inspector and `explain` printing three named bools | **deleted** — both print declared fields now |
| `render.rs`, `build.rs`, `passes/listing.rs` | `noindex` → `<meta name="robots">` | **`[html.head.meta]`** — the one invention, below |

### Every row is governed

`Schemas::resolve` used to return `Option`, and `None` meant "no `.schema.toml`
governs this path, so tolerate any front matter." That tolerance is gone:
resolve returns a map, possibly empty, and an undeclared front-matter key is a
load error naming the knowns wherever it appears.

It cost exactly one line across every site in this repo: `hide_sidebar: true`
in grack.com's `index.html`, a Jekyll-era key that nothing had read since the
port and that the tolerance had been hiding. That is the whole argument for the
rule — the failure it produces is a dead key named out loud, and the failure it
replaces is a live key silently ignored.

Two consequences worth stating:

- **A site that declares nothing is governed by an empty schema**, so its error
  names zero knowns. That reads oddly for a second and is correct: the site
  said nothing, so nothing is allowed.
- **`extends = "none"` is now load-bearing for vocabulary, not just routes.**
  Declining the base declines its `[schema]`, which is why both no-inherit
  sites in this repo grew three lines. That is the cost of the escape hatch
  being honest.

### The two axes config was missing

`.schema.toml` is **positional**, and §4 explicitly supports one collection
with several sources — so "every post has a `series`" had to be copied into
`_posts/.schema.toml` *and* `_drafts/.schema.toml`, two declarations that can
drift. That is the disease `[sets.published]` exists to cure, one layer down.
Two config axes join the positional one, resolved by the same law:

```toml
[schema]                              # every row of the site
archived = { type = "bool" }

[collections.notes.schema]            # every row of one collection
series = { type = "string" }
```

**Nearest wins: positional beats collection beats site-wide**, because a
`.schema.toml` sitting beside the rows is the most specific statement anyone
made about them. `[schema]` is where the base config will declare the flag
family, which is the point of having it — those are properties of a *row*, not
of a directory, and no positional file could say so without sitting at the root
of every site.

### Why the flag move was one step, not two

`route_schema()` declares `draft`/`hidden`
because `from = "*"` filters over routes, and the base config's own sitemap
uses them. A route's vocabulary must include what its `where` clause can filter on. **A
filter environment that type-checks a name nothing can answer is a worse
failure than the hardcoding it replaced**: it fails at runtime, as `false`,
silently.

So it went as one: `Row` lost the three, `Route` lost its two and gained
`fields`, `route_schema()` takes the declared set, and `SiteDb` carries the
site's vocabulary (`db.declared`) because a consumer that wants to parse a
filter needs the site's names rather than the engine's.

**Two defects fell out of the audit; both are fixed.** `.schema.toml` fields
were nameable in `order_by`, `group_by` and a relation's `rank` but **not in a
view's `where`** — declare a bool, set it, group by it, sort by it, then get
`unknown field` from your own filter. `Schemas::row_filter_schema` is the one
definition now, and `where` is its third consumer rather than its exception.

The sharper one: **a marker or rule default could not set a declared field at
all**, because `cascade()` read the names out of a Rust struct — so `[markers]
".archived" = { archived = true }` did nothing, silently. `CASCADE_KEYS` went
to four (`theme`, `shell`, `layout`, `toc`) and then to **none**: MERGE.md C1
declares those four in the base's `[schema]` too, so *every* key a marker or a
rule sets goes through `schema::apply_defaults` — a declared field or a load
error naming the knowns, front matter still the nearer writer, a type mismatch
also an error. **§4b's mechanisms work for any field a site invents**, which is
what the flag move was for.

Being engine vocabulary turned out to be a statement about who *reads* a field,
not about who types it: the engine still reads all four off a row by name, and
a declaration is simply how their cascade gets typed. Their types are the
engine's — declaring one at another type is a load error, since the value would
be typed one way and read the other.

### The head is config: `[html.head.meta]`

`noindex` is not a
query concern — nothing filters on it — it is an *output* concern: whether a
`<meta>` is emitted. So the binding moves into config as an expression:

```toml
[html.head.meta]
robots = 'noindex ? "noindex" : ""'      # empty ⇒ the meta is not emitted
description = 'description'
```

Three notes on the shape:

- **A conditional belongs here.** §5d's no-control-flow rule governs
  *templates* — a fragment wanting an `if` means a missing fact. This is the
  expression surface (§5f), which is exactly where a conditional is legitimate,
  and "which string does this meta take" has no fact-shaped spelling.
- **Spell it as CEL's own ternary.** §5f's contract is that every expression is
  *grammatically valid CEL*, and CEL has `a ? b : c` natively — so
  `noindex ? "noindex" : ""` costs nothing, while `if_else(…)` would be a
  registered grackle function and a small divergence. Functions are registered
  in Rust and allowed (§5f), so `if_else` is not *wrong*; the ternary is just
  already there.
- **Empty means absent**, which is §5e's rule 2 one layer up: an empty part
  deletes its element, an empty meta value emits no tag. One rule, two places.

**As built**, four notes:

- **`Head.noindex` is gone**, replaced by `Head.meta: Vec<(String, String)>` —
  already-evaluated pairs, so emitting them is a loop with no decision in it.
  `light_head` and `head_html` share one `meta_tags`.
- **Two compiled sets, and an expression must fit both.** A document's head is
  evaluated against its ROW, a listing's against its ROUTE, and the two have
  different vocabularies — so `compile_metas` parses each expression twice and
  a failure on either side is a load error naming which. Stated rather than
  discovered, because the alternative is a meta that silently appears on posts
  and not on listings.
- **A view's `noindex = true` becomes a route field.** `[routes.tag_index]
  noindex = true` writes `route.fields["noindex"]`, so one expression answers
  for both surfaces. The engine still spells `noindex` at that one spot; the
  honest end state is `fields = { noindex = true }` on a route, once a view can
  declare arbitrary fields.
- **The drafts profile keeps working**, as a config patch: `[profiles.drafts]
  noindex = true` now overrides the `robots` declaration instead of setting a
  bool the head pass read by name. Same behaviour, said in the site's
  vocabulary. `Site.noindex` survives as the profile's own record of it.

Verified by mutation: deleting `[html.head.meta]` from `base.toml` drops every
`<meta name="robots">` on grack.com from its usual set to zero, and the site is
otherwise byte-identical — which is the proof the tag was coming from config
and not from the engine.

### The producers, and which of them fold

The head's contents sort into five classes by where their value comes from.

| class | tags | folds? |
|---|---|---|
| **row columns** | `description`, `og:description`, `og:title` | ✅ `'description'`, `'title'` |
| **config constants** | `author`, `article:author`, `viewport` | ✅ once `site.*` is in the environment |
| **derived from a column** | `canonical`, `og:url`, `og:type`, `article:published_time` | ✅ once `+` concatenates and a conditional exists |
| **variable-length lists** | `rel="alternate" hreflang` × n (q53) | ❌ a name→string map cannot repeat |
| **composites** | the JSON-LD `<script>` | ❌ a whole document, not a value |

Three additions made the first three classes reachable:

- **`site.*` in the environment.** A head says as much about the site as about
  the row. Rather than teach the evaluator about config, `HeadRow` answers
  three extra names and the schema gains them.
- **String `+`.** CEL concatenates with it; nothing in grackle had needed it
  until `site.url + url`. It also retires the one function this was going to
  need: `date` is ISO-8601 already, so `date + "T00:00:00+00:00"` is the
  Atom-shaped timestamp and no formatter had to be registered.
- **Three tables, because there are three ELEMENTS** — `[html.head.meta]` is
  `<meta name>`, `[html.head.property]` is `<meta property>` (Open Graph and
  `article:*` use the other attribute), `[html.head.link]` is `<link rel>`.
  One table with the engine deciding which name takes which attribute would
  have been the same smell in a smaller room.

**One compiled set, not one per surface.** The environment is the *head's*
vocabulary — `title` and `url` are what the head is being built for, `site.*`
is config, the rest is the row — so a listing simply reads Null for
`description` and emits no tag. That is the `if let Some(d)` that used to live
in Rust, and it is why `og:title` works on a listing where a route-schema-only
environment would have silently dropped it.

**What the engine still emits**: `<title>` (an element, not a meta), `charset`,
the stylesheet link (per-row theme resolution, not a row fact), the hreflang
list, and JSON-LD.

**The favicon block is gone.** `FAVICONS` was four lines of grack.com compiled
into the engine — two `<link rel>` and two `<meta name>` — and every site
emitted them, which `examples/minimal` made impossible to ignore: an empty
config file produced a page advertising another site's icons and calling itself
`grack.com`. The two metas moved into grack.com's own `[html.head.meta]`,
where they always belonged and where they now sit beside the base's four (a
live demonstration of the registry rule: the site's entries JOIN the base's
rather than replacing the table).

The two `<link>`s were **deleted, not moved**: they carry `sizes` and `type`,
and `[html.head.link]` is a rel→href map. The honest consequence, stated
because it is user-visible: **grack.com currently has no favicon** — there is
no `/favicon.ico` at the root to fall back to. They come back when a link entry
may be a table (`{ href = '…', sizes = "180x180" }`) rather than a bare
expression, which is the one piece of head machinery still owed.

**The `light` tier keeps `[html.head.meta]` and drops the rest.** The line is
the element, not a list of blessed names: a `<meta name>` is a fact about the
document, while Open Graph and a canonical link are apparatus for describing it
to other systems. Cost: the tier's head grew by two tags (`author`, `viewport`).

#### What the fold found

Moving the head to expressions fixed multiple defects at once: an expression reads the row rather than a hardcoded constructor default. q51 gave pages dates and typed fields; the head had never been told.

### And the inspector stopped naming them too

`debug.rs` carried three named bools in its row shape and `query explain`
printed two. Both now render whatever the site declared: the inspector folds
them into the `fields` list it already had for pages, and `query stats` prints
one count per declared `bool` that any row carries. A site's own flags were
invisible in both before — `archived` would have been a field nothing reported
on, which is the same defect one layer out.
## 5. Views (the generators, declaratively)

Everything Jekyll plugins generated becomes a declared, incrementally maintained view over a table:

> **Vocabulary** *(2026-07-19)*. This document says **view** for the shared concept, as SQL does — a query, materialized or not. Config splits it: **`[sets]`** never lands, **`[routes]`** does, and `path` is what tells them apart (§5c has the key census). A collection rule keeps `route` — it makes one URL per row, not per query.

### Route fields

`kind` `view` `url` `ext` `key` (string); `dir` (bool); `page` `rows` (int).

**`noindex` is deliberately absent.** Computing it needs the layout chain (phase 2), and a field we cannot populate correctly is worse than no field: omitted, referencing it is a load-time error; present-but-wrong, it silently lies. It also turns out not to be needed.

`over = "*"` views read the finished route set, so they run in a second pass. Views iterate in name order.

`where`/`group_by`/`limit` are deliberately tiny — a predicate language over row fields, not SQL. Anything fancier is a Rust `Generator` impl registered under a name the config references. View outputs are routable rows like any other, so they land in the same URL→row reverse index.

### The filter language

The grammar is, deliberately, a **CEL subset** — §5f pins that contract:

```
expr    := or
or      := and ("||" and)*
and     := unary ("&&" unary)*
unary   := "!" unary | primary
primary := "(" expr ")" | "*" | field | field OP literal | string "in" field
OP      := "==" | "!=" | "<" | "<=" | ">" | ">="
```

A bare field is a **truthiness** test, which is what makes `!draft` read naturally and gives `description` the useful meaning "has one": bool → itself, string → non-empty, list → non-empty, int → non-zero, absent → false.

Post fields: `draft` `hidden` (bool); `title` `slug` `stem` `layout` `description` `url` `date` (string); `year` `month` `day` `body_bytes` (int); `tags` (list). `date` is ISO-8601, so string ordering *is* date ordering and `date >= "2020-01-01"` works without a date type.

```toml
where = '!draft && !hidden'
where = 'year >= 2020 && "rust" in tags'
where = '!(draft || hidden) && description'
```

**Expressions are parsed and type-checked once per view at load time**, against a schema — not interpreted per row. This is the point, and it fixes a real hazard: a version that split on `&&`, understood only `draft` and `hidden`, and **returned true for anything it didn't recognise**, so `filter = "!drafts"` silently matched every row. Now:

```
view blog_index: filter "!drafts"
  unknown field `drafts` (did you mean `draft`?)
    known fields: body_bytes, date, day, description, draft, hidden, layout, ...
```

Type errors are caught the same way, with the fix in the message.

### Audited against `/code` and `/writing` (and the mindstorms gallery)

**Curated indexes are content, not views.** `code/index.md` is hand-authored, hand-ordered, with foreign keys reaching across tables. It must *stay* authored — a content-first system keeps "a human chose this list" distinct from "a query derived it", and the model already does: it is a `document`.

**The gallery is a restructure the tree already knows how to express.** Positionally restructured, a tree encodes it and the view is ordinary config:

```toml
[routes.mindstorms]
from     = "objects"
match    = "demos/mindstorms/**"
group_by = "dir"
variant  = "gallery"
path     = "/demos/mindstorms/{key}/"
```

**Three gaps this audit found** — objects need schema, scoping needs `match`, and `order_by` must dispatch to row ordering. All **built** 2026-07.

Still open: group `hero` (q23), and URL-parity for restructured trees (q28).

## 5a. Presentation, from first principles

> Superseded in the build by §5e, which carries the model as shipped. What stays here is the layer cut §5e rests on.

Four layers, each changing for its own reason and at its own rate:

| Layer | Owns | Changes when |
|---|---|---|
| **Schema** | what fields a row *has*, typed | the content model changes |
| **Rendering** | body markdown → semantic HTML fragment | an author writes |
| **Physical layout** | arranging rows + fields into `main` | the information architecture changes |
| **Visual theme** | the shell around `main` — chrome, `<head>`, CSS | the design changes |

### Layout kinds: there are three

Not "what this site has" — what a site of this shape *needs*:

| Kind | Input | What it was in Jekyll |
|---|---|---|
| **document** | one row, full content + relations | `post.html`, `page.html` |
| **listing** | N rows, summarised | `tag_index`, `monthly_archive`, `blog/index` |
| **feed** | N rows, serialised | `atom.xml`, `sitemap.xml` |
| **raw** | one row, content *is* `main` | the 6 pages using `layout: default` |

**Layout kind follows from what a row *is***: a post or page → `document`; a view with `group_by`/`paginate` → `listing`; a feed/sitemap view → `feed`; a row that opts out → `raw`.

### `<head>` is computed, then selected

The schema yields typed **head facts** — `title`, `description`, `canonical`, `robots`, `og`, `jsonld` — and each tier renders the subset it wants. A row with a `date` yields `og:type=article` + `BlogPosting`; one without yields `website`. That's a *fact about the row*, not a branch in a template, and it deletes all five of the old shell's if-chains.

### Theme is per row; layout kind is inferred

**Theme is chosen per row** (unusual, but it is what this site does): `theme:` in front matter or a rule default (§5b), rather than a site-wide setting. Per-row is the *mechanism*; it was never the whole answer. `[site] theme = "name[:tokens]"` is the bottom of the same cascade — front matter → rule default → site → the `default` directory → the base theme — so it adds a rung rather than a mechanism. A site-wide dark mode is one config line; a row that names its own theme states its own tokens. (The residue of that word on both rows and views is q33.)

**A view may name one too** *(built 2026-07-25)*. A route over a query had no way to say what it wore: an unclaimed listing took the theme its members *agreed* on (`unanimous_theme`), a claimed landing took its claimed row's, and both make the look a property of the CONTENT. The consequence is only visible when you want one query under two looks — the only way to get it was two copies of the rows, which is exactly what `theme-preview/` was doing with six.

`[routes.x] theme = "ledger:dark"` makes it a property of the route, and the cascade gains a rung at the top of the view side: **view → member unanimity (listings) or the claimed row (landings) → `[site] theme`**. The view wins because unanimity is an *inference* and a declaration is not. Tokens ride along exactly as a row's do, and an unknown name is a load error listing the knowns — checked against the theme registry, which is the only thing that knows what exists, and the same reason `[site] theme` is checked there.

What it does **not** solve is the reason the six copies exist: a post at `/vanilla/notes/one/` and the same post at `/ledger/notes/one/` are one row at two URLs, and a row has one route. That is the axis (q53), and this is the half of it that does not need one.

### Schema drives rendering, not just display

Per-collection fields fall into three kinds, and the distinction *is* the layer boundary:

| Kind | Read by | Example |
|---|---|---|
| **content field** | layout | `title`, `date`, `tags` |
| **render directive** | the renderer | `toc: true`, `style:` (§6c) |
| **layout hint** | the layout | `wide` |

One declaration then drives filters, `<head>` generation, layout requirements and validation — so "layout `document` requires `date`, but collection `pages` has no `date` field" becomes a load-time error like every other constraint (§4). §5b's `.schema.toml` is where that declaration ended up living.

### The renderer emits hooks, and that is not a layering violation

`{% image right foo.png %}` is the author saying "this floats right". The renderer emits `class="image image--right"`; the theme decides what that means. The rule is that a class is a **contract**, never a CSS implementation detail.

### What this cost: chrome parity

Redesigning layouts changes the chrome HTML, so `diff` cannot verify it. That was affordable and the budget was spent once, on §5e: **bodies verified by machine** (327/327 post content regions byte-identical across the cut), **chrome verified by eye**.

## 5b. Tree overlays: styles, slots and schema declared by position

> **The root `.style.scss` is built** *(2026-07-25)* — rung 1 of the ladder (themes/DESIGN.md §2), and the cheapest real customization there is. A `.style.scss` at the site root compiles into `@layer overlay`, above every theme's CSS, and is appended to **every** theme's stylesheet: the guarantee is that a knob set here survives a theme *switch*, not merely a theme update, which is what makes it a rung below "derive a theme" rather than a worse way to do it. Being unscoped, it may declare `:root` custom properties — precisely what a scoped one cannot (below), and precisely what recolouring needs.
>
> Two things learned building it, both from failing first. **Select on the slot, never the tree around it**: a theme wraps its parts in whatever it likes (ledger's title sits in a `header.doc-header`), so a site sheet reaching across six themes may assume the slot name — §5e's contract — and nothing else. A first draft used `> [data-slot="title"]` and matched in zero of six. And **`serve` has to watch it**: `is_content` excluded everything under `/grackle/` bar three exceptions, so the gallery's own sheet was invisible to the watcher and a site sheet you cannot iterate on is half a feature.
>
> **Status.** The **schema leg is built** (`schema.rs`): `.schema.toml` declares typed fields for its subtree, resolution accumulates nearest-wins like markers, and a governed row's extra front matter is *validated*. Per-row themes are built (`theme:` field cascading via rule defaults). The **`.style.scss` leg remains unbuilt**, and the `.slots/` leg was **absorbed by §5e**: a slot fill needs no templating.

**This is the marker pattern (§4b) again**, which is the argument for it: the tree declares *where*, the config declares only the vocabulary.

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

**Listings mix subtrees.** An unscoped `.style.scss` would bleed onto neighbouring posts in every listing. Every rendered row carries its **scope chain**, and styles compile inside it:

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

SCSS nesting does the scoping for free. `[data-scope~="…"]` matches whitespace-separated values, so a row carries every ancestor scope at once.

**Specificity**: an attribute selector is 0,1,0 — the same as a class — so source order decides. Emit **outermost first**; deeper subtrees then win naturally.

### "Only for posts?" — no, but the tree means the *source* tree

Scope is the **source path** of any rendered row, uniformly for posts and pages.

**For posts the source tree is not the URL tree.** `_posts/2022/foo.md` lives at `/blog/2022/12/16/foo/`. What it means is that per-subtree styles only get interesting for posts once posts are **page bundles** (§6a):

```
_posts/2022/coffee-part-1/
  index.md
  .style.scss     <- styles exactly this post
  leak.jpeg       <- and its assets resolve as siblings (§6a)
```

At which point **§6c's per-post `<style>` and this become the same feature**, with the bundle as the thing that unifies them.

### Schema per subtree, and the payoff

`code/.schema.toml` adds fields for rows beneath it, accumulating down the tree like markers:

```toml
github_link = { type = "url" }
```

One declaration then gives
- front matter validation (`github_lnk:` → error naming the file),
- filter type-checking (`'"x" in github_link'` → error: not a list),
- **slot template checking** — `{{ page.github_lnk }}` becomes a load-time error instead of rendering empty.

### Where the CSS goes

One rule: **shared → the shared file, unique → inline.** Subtree `.style.scss` → appended to `main.css`, scoped. Per-row `<style>` (§6c) → inline in `<head>`, scoped to that row.

### Constraints worth knowing before building this

1. **Scoped SCSS cannot declare `:root` custom properties** — they would be scoped to the selector and silently not apply. `@media` inside a scoped block is fine. This must be a documented constraint or a load-time error.
2. **Every rendered row must emit `data-scope`**, summaries included.
3. **Order must be deterministic**: outermost-first, then lexical.

## 5c. A view is a query; a route is where it lands

§5 declared views as generators: each one had a `path`, and routes were the only reason a view existed. The home page broke that, and the break was load-bearing.

### What `/` actually is

`index.html` has two lines of its own content — an `<h1>` and one paragraph. Everything else is three other things wearing a page costume:

| slot | filled by | kind |
|---|---|---|
| intro | authored prose | content |
| left | site data | site data, also used by the footer |
| right | latest 3 posts | a query |

Even the grid is not content: `.blocks-50 { display: grid; grid-template-columns: 1fr 1fr }` with two `.block-50` children is a **layout with two slots**, hand-written into a content file because Jekyll gave it nowhere else to live.

### The five-opinions problem

The reason to name a set is not to save a line of TOML. Five hand-written `{% unless %}` clauses had drifted into three different answers to "what is a post list?" — `blog_index` and `tag_index` excluded drafts, `monthly_archive` and `/` excluded hidden *and* drafts, and **the feed excluded only hidden, so it shipped drafts.** Nobody decided this; it accreted.

So: **one named set**, and everything composes over it.

```toml
[sets.published]          # query only: no route, no layout
from  = "posts"
where = "!draft && !hidden"

[routes.blog_index]  from = "published"  paginate = 5  paths = [...]
[sets.latest]        from = "published"  limit = 3
```

### Three shapes, one concept

| shape | route | layout | example |
|---|---|---|---|
| named query | — | — | `published` |
| embeddable | — | ✓ | `latest` |
| materialized | ✓ | ✓ | `blog_index` |

`path` is optional — its presence is what makes an entry a `[routes]` rather than a `[sets]`. `from` may name a collection, `*`, or **another query — but only a query-only one.** That restriction is the whole reason composition stays simple. Compose over things with nothing to inherit. Cycles, unknown names, and composing over a materialized view are all load-time errors.

### Members: the match this deleted

Each route carries `members`: the rows it materializes, decided once by the declared query. The renderer now iterates `members` and matches only on the *layout kind*: layout kinds are code, view names are the user's.

Routeless views have no route to hang `members` on, so their single row set lives in `db.views` — which also makes named queries introspectable via `export`, like every other table.

### Why compose over views, not routes

The tempting shape is `over = "/blog"`. It does not work: `/blog` is **66 routes**, so "the posts from /blog" is ambiguous — page 1's five, or the whole set? Routes are *outputs*. Querying one means `/` depends on `/blog` having been materialized, inverting the dependency graph §2's incremental rebuild rests on. Views are pure functions of tables; routes are results.

### The embedding seam

```
grackle.toml   [sets.latest] from="published" limit=3 layout="link_list"
  ↓ source/views.rs   routeless + ungrouped → one row set → db.views["latest"]
  ↓ tags.rs    {% view latest %} → look up rows, dispatch on layout
  ↓ render.rs  link_list(rows, site)
```

Nothing in `tags.rs` or `render.rs` knows what "latest" means; `render::link_list` takes rows and a site and cannot reach the database.

Two deliberate refusals, both the same line drawn in §6d against exposing blocks to templates:
* `{% include %}` **rejects parameters.**
* `{% view %}` **dispatches to a layout kind** rather than handing rows to a template to iterate.

### What it cost

`{% view %}` is not Liquid, so Jekyll can no longer build `index.html`. The reference build cannot be regenerated while `{% view %}` stands.

### Grouping is one operation *(generalized 2026-07)*

`group_keys` had three hardcoded specs (`tags`, `date.year`, `date.month`); they were one operation — **group by a typed schema field**. A `List` field multi-keys (one group per item), scalars single-key, `Null` means absent from the partition. The date specs survive as aliases for the `year`/`month` fields the filter schema always had. Proven the strong way: the main site's three groupings are **byte-identical through the general path**.

### One materializer: grouping and pagination over every base *(built 2026-07-25)*

Grouping was one operation over *some* bases. `build_object_view` was a second materializer, and it refused `group_by` and `paginate` with a message that admitted the shape of the problem — *"not supported yet"*. It differed from the row path in exactly three things, and every one is a **parameter**, not control flow:

| | row bases | objects |
|---|---|---|
| expression vocabulary | `row_filter_schema()` — built-ins plus declared fields | `object_schema()` — narrow, no front matter |
| membership | `base_filter(kind)`: OR over every collection of that kind | `collection == <this one>` |
| eligibility | `rendered && !claimed && locale` | none — an object is bytes |

So they became one `build_view` taking a `Base { schema, membership, parsed }`, and grouping, pagination, subdivision and routeless embedding stopped being row privileges. *All the jpegs at one route, the pngs at another* is now `group_by = "ext"`, because `ext` was always a column of the narrow vocabulary.

**The tell that this was a merge and not a feature**: `check_group_chain` already carried a `Kind::Objects` arm validating group specs against `object_schema()`, reachable only from a branch the dispatch never sent objects to. Grouping validation for objects had been written and could not run. It runs now, and the narrow vocabulary it guards is preserved — `where = "draft"` on a gallery is still a load error naming the object columns, which is the strictness §3 chose on purpose.

**And `paginate` under `group_by` was silently ignored.** The grouped branch returned before the paginated one was read, so a grouped view that asked to paginate got one route per group and no complaint. A grouped view now paginates **inside each partition**: the partition says which rows, pagination says how many to a page. This is not q30 — that is pagination × *subdivision*, where a pageable parent and its subdivided children share a URL namespace, and `config.query` still refuses to compose over a paginated route, so a grouped-and-paginated view stays a leaf.

Two defects surfaced downstream, both visible only to a route that is grouped *and* paginated, and both fixed by deleting a re-render. `pagination_parts` honoured q32 ("page URLs come from the owning view, not a literal copy in the producer") by re-rendering the view's route templates with `{n}` — which cannot fill `{key}`, and whose page count was taken over *every* page of the view rather than the group's, so a three-page partition would have offered three pages to every group beside it. It reads the view's already-materialized sibling pages instead: same rule, one fewer way to be wrong, and a materialized URL already wears its group key, its record slug (`{key}` is slugged in the URL and not in the params, so the two could disagree) and its locale prefix.

One new load error, because the silence it replaces had two shapes: a paginated view declaring a single `path` used to either collide page 2 onto page 1's URL or — with `path` rather than `paths` — emit **no routes at all**, a view that asked to paginate and produced nothing.

Measured: grack.com, field-notes, minimal, raw and theme-preview all render **byte-identical** across the change, the feed's wall-clock `<updated>` excepted. Two fixtures hold the new capability (`object-grouping`, `paginate-one-path`), each mutation-checked when written — per-group pagination fails when the group comparison is dropped, and the page-2 route disappears when `paginate` is un-read.

### `from` scopes to what it names, and unions are written out *(built 2026-07-25)*

The other half of the merge above, and a behaviour change rather than a deletion. The two materializers had disagreed about what `from = "<a collection>"` meant:

| base | ranged over |
|---|---|
| an objects collection | **that** collection |
| a posts or tree collection | **every** collection of that kind |

So `from = "notes"` meant the whole posts table, and §4's "several collections, one table" — `_posts` and `_drafts` as two sources of one corpus — was a thing the *engine* kept and the config could not say. Now `from` scopes to the collections it names, for every base, and the union is spelled:

```toml
[sets.published]
from  = ["posts", "drafts"]     # two sources, one corpus, said out loud
where = "!draft && !hidden"
```

Membership stops being a per-kind rule and becomes one clause — *the row's collection is one of these* — which is what collapses the last of the objects/rows asymmetry: what remains is the vocabulary and whether the rows are parsed.

A union may name only **collections**, and they must **share a kind**. Unioning two sets is a general query operation this does not attempt (the error says to compose over the set with a `where` instead), and unioning across kinds would ask one `where` to type-check against two vocabularies and one materializer to decide whether the rows are parsed. Both are load errors naming the members.

**What caught the change is worth recording, because it is the failure this spelling exists to prevent.** The `crumb-trails` fixture has two posts collections and a `published` set that named one of them; under scoping its `_drafts` rows silently left every listing. On grack.com the same defect was invisible in the default projection — the `!draft` predicate already excluded those rows — and appeared only under `--profile drafts`, which relaxes the predicate to surface them: 561 pages became 560 and every paginated listing shifted. A URL-set check could not see it, because a draft is routed either way; only a full render under the second profile could. **Parity has two profiles on this site, and one of them is the only place the interesting rows exist.**

`theme-preview` was the beneficiary. Its six per-theme sets each restated `collection == "…"` beside their own `from`, which §4d had read as redundancy and was not — the restatement was the only thing scoping a set to its own theme. They are now one union naming the corpus and six two-line sets narrowing it by `match`, which conjoins along `from`: one declaration of the predicate, the sort and the summary truncation where there were six.

### Subdivision: `from` a grouped route refines its partition *(built 2026-07)*

A grouped view is a partition of its base; a grouped view **`from` a grouped view is a finer partition of the parent's groups** — GROUP BY year, month, expressed compositionally:

```toml
[routes.yearly_archive]
from     = "published"
group_by = "date.year"
path     = "/blog/{year}/"

[routes.monthly_archive]
from     = "yearly_archive"          # subdivision: year key comes from here
group_by = "date.month"
path     = "/blog/{year}/{month:02}/"
```

Three consequences:

1. **Group keys accumulate down the chain.** Composite membership is provably identical to flat grouping.
2. **Provenance is structural, not declared.** The month group has the year group as its parent *because that is how the query nests*. Breadcrumbs are a provenance walk: collection → year → month → row.
3. **Naming is config, not code.** Views declare `title` and `crumb` as templates over their group params.

**URLs are derived values, all the way down** (q32): producers take URLs and never construct them. Pagination links render from the owning view's own `paths` templates; tag pills render from the tags-owning view's template; slugs apply at exactly one seam per base kind. The collection's own `crumb`/`index` fields are the last non-derived names in a trail (q46 proposes dissolving them into §5h's landing chain).

Composition rules, enforced at load: `from` may name a set or a **grouped, unpaginated** view — and the composer must then be grouped itself. **Pagination × subdivision is deliberately punted** (q30).

### The split the section title always implied *(built 2026-07-19)*

"A view is a query; a route is where it lands" was a sentence in this document and one `[views]` section in config, where the only way to tell the two apart was whether `route` happened to be present. It is now the shape: **`[sets]`** for a query that never lands, **`[routes]`** for one that does.

Measured across both sites' 23 queries before deciding: `path(s)`, `title`, `crumb`, `shell`, `template`, `content`, `intro`, `featured`, `paginate` and `group_by` NEVER appear without a route; `from`, `where`, `match`, `order_by`, `limit`, `layout` and `variant` appear in both. Ten keys are meaningless without a URL.

**One keyword, not two.** `from` names a collection, a set or a route, and what it names decides what it means.

**One namespace, now enforced** — which exposed a latent collision: the resolver tried views *before* collections with no guard, so `[views.blog]` beside `[collections.blog]` silently shadowed the collection. A name now lives in exactly one of the three.

**Profiles split the same way**, and say more for it: relaxing `[profiles.drafts.sets.published]` patches a QUERY, relaxing `[profiles.drafts.routes.search]` patches a LANDING.
## 5d. Templating: there is almost none, so don't build for it

Of ~60 liquid constructs across the port, only three were genuinely display iteration: breadcrumbs, tag pills, pagination nav — all components. The rest were queries, schema facts, or argument passing that Liquid was merely the available vocabulary for.

### The rule

> **A template may not contain control flow.**
> Needs a loop → it is a view. Needs a conditional → it is a schema fact, or a different layout kind.

This is a **tripwire**, not an aesthetic. Every `{% if %}` you want is a missing schema field; every `{% for %}` is an unnamed query.

It preserves discipline: `filter.rs` is typed with load-time checking and "did you mean" suggestions. A template language throws that away — untyped, runtime-resolved, `{{ post.titel }}` silently rendering nothing. The ethos is load-time errors, not 404s; Liquid is the opposite by construction.

`/` was the existence proof: HTML, typed holes, **zero control flow**, matching the reference exactly. Its nine-line counter loop became `where` + `limit`.

**So `liquid` was retired by never being taken** — §9a had listed it as the biggest dependency risk. `tags.rs` is a targeted expander, not a liquid implementation:

| construct | uses at the port | note |
|---|---|---|
| `{% image %}` | 194 | §6a |
| `{{ site.baseurl }}` and its `prepend:` form | 12 | whole shapes, not a filter pipeline |
| `{% view %}` | 1 | §5c |
| `{% include %}` | 1 | parameterless only |

Anything unrecognised is emitted **verbatim**, so an unimplemented construct appears in the output rather than evaluating to nothing.

### Custom widgets: named HTML expansions with a markdown body *(built 2026-07)*

**Built as specced**: a `[widgets]` registry in `grackle.toml` (`name → wrapper template` with a `{body}` hole, validated at load), paired-tag expansion in `tags.rs`. A widget is the block-level sibling of `{% image %}`: a named expansion, not control flow, so it stays inside the rule above and needs no template engine.

**The rule the registry enforces:** still no arguments, still no control flow. An argumentful or conditional widget is the tripwire that says "you want a template — you don't."

**The rule's two former weaknesses are both closed.** Pagination was the best stress test — a genuine range loop plus a three-way conditional — and fell out as ~40 lines of Rust, semantically identical to the reference nav. And "themes are Rust" was dissolved by §5e: themes became directories of data.

## 5e. The presentation synthesis: parts fill slots, CSS does the geometry

**Status: built, and the synthesis is real.** Layout kinds emit part maps: named, typed parts in canonical order. The fragment binder is a strict parser plus the hole algebra plus complete load-time validation. `parts::canonical()` renders any part map with no fragments at all, so **themes are partial by construction**: a theme with no fragments IS the null theme, and a new theme can start from one fragment and grow.

### The model

Layout kinds emit a part map — named, typed parts, each a flat piece of semantic HTML or a typed scalar. For a `document`, this includes title, url, crumbs, date, tags, content, notes, neighbors, truncated. `listing` adds items (a stream of `summary` part maps) and pagination. `feed` and `raw` are handled specially.

**A theme is a directory of data, not code:**

```
themes/default/
  theme.toml
  shell.html        # the outer skeleton: holes for header/main/footer
  document.html     # optional: per-kind arrangement fragments
  summary.html      # optional: how one listing item is arranged
  theme.scss
```

**A fragment is straight-line HTML with holes.** The hole algebra is four rules:

1. **A hole is `data-slot="name"`.** The element's content is replaced by the part. Scalar parts are escaped text; fragment parts are trusted HTML.
2. **An empty part deletes its element.** This one rule replaces every presence-conditional. `<footer data-slot="footer">` with nothing to say does not render a footer.
3. **A stream maps a fragment over its items.** `<div data-slot="items">` renders the fragment once per row. The loop lives in the engine; the fragment stays straight-line.
4. **An attribute hole is `data-slot-attr="name"`** — `<a data-slot-href="url">` sets `href` from a text part, absent part omits the attribute. HTML's own semantics absorb the variants: a placeholder link is the spec's inert `<a>` without `href`.

**Every name is load-time checked** against the part schema of the kind — unknown slot, unfilled required slot, unknown fragment: errors naming the file, exactly like the filter language. `{{ post.titel }}`-class bugs die the same death twice.

### CSS does the geometry

Layout kinds emit parts in **canonical semantic order** — reading order, the order a screen reader sees. Slot names are the styling contract: `[data-slot=…]`, `[data-kind=…]`, `data-<fact>`. The renderer's classes are API, not implementation.

Themes place markup with `grid-template-areas` keyed on slot names, not by reordering HTML:

```css
[data-kind="document"] {
  display: grid;
  grid-template-areas: "crumbs content" "tags content" ". neighbors";
}
```

One `document` kind, one markup, and "post vs page" is two grid declarations the theme owns.

**The styling contract changes from classes to structure.** Emitted markup carries semantic elements + `data-slot` + schema facts as data attributes (`data-kind`, `data-tree`, `data-truncated`). Slot names appear in the part schema, the theme fragment, the CSS selector, and the tree overlay filename — one vocabulary, checkable end to end.

### The modern CSS baseline

The theme contract assumes modern CSS: nesting, `:has()`, container queries, `@layer`, subgrid, `aspect-ratio` (Baseline as of ~2023). This is a declared floor: each feature retires machinery the contract would otherwise carry.

| feature | what it retires |
|---|---|
| `@layer` | specificity management by convention |
| nesting | BEM's flattened strings |
| container queries | context classes |
| `:has()` | upward-stamped helper classes |
| `aspect-ratio` | client-side measurement and layout shift |

BEM's justifications — specificity wars, no scoping, decoupled selectors — are each answered by the platform now.

**Theme CSS is checkable.** Fragments are parsed at load, so the engine can verify that every `[data-slot=…]` selector in a theme's CSS names a real slot of the kind the fragment binds.

### The archetype test: any layout is theme CSS plus a fragment choice

The model's bet is that *all geometry* lives in theme CSS, so "can it do layout X" decomposes into "can modern CSS express X" (a browser question) and "does the part schema carry what X's CSS needs" (the engine's only obligation).

Auditing archetypes — document with margin or sidenotes, album gallery, Pinterest masonry, magazine full-bleed, timeline, dense index — surfaced **four genuine gaps, and every one resolved to "add a part or fact", never to control flow.** Three are built:

- **The `hero` part** (q23) — sourced from the image-typed schema field named `cover`, with first-image-block fallback and group hero.
- **Per-view fragment variants** (q24) — variant fragments per view.
- **Dimension facts on images** (q26) — **closed 2026-07-21.** Body images emit `width`/`height` at expansion.

The one still open is **per-block facts** (q25): full-bleed needs one block to escape the content column — a block-level directive becoming a `data-` attribute on that block.

**The one honest limit is masonry.** True Pinterest packing with strict reading order cannot yet be fully expressed in CSS — native masonry is still settling in the working group. Interim: CSS `columns` or row-span tricks fed by the dimension facts. When native masonry lands it is one declaration in one theme file, zero engine work.

**§5e turns "can we do layout X" from an engine question into a browser question, and the engine's obligation becomes crisp — every part or fact a plausible theme could need must be in the schema.**

### Variants and the one preview kind *(q24 + q36, built 2026-07)*

Two settlements: **one preview kind** (q36), and **fragment variants** (q24).

**One preview kind**: a card is a view's projection of a row, and so is a summary — they differ by what the row HAS, not by what they are. `summary` is the one kind, presence-driven; `card`/`card_list` are deleted, `figure` and `gallery` folded the same way. `LAYOUTS` is down to `listing`/`link_list`/`card`, `passes/` to one file, and six listing producers to two.

**Fields reach display**: a declared part the engine has no producer for is filled from the row's `.schema.toml` field of the same name — `score = { type = "int" }` + `[[parts]]` + `data-slot="score"` renders the number. A `bool` lands as a FACT; a `list` fills a stream of text parts. A type that cannot fill its part is a load error.

**An image field is a reference**: its value names a ROW — an objects collection already claimed that file. It is checked at load like a prose link, a dangling `cover:` erroring by name. Pixel dimensions are header-read at load onto the object row, so a view can select on them.

**Fragment variants**: a fragment file's stem before `--` is its kind (`summary--card.html` binds `summary`). A view declares `variant = "cards"`; rendering tries `{kind}--{variant}`, falls back to base, then canonical — partial themes throughout. `data-fragment` selects a variant for stream/map children and must resolve at load to the right kind.

### The precedence law, stated once

The same resolution order governs rules (§4), markers (§4b), buckets (§6a), and slots:

> **Nearest wins; first writer per key.**
> front matter > tree overlay (`.slots/`, §5b) > layout kind > theme default.

### Tree-filled slots: `.slots/` is a table *(settled 2026-07)*

A directory may carry a `.slots/` subdirectory; each file fills one slot for every row beneath — **filename = slot name = key, content = fill**. Resolution is positional — nearest `.slots/<name>.*` up the source path wins.

**Extension picks the pipeline.** `.md` renders to an `Html` part; `.html` is a binder fragment with holes validated at load.

**The block-arity rule** checks fills against the content model of the slot element at load: a fill in a phrasing element must be exactly one block (unwrapped); a flow element takes any number of blocks verbatim.

### The base theme is in the binary *(built 2026-07-24)*

The null theme was complete and unusable — `canonical()` has no way to know which sibling a `url` part is the address *of*, so it renders one as `<a href="/x/">/x/</a>`. Every kind that pairs a label with a link needs a fragment saying "this text, that href", and there is exactly one sensible way to write each.

The base moved into the engine — embedded with `include_str!`, and inherited by every theme. A site with no `themes/` directory renders semantic HTML with a stylesheet.

Three lines had to be drawn:

- **The base is structure, never decoration.** A rule belongs there if a theme would have to re-derive it (the measure, a nav that is a row not a bulleted list, the reset). It does not if a theme would have to *undo* it.
- **Ship a shell, own the frame.** The base's page geometry keys on `[data-frame]`, stamped by its own `shell.html`, so a theme writing its own shell inherits none of it. A fixed rail or full-bleed bar must be its own frame.
- **An arrangement may decline a part; `canonical()` may not.** Completeness is the *parts layer's* obligation, not the theme's — `terminal` drops tags from its summary on purpose. The base's own exemptions are declared with reasons.

### Tripwires

- A layout kind wants to emit a wrapper div → that div belongs in a theme fragment.
- A rule in the base makes a theme write an override → it was decoration, not structure; move it to `themes/vanilla/`.
- A theme fragment wants a conditional → there is a missing fact; empty-collapses cover presence, facts-as-attributes cover variants.
- The binder grows an expression syntax → stop; it is becoming a template language.
- A slot name appears in CSS but not the part schema → the load-time check should already have caught it; if it didn't, the check is broken.
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
| relation `where =` (§6g) | `bool` over the two-row environment | built — §6g slice 1 |
| relation `rank =` (§6g) | `double`, bigger wins | built — §6g slice 2; forced arithmetic, unary minus, the `Double` type and the two-row registry |

Route/`title`/`crumb`/`content` templates stay the `{token}` placeholder
language: string interpolation over the route's dimensions, not computation.
Folding them in would put logic where §5d forbids it. A token may name a
namespace — `{axis:theme}`, `{group:year}` — for the case where a path spends
both an axis and a group key of the same field name; a bare `{year}` resolves
in whichever single namespace has it, and is a load error where both do. The
pad shares the punctuation without a second convention: a **trailing** `:<digits>`
is the zero-pad (`{month:02}`, `{group:month:02}`), a `:` before non-digits the
namespace separator.

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
  function; expressions stay row-local. One sanctioned, bounded exception:
  relation expressions (§6g) bind exactly two rows — `self` and
  `candidate` — plus finished relation lists as names. Anything reaching
  for arbitrary rows still trips.
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

The engine owns `root_shell`: doctype, `<html lang data-kind="shell" [data-subtheme]>`, `<head>` from computed facts, `<body>` around theme chrome. A theme's `shell.html` is now **body chrome only** — no theme writes a skeleton. A fragmentless theme yields a valid document; `light` dissolved into a minimal head option inside the same root shell as everything else; `subtheme` moved to the engine root (no per-theme opt-in).

Pending: a theme wanting to add head content (fonts) needs an optional `head.html` theme fragment appended after computed facts.

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
contribute their rendered body. Other route kinds are silently unsearchable. The example searches notes AND recipes/books/manual (18 docs, 5 KB); the main site declares `kind == "post"` and its index is **byte-identical** through the view path. The js/wasm consumers are emitted only when a search view exists; a site without one ships zero search bytes.

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
build — script shells are for cheap serializations, not compilers. **Gotcha with a real scar**: a script shell's source is a file in your
tree, so it will be routed and *published* unless excluded. Add `shells/**` to `exclude`; the `/llms.txt` route still builds, because the command comes from config, not from a content row.

### The md shell

Specced; not yet built. A markdown serialization of part maps; forcing consumer is `/llms.txt` (titles, URLs, summaries as markdown listing).

### Row shells: a row picks its own wrapper *(q44, built 2026-07-19)*

A row declares `shell:` and picks its own wrapper: **`none`** (body IS output — no skeleton, no theme), **`light`** (engine skeleton, canonical parts, no theme chrome) or **`html`** (the theme). Closed vocabulary, checked at load.

`none` adds a capability: an imported artifact can now carry front matter *and* emit itself verbatim. Before, front matter nested the whole document inside a second `<html>`. The example's `demos/pane.html` is the occupant — 521 bytes of its own document with a `title` the database sees. Pair with `hidden: true` to keep it linkable but out of the sitemap.

### Row tiers: where a row leaves the pipeline *(settled 2026-07-19)*

The tiers are not alternatives to something else — they are **exit points on
one pipeline**.

| tier | head | body | skeleton |
|---|---|---|---|
| object | — | bytes off disk | none |
| `none` | — | rendered parts, emitted verbatim | none |
| `light` | minimal — 85 B | canonical parts, no theme | engine |
| `html` | full — 739 B | theme fragments | engine |

**"Aren't `shell: none` rows just objects?"** They emit verbatim (last step only they share), but enter the full pipeline: tag expansion, object resolution, thumbnailing, content-addressed assets — all run with load-time enforcement. Objects never enter; their bytes come off disk by extension. **"Isn't `shell: light` just `theme: light`?"** No: a theme chooses body chrome; the head is computed from schema and no theme may write it. The root shell enforces this separation. `theme: none` fails because **the null theme still emits a valid document**, so `shell: none` (promising the body already is one) cannot be silent about the head.

**One correction:** `light` is not "the null theme" — it is a **tier** bypassing the theme registry; there is no `themes/light/` directory.

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

**The guarantee ladder**: each tier is what the engine *promises* about bytes — **object** (nothing, yours); **`none`** (content rules ran, validity is your promise); **`light`** (valid document, minimal facts); **`html`** (valid document, full computed head, theme). A theme cannot lower a guarantee it did not make.

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

"The page that stands for a set" had four separate implementations — view roots, index pages, the home page, and `collection.index` — each answering "what's above me" differently.

### The rule: the engine never guesses the arrangement

Either the theme owns the arrangement, or the author does. Three tiers:

- **Bare**: query + route + `listing`. 
- **Declared text**: the view declares `intro` — a `LocalizedStr` rendered as markdown through the locale-aware link resolver (a `view:` link in an intro gets strict validation; no browser-agreement bypass). Fills an `intro` slot on the listing layout. Empty collapses.
- **Referenced content**: the view declares `content = "path.md"` (mutually exclusive with `intro`). The row becomes the body and **must place `{% view <owner> %}` itself** — a load error otherwise. The self-embed is **route-aware**: page 2 renders page 2's rows, `/fr/recipes/` the French partition. **`content` may be a TEMPLATE** *(per-group content, built 2026-07)* — `content = "{group:key}/index.md"` resolves per route through the same `{token}` language `path`/`title` use, so a grouped view gives each of its N routes its own words (`/alpha/` embeds `alpha/index.md`, `/beta/` embeds `beta/index.md`). It resolves over the route's group params (`group:`/bare) and axis members (`axis:`). A templated `content` stays a **promise** (a group whose file is missing is a load error); a templated `default_content` stays the **offer** (missing, or a row that does not place the embed, leaves that route a plain listing — decided per route, not per view).

### Claiming

A referenced content row is **claimed**: no standalone route, and out of every query **structurally** — by ownership, not a naming convention. The row keeps everything rows have: front matter (its `title:` beats the view's), its rule-derived theme, its directory (slot fills resolve nearest-wins from there), suffix localization with default-locale fallback.

Claiming is **declared, never discovered** — a convention would claim silently — which makes migration incremental: unclaimed index pages behave exactly as before. Load checks: the path names a row, one owner per row, intro XOR content, materialized views only, must-place. **Claimed rows leave the backlink scan** — membership is not citation.

A **literal** claim is settled at load (the row is marked, its own route withheld, it is excluded from every query before views materialize). A **templated** claim cannot be — the group keys are not known until the routes exist — so it is settled in a pass that runs after materialization but before the collision check: it resolves each route's `content`, marks the rows, drops their own routes and their query membership, and records `Route.content` so the landing pass finds the per-route body. That ordering is what lets a group landing at `/alpha/` and the `alpha/index.md` row's own `/alpha/` route coexist through load and then resolve to one — the claim removes the second before the collision check ever sees it.

### The chain: URL nesting is parent derivation

`ancestors()` answers "what's above this URL": a rendered **page row** at the parent URL (mode-B landings match here, row title winning), else a **materialized landing route** (the view's crumb-else-title at the locale). Locale-prefix homes are skipped (`/fr/` is not a directory). **Listing trails climb the same chain**, so `Home › Recipes › Dinner` fell out of moving course archives under `/recipes/`, with zero source edits.

**Materialized landing route** is tested as `params` empty, page ≤ 1 — a q46 correction finding that the climb was broken by paginated view synthetic keys. **Theme rides the same logic one level up**: a tree-backed listing whose members wear one theme *name* wears it too (subtheme tokens are one row's dress and never lift).

### The collection stops naming itself *(q46, settled and built 2026-07)*

`collection.crumb`/`index` are **gone**. They stated in the collection what the collection's landing view already declares, duplicating a single fact. The dissolution is the climb doing its job: `trail_root` is now Home and nothing else; every crumb between Home and the current page comes from the climb.

### Honest edges, pending

- An explicit `parent =` for when URL nesting lies: unneeded so far.
- Orphaned translations should warn.
- Mode-B prose is not searchable (landing routes structurally excluded); keep until someone misses it.
- A variant fragment lacking a hole drops that part **silently** — wants a load-time warning.
- Home and the manual haven't lifted yet — home is the queryless
  landing (`route = "/"`, `content = "index.html"`, no rows to strand;
  q37's board hangs in this frame), the manual waits for the section
  tree to be a landing's listing. The example search's one remaining
  `stem != "index"` filter survives exactly until they do.
## 6a. Object references: paths and names

### The measurements that shape this

- Every existing reference is a **root-relative path**: `{% image assets/2022/12/part-2-disassembly-a.jpeg %}`.
- **Posts keep assets in a bucket**: posts at `_posts/2022/`, images at `assets/2022/12/` — disjoint trees.
- **Tree pages already use side-by-side assets**: `code/legacy/romtool/` holds `index.html` *and* `screen1.png`.
- **6 basenames look ambiguous site-wide — 4 dissolve under bubble+bucket**: nearest-wins bubbling and bucket scoping resolve nearly all collisions; only `screenshot5.png`/`screenshot6.png` remain genuine (both in `assets/`, different years).

The two-phase rule isn't a compromise — it's the shape the content is already in.

### The rule

A reference is a **path** if it contains `/` or `://`; otherwise it's a **name**.

**Paths** resolve from the site root, unchanged. All 194 existing invocations take this branch → parity preserved.

**Names** bubble up from the referencing row's directory to the site root:

1. **Siblings** — direct children of this level. Hit → done.
2. **This level's bucket** — if this level contains a directory matching the configured asset pattern (`assets/`), scan it as a subtree. Hit → done.
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

**Buckets are positional, not configured.** There is no list of bucket paths to maintain, because the tree encodes both:

- `_posts/assets/` is a **posts-only** bucket — solely because only posts bubble through `_posts/`.
- root `assets/` is the **global** bucket — solely because everything eventually reaches root.
- A nearer bucket beats a farther one automatically.

Add `_posts/2022/assets/` tomorrow and it starts winning for 2022 posts, with no config change.

Worked examples against the real tree:

| Reference | Walk | Result |
|---|---|---|
| post `_posts/2022/…coffee.md` → `part-1-leak.jpeg` | `_posts/2022/`: no siblings, no `assets/` → `_posts/`: same → **root**: root has `assets/` → scan subtree | `assets/2022/12/part-1-leak.jpeg` |
| page `code/legacy/romtool/index.html` → `screen1.png` | level 1: **sibling hit** | `code/legacy/romtool/screen1.png` |
| post `_posts/2003/…md` → `a.png` | bubbles to root → `assets/` subtree | `assets/2003/06/a.png` |

**Ambiguity is per-step.** 2+ hits within one level → error listing candidates. Across levels there is no ambiguity by construction: nearer wins.

⚠️ **Specced, not built** *(measured 2026-07-21)*. This describes a design; the code does not implement name bubbling. `thumbs::one` joins the filename to the site root; a bare name resolves to `root/burrs.jpg`, misses, and fails with `{% image %} source not found`. `[objects] bucket` is parsed but read by nothing; `by_name` is built every load but read only by `query stats`. All 194 corpus invocations pass a path, so the unbuilt branch has never been reached.

An earlier version of this section claimed "bare names work for posts today, with no restructuring and no bucket configuration at all". They do not. This is the same class of drift as §9b Round 3's *declared-and-ignored* `layout` names — a third instance found in one week, after `grackle diff`'s URL-parity claim and the heading anchors story. The tour's own worked example (§0 step 4, `burrs.jpg` resolving to a sibling) is aspirational for the same reason.

### `{% image %}` vs `<img>`/`<iframe>` (and `<style>`)

Do **both**, but let the reference form decide:

- `{% image %}` stays — 194 uses need it, and it carries the `left`/`right`/`inline` mode that markdown image syntax can't express.
- A **post-render `lol_html` pass** rewrites `<img src>` and `<iframe src>` **only when the src is a bare name**. Anything containing `/` or `://` passes through untouched.
- This makes plain markdown `![alt](foo.png)` work with no new tag, and gives `<iframe src="demo.html">` the same treatment.
- The same pass is where `feed_images` already lives and where `<style>` extraction happens (§6c).

### Row links and view links *(built 2026-07)*

URLs are DERIVED values here. Matt's rule closes the gap: **authored links reference what the database owns.**

1. **A link to a row references its source file** — relative (`carbonara.md`) or root-relative (`/recipes/carbonara.md`) — and the engine renders the URL. An unknown source is a build error.
2. **A link to a view uses `view:` syntax** — `view:gallery`, `view:recipes_by_course/dinner` — rendered through the owning view's route template, locale-aware, and verified against the route set.
3. **`[links] policy`** grades enforcement. `strict` — **the default since 2026-07-20** — errors on raw internal URLs, answering with the correct form. `loose` resolves the new forms but leaves both alone, for importing unconverted corpus.

**What flipping to strict caught, which is the argument for it.** Two engine gaps: **a link to a directory means its index** (35 good links were dangling because the resolver didn't know it), and **`javascript:` is code, not a path**. Two real defects: `/blog` without a trailing slash was not canonical, and `/demos/dress` and `/demos/adventure` did not resolve — **25 pages gained a `Linked from` section** that had been silently dropped.

✅ **Raw HTML joined the net, 2026-07-21** (§6d stage B). `.html` page bodies, `.html` slot fills and raw-HTML landings never met comrak; a narrow `lol_html` pass now walks them, catching the `/blog` above in `index.html` and `_includes/social.html` and closing the example's `index.fr.html` residual.

Resolution is a comrak AST pass over Link nodes per-row, against a `LinkSpace` built once per build. The byte-oracle rule: **the engine rewrites only where the browser would get it wrong** — a relative link whose source-resolution and URL-resolution agree ships byte-identical; `.md` references and cross-dir links get the engine's answer.

**`.slots/` fills render THROUGH the resolver, per consuming page**: one `nav.md` of `view:`/source links serves every locale — `view:blog_index` is `/blog/` on English and `/fr/blog/` on French — and `nav.fr.md` exists only to translate labels.

Pending: the closest-match suggester is stem-exact, not fuzzy.

## 6b. `_cache/`: one content-addressed store for every derived artifact

Everything expensive is a pure function of bytes → cache it by the hash of those bytes. Keys are content hashes, so entries are **self-invalidating and never stale**: a changed input is simply a different key.

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

Two distinct things:

- **`_cache/` is the build cache** — gitignored, never published, keyed by content, always safe to delete.
- **`static.dir` is the published location** — where derived assets get URLs (`/static/{hash}{ext}`).

**Cache is keyed by content, not by path**, which means a renamed post keeps its embedding, a moved image keeps its thumbnail, and the drafts profile shares every entry with the public profile.

### What moving to `/static/` buys (now that image URLs are free to change)

Derived assets are exempt from URL parity (q12), and the `/static/{hash}` scheme answers three real constraints:

- **Extensions on URLs.** Extensionless blobs require browser sniffing; `/static/{hash}.webp` is self-describing.
- **Immutability by construction.** Every URL is a content hash, so `Cache-Control: public, max-age=31536000, immutable` is correct — no query-string cache-busters, no config edits needed.
- **WebP becomes available.** The variant contest picks the smallest of {original, PNG, JPEG}; adding WebP is one more encoder.

### Embeddings (this retires LSI) *(built 2026-07)*

**Built**, with implementation details (stale-while-revalidate, vector hashing, L2-normalization, brute-force ranking) proven in the serve log.

Vectors cached content-addressed (`_cache/embeddings/{hash}.vec`, 1.5 KB each), model beside them (`_cache/models/`, downloaded once). A post never matches itself — its own vector is the perfect cosine, pinned by a test rather than left incidental. Measured: warm build 1.5s total.

**"Related" is RELATIONS, not a list.** A post relates to others along multiple axes — embedding similarity, earlier, later — and pivots along each. The part model carries `relations: Stream("relation")`, each relation = `{axis, label, items: Stream("neighbor")}`. The axis rides into markup as `data-axis` for per-axis styling; an axis with nothing to say contributes no group. *(Since the q53 split these groups are RELATIONS, declared per collection — §6g, where `data-axis` is renamed `data-relation`.)*

This retired **Jekyll's LSI**: a dominant chunk of the 90-second build, recomputed from scratch every time with no content-addressed cache. LSI's related-posts were mediocre and `diff` cannot judge relatedness, so embeddings are deliberately *better* than equivalent, at no parity cost.

⚠️ **The embedded body is RAW MARKDOWN, so link syntax is semantic signal.** Rewriting 56 links to file-relative form changed no rendered href at all, but reshuffled `Related` on **37 of 327 posts**, because `{{ site.baseurl }}{% post_url … %}` and `../2010/….md` are different text. Related-posts are a function of markdown *syntax*, not of prose. Until the fix (embed rendered plain text) lands, do not read "Related changed" as evidence that a refactor changed meaning.

### TF-IDF search index — the searcher is the same code, compiled to wasm *(built 2026-07)*

**The architecture:** the search core is **one crate** (`search-core`: stem, tokenize, index build, rank) used by both ends. `grackle build` calls it to ship `/search.bin` (postcard, private format), and the identical code compiles to WebAssembly (`search-wasm`, ~90 KB cdylib: `alloc`/`init`/`search`). **Symmetry by construction**: the browser stems queries with the same compiled function that stemmed the corpus, and the stemmer is free to stay simple because it cannot desynchronize.

The page ships an icon and nothing else. Clicking it injects `/search.js` (3.6 KB loader), which fetches the blob and index and answers per keystroke. The **last query token is a live prefix** over the sorted term map ("bluet" finds bluetooth).

The wasm blob and its loader are **engine assets** (embedded via `include_bytes!`, emitted when a site declares a search view) — they must version with `/search.bin`'s format, so they cannot be theme-committed; a theme owns only the trigger and the overlay CSS. The index itself is a declared SHELL (`shell = "search"`, §5g), so the searchable set is a query over the route schema, spanning tables.

Embeddings answer *"what is this like"* (fuzzy, build-time); TF-IDF answers *"where does this word appear"* (exact, shippable, no model at runtime). Two tools, one cache discipline.

This retired **Swiftype**: the header search used to be `javascript:document.getElementById('st-launcher-tab').click()`, a launcher for a third-party service, which is also why the layout carried `data-swiftype-index` attributes and a `<meta class="swiftype">` tag. The site ships **zero** JS by default.

## 6c. Per-post `<style>` (SCSS)

**This formalises a pattern the posts already use.** Posts containing `<style>` blocks are already written in SCSS shape — nested rules and `&` parent selectors — but Jekyll passes them through raw, so today they only render because native CSS nesting happens to work in current browsers. They are unvalidated and broken on anything older.

```scss
table#bit_twiddling_truth_table {
  thead { background-color: #fafafa; th { padding: 0.2rem; } }
  th, td { &.slashed-background { … } }
}
```

Compiling them through `grass` flattens the nesting — widening browser support while preserving intent.

The rule: any `<style>` block in a row's body is extracted by the HTML rewrite pass (§6a), compiled as SCSS, cached by hash in `_cache/css/`, and hoisted into `<head>`.

- **Inline `<style>` in `<head>`, not a `<link>`.** These blocks are small and per-page; a separate file would add a render-blocking request.
- **Auto-scoped by default.** Compiling as SCSS makes scoping free: nest the author's rules under the post's unique selector. Opt out with `style_scope: false` in front matter for the rare global rule.
- **Syntax errors become build errors**, named to the post — a transaction-time constraint like every other.

## 6d. Blocks and rewrites: two ways into the rendered markdown

`markdown::render_doc` parses once and yields both the whole render (unchanged) and the top-level block sequence.

**The summary is a computed field on the view's rows** — a derived column:

```toml
[sets.published.fields.summary]
truncate = { max_blocks = 4, max_chars = 700 }
```

`Doc::truncate` is mechanism only (blocks kept until a budget runs out, block granularity, at least one always kept). **Fields flow with rows through `from` composition**: declared once on `published`, every listing composed over it inherits the column; redeclaring the name overrides, nearest wins. The deriver's fact (`truncated`) rides along, feeding `data-truncated`. Listing previews consume the field named `summary` by convention; no summary field means rows ship whole.

Two wrong altitudes were corrected: the cut rule started as engine code — policy belongs in config — and then as a view *attribute*, when a summary is a property of the rows, not of the view's rendering. **Marked not-quite-right (q31)**: deriver-as-struct-key is a stopgap shape; if config grows *functions*, a field wants to be an expression (§5f).

**Stage B, partly built (2026-07-21).** The **rewrite stage exists, narrowly** (`rewrite.rs`, lol_html): `a[href]` resolution for rows whose source *is* HTML — `.html` page bodies, `.html` slot fills, raw-HTML landings — the one job the AST pass cannot do. It is deliberately not the rule table below: q26's dimensions became the fourth at expansion time, and neither site wants an authored rule, so the selector language waits for its second consumer. Still deferred: the **notes stream**, which needs its consumer — sidenotes want a third grid column (q18).

One asymmetry the narrow stage carries: a raw-HTML body has `{% view %}` expanded INTO it, so the rewriter meets engine-derived URLs beside authored ones and cannot tell them apart. A URL already naming a materialized route is left alone rather than answered with strict's "link the source instead"; a page with no embed gets strict whole.

### Blocks, and the 93% that justified them

The justification is measured: the site used to truncate summaries in CSS, so `/blog/` shipped complete post bodies and hid most with `display:none` — **93% of the page (131,071 of 140,884 bytes)**. With blocks the summary never emits blocks 3..n. **Result: `/blog/` 160 KB → 15.7 KB, `/blog/tags/rust/` 180 KB → 11.3 KB (93.8% smaller)**, and CSS truncation rules are deleted.

### Blocks

Markdown renders to a **sequence of top-level blocks** (paragraph, heading, code, list, table, html) rather than one string. A layout kind takes what it needs: `document` takes all, `summary` takes the first few, a future `lede` slot takes `blocks[0]`.

**326 of 327 posts satisfy `concat(blocks) == markdown_to_html(src)` byte for byte** — comrak's `format_html` takes any node, so blocks are a loop over `root.children()`, not a parser change, and a summary is a literal *prefix* of the document, which the harness can prove rather than eyeball. **The single mismatch is footnotes** — see below.

Blocks stay **internal to layout kinds** — they are not exposed to templates.

### Rewrites: one selector-driven stage, not five ad-hoc passes

The design has accumulated five separate transformations: bare-name `<img>`/`<iframe>` resolution (§6a), `feed_images` (§8), `<style>` extraction (§6c), code blocks → Rouge shape, and stripping comrak's heading anchors (§8a). Those are all the same operation — *match something, change it* — and `lol_html` is already a dependency.

Make it one stage with a rule table:

```toml
# code/.rewrite.toml
[[rule]]
match = "table"
wrap  = "<div class='table-scroll'>"

[[rule]]
match    = "a[href^='http']"
template = ".hooks/external-link.html"   # gets href, text
```

A rule whose replacement is a **template** addresses by CSS selector instead of by node type — a better addressing language, and one that already exists in the codebase.

### Why they compose rather than overlap

**Blocks give position; selectors give type and attributes.** Neither can do the other's job:

- A streaming rewriter cannot *relocate* content into a different layout slot, which is what a lede or a summary needs.
- A block list cannot say "every external link", which is what a rewrite needs.

And blocks *remove* the need for positional selectors in the rewriter, which matters because `lol_html` supports only a subset of CSS (attribute, class, descendant/child combinators, some `:nth-child` — **verify before relying on `:first-of-type` or `:has()`**). Position comes from the block index; the selector only has to match kind.

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

The blocks pipeline changes are coordinated with updates to the comrak rationale (§9a), the `<style>` extraction (§6c), and CSS truncation rules.

### Footnotes are not blocks — they are a second stream

The one corpus mismatch (`life-before-main`, +898 bytes) is not an edge case to paper over. It is a **category error** in the block model.

comrak's parser *relocates* footnote definitions to the end of `root.children()` — the author writes each definition directly under its referencing paragraph, and that adjacency is destroyed at **parse** time. Rendered standalone, each definition emits its own complete `<section class="footnotes"><ol>` wrapper, so `concat` yields N sections instead of one merged one. Those are the 898 bytes.

A footnote definition is not a block. **It is an annotation on a block.** Both hooks already exist:

```
definition content only:  format_html over the definition's *children*
                          → <p>The definition text.</p>   (the ↩ backref is gone:
                            it is formatter-injected, not in the AST)
block → note association: NodeValue::FootnoteReference { name, ix }
```

Model two streams:

```rust
pub struct Doc   { blocks: Vec<Block>, notes: Vec<Note> }
pub struct Block { html: String, tag: &'static str, notes: Vec<usize> }
pub struct Note  { name: String, num: u32, html: String }
```

The exception then **dissolves** rather than being special-cased: `concat(blocks) == whole` holds 327/327 for the content stream, and placement becomes a layout decision:

- **sidenote** — each block, then its notes into a margin slot (Tufte-style)
- **endnote** — all blocks, then the gathered section (today's behaviour)
- **summary** — `blocks[..cut]`, notes dropped

That last one fixes a bug for free: summaries currently ship `<sup><a href="#fn-0">` refs whose definition is past the cut and `display:none`'d — dead anchors on every listing. (There is also a latent duplicate-`id="fn-0"` collision if two footnote posts ever share a listing page. Only `life-before-main` defines footnotes today — the 2004 post's `[^` are regex character classes — so it is theoretical, but it stops being possible.)

**Sidenotes need a layout change**: the post grid is `grid-template-columns: 8.75rem minmax(0, 1fr)` — a *left* sticky margin with the content column claiming everything else. There is no right margin to render into. A third column is a theme change, and it is the first genuine use case for a layout owning a slot the document stream does not.

### The third addressing mode

Footnotes contradict the earlier claim that blocks + rewrites is enough:

| mechanism | addresses by | can do sidenotes? |
|---|---|---|
| blocks | position | ✗ — the note is not at its position |
| rewrites (`lol_html`) | selector | ✗ — streaming; cannot move a definition *backwards* to its ref |
| notes | **identity** (`name` ↔ `#fn-0`) | ✓ |

Association by identity is what neither covers, and it exists only at the AST. That is the concrete argument for AST-level access — not a preference.

### Risks

1. **Truncation semantics must be reproduced exactly**.
2. **Rewrite rules are unbounded rope.** A selector table that can inject templates is a small language; it needs the same load-time validation as filters (§5) or it becomes the untyped front matter problem again.
3. **Per-element template calls cost.** 327 posts × many elements. Mitigated by the content-addressed cache (§6b), but worth measuring before allowing templates in rules.
## 6e. Hierarchy: the page's tree and the tree's tree *(specced and built 2026-07)*

> **Status: both axes are built** (`outline.rs`, against the example site).
> **Path axis**: `.section` is engine vocabulary like `.slots/`, `order:` front matter landed on pages, index-less directories appear as unlinked labels (q27), nested `.section`s resolve nearest-wins, and `aria-current` rides the attribute hole.
> **Heading axis**: `toc:` rows carry their outline, extracted from rendered block bytes (id and text read out of shipped `<h2 id=…>`), so link and target cannot desync.
> One recursive `outline_entry` kind serves both axes through one theme fragment.

The site has two hierarchies on two axes: **headings nest by level** (h2 contains its h3s) and **pages nest by path** (`code/legacy/` contains projects). Both derive from position — §6d's position axis, read in depth instead of sequence. Half the machinery exists: **breadcrumbs are the upward walk** of the path tree (`ancestors()`, §5c). What's missing is the **downward walk**:

| | toward the root | toward the leaves |
|---|---|---|
| heading tree | — | **page ToC** — this document's outline |
| path tree | breadcrumbs ✅ (§5c) | **section tree** — a manual-style file ToC |

### One part vocabulary, two producers

Both ToCs are **the same recursive part kind**, produced from different sources.

```
"outline_entry" => [("label", Text), ("url", Url),
                    ("current", Text),                   // aria-current, the pagination trick
                    ("children", Stream("outline_entry"))]
```

The `document` schema gains two parts sharing it: `outline` (document headings) and `section` (enclosing section's page tree) — both `Stream("outline_entry")`. A page showing the file tree on the left and its own headings on the right is then *two grid areas in theme CSS*; a theme that declines either slot loses nothing (rule 2 deletes the empty element).

This is the **first self-referential schema**: streams render their child fragments by kind, recursion terminates on finite data, and a `toc` fragment containing `<ol data-slot="children">` is just a fragment that maps itself.

**Why not relations?** `relations` (§6b) are flat row↔row groups; outlines are recursive *containment*. Forcing an outline into a relation flattens it. Two shapes, kept apart.

**Derived, not authored — so parts, not slot fills.** `.slots/` carries content a human wrote; ToCs are computed from structure that already exists. Both exit through `data-slot` holes, but a ToC never lives in a file.

### The page outline (heading axis)

- **Source: the same parse that renders.** `render_doc` walks the AST once; collecting `(level, id, text)` per heading is a few lines in that walk. Ids are comrak's `auto_ids` — already emitted, already verified — and because the outline is extracted from the same AST pass that emits them, link and target *cannot* desynchronize.
- **Opt-in is schema, cascaded by the tree.** `toc: true` front matter, with markers/rules supplying subtree defaults, so "everything under `doc/` has a ToC" is one marker.
- **Depth is production policy, not CSS.** v1 hardcodes h2–h3; the §5f expression form is the future home — `toc = outline(content, {"max_depth": 3})`.

### The section tree (path axis)

- **The root is declared positionally.** A marker (working name `.section`) makes its directory a section root: every rendered row beneath it carries a `section` part — the root's subtree of pages, with the current row marked.
- **Membership and labels come from the database.** Rendered rows only (v1); labels are page titles; an index-less directory is q27's unlinked label.
- **Ordering must be declared, not inherited from `ls`.** `order:` front matter, else lexical filename.
- **`current` is the pagination trick**: an attribute hole fills `aria-current` on the row's own entry.
- Derivation is once per section per build, not once per page — every page in a section shows the same tree, only `current` moves.

### Costs and edges, named now

- Heading text needs inline-markup stripping (a heading containing a link outlines as text).
- Static passthrough rows are not pages: legacy HTML trees get no section part until they become rendered rows.
- A marker that declares *scope* is a new marker flavor: today's markers set row defaults; `.section` names a subtree unit. If it also wants options (depth, ordering), markers grow a payload — or `.schema.toml`-style per-directory config does it. That choice is q35.

## 6f. i18n: the locale axis *(first slice built 2026-07)*

q41 called this the one classic SSG feature the model lacked. The slice that exists follows Matt's framing — **path selectors tell us which language variant to select, and that is configurable** — and the model absorbed it with less machinery than the survey feared, because every hard part mapped onto something already built.

> The selector is the **second** instance of a mechanism this document now names once: a path carries properties, an extractor pulls them out, and what remains is the row's identity (q51). `filename_formats` is the first. The prefix selector reads a directory component, so `fr/recipes/dal.md` and `2026/01/01/hello.md` are the same shape. Locale must be stripped (row and translation pair on `by_logical`); dates need not.

```toml
[i18n]
default = "en"
locales = ["fr"]
# selector = "suffix" (default: dal.fr.md) | "prefix" (fr/recipes/dal.md)

[i18n.names]
fr = "Français"

[records.tags.meta]
name = { en = "meta", fr = "méta" }

[records.course.dinner]
name  = { en = "Dinner", fr = "Dîner" }
intro = { en = "These dinner recipes are sure to please!", fr = "…" }
```

The design, piece by piece:

- **The selector splits every row's path into (logical path, locale) at load.** Rules, globs, route tokens, schema governance and theme rules all see the LOGICAL path — so `red-lentil-dal.fr.md` rides the same rule, the same `.schema.toml`, the same recipes theme as its original, and lands at `/fr/recipes/red-lentil-dal/`. i18n off = the selector never fires = the main site is byte-identical.
- **A translation is a row, not a copy of the site.** Rows sharing a logical path pair through `by_logical` — the ONLY index that sees translations. Other indexes admit default-locale rows only, which makes every listing, feed, archive, section tree single-locale **in one place each**.
- **The language switcher is a relations axis.** `translations` joins similar/earlier/later/linked-from: dateless neighbors labelled via `[i18n.names]`, both directions, zero fragment changes in any theme — the §6b axes design absorbing its fifth member. The visible switcher is theme CSS geometry, not a mechanism: the relation fragment already stamps `data-axis`, so both example themes lift `.relation[data-axis="translations"]` and absolute-position it as a chip.
- **Enum records** (`[records.<field>.<id>]`): declare the value domain of ANY grouped field — tags, courses, whatever a view groups by — with `slug` (route-facing, locale-independent, defaults to id), `name` (string or per-locale map; fallback locale → default → id), and `intro` (the value's own landing prose). French tag displays *méta*; the French tag ARCHIVE is titled *« méta »*, because `{key}` in grouped titles renders through the record's name at the route's locale.
- **Filters see the axis**: `locale` joined the post, page and route schemas. The example's search deliberately declares nothing about locale — French rows are searchable, which is the right default.
- **Locale IS an axis, exposed through the q53 interface** *(built 2026-07)*. It stays two rows paired by `by_logical` (this section is unchanged), but it is a FILE axis in the q53 vocabulary — each member owns a content file (`index.fr.md`), against theme's REUSE axis where members share one row — and so it answers the same interface: `?locale=fr` selects a member the way `?theme=ledger` does (via `by_logical`, since the member is a different file; the `.fr` suffix is the implicit value, `index.fr.md?locale=en` overrides it), and locale composes with another axis, `/fr/{theme}/…`, as a positionable `{axis:locale}` token that falls back to the outer prefix when a template does not spend it. A member with no file skips — the file axis's only honest "missing" answer, since a reuse axis already covers the always-present case. See q53 for the composition, the reuse/file distinction, and the default-axis path list.
- **The axis slot: the switcher (q47, built 2026-07).** The `axes` part is a **shell** part — every axis the current page is a member of, each a group of member links with the current one flagged — so the switcher is chrome, by the search icon, not something in the article body. A theme places `data-slot="axes"` in its shell; the base renders each axis as a native `<details>` dropdown (`axis`/`axis_member` fragments) whose summary is the current member's label (`ledger`, `Français`) and whose menu is the rest. It works for **listing views**, not just rows — the engine computes a view's members from its own routes in other locales, so `/fr/blog/` finally links back to `/blog/`. It **superseded the `translations` relation**: the locale switcher is one group in the axis slot rather than a relation among earlier/later/related. An author can also write one by hand — `.?locale=fr`, `.?theme=ledger` — a relative link whose path is `.` (self) and whose selector pivots the current page onto that axis. The head's `hreflang` is unchanged: it still describes the document, the slot the navigation.

### Display names: one shape, one hierarchy

Every human-facing string a site emits resolves through one three-level hierarchy — **inline beats global beats engine built-in**:

1. **Inline, at the site.** Any display-name position takes a **`LocalizedStr`**: a bare string, or a per-locale map — `crumb = { en = "Notes", fr = "Carnet" }`.
2. **The global map, `[i18n.strings]`.** Two kinds of key: *engine vocabulary* keys override what the engine emits everywhere; *user keys* are shared strings any site can pull in by **reference** — `title = "@tagged"` (`"@@…"` escapes a literal `@`).
3. **Engine built-ins.** The engine's vocabulary is closed: `home`, `drafts`, `related`, `later`, `earlier`, `linked_from`, `translations`, `page` = `"Page {n}"`, each with its English default.

Load rules keep resolution total and typos loud: a per-locale map may only name declared locales and must include the default; a `@reference` must resolve (error naming every known key); and a **non-engine global key nobody references is an error** — how a typo'd engine override surfaces.

**Resolution locale is the row's locale** for row-scoped surfaces (axis labels, trails) and **the view's locale** for listing surfaces — default locale today.

### Honest edges, named now

- **A localized post's trail is complete**: `Accueil(→ /fr/) › Carnet(→ /fr/blog/) › 10 January 2026`. "Home" is **existence-checked** — it links the locale's own homepage when a translated index exists, else the site root.
- **Localized tree pages walk URL ancestors**, and the duplicate home crumb on `/fr/…` URLs is **cured** (§5h: `ancestors()` skips locale-prefix homes). A section crumb appears in French exactly when the section's landing has a French variant.
- **`.slots/` fills localize by the same suffix convention** (`nav.fr.md` beside `nav.md`), and their view links resolve per consuming page's locale.
- **Locale-parallel views are built and DEFAULT-ON.** Every materializing row-query view partitions per declared locale: that locale's rows, the locale-prefixed route (default locale unprefixed), title/crumb/trail resolved at the route's locale. **A locale with no rows materializes nothing**: the partition is real, not mirrored. Opt-out is `locales = "default"`. Exempt by design: **star views** never multiply, **object views** carry no locale, and **embedded views** follow their embedding page (pending).
- **Still locale-free, and known**: `month_name` (computed at route build), `pretty_date`, the search overlay's strings (client-side, pending search being locale-aware), and `site.title`. Localized group *keys* are q40-adjacent.
- The markers walk uses **physical** paths — irrelevant for suffix, a known caveat for prefix (built and tested but unexercised by a corpus).

## 6g. Relations: every neighbour list is a declared query *(q52, resolved 2026-07-23; built 2026-07-23 — the §5f forcing point)*

Relations are hardcoded — five groups in `parts.rs`, unconditional, ranging over whatever table the code reached for. The reframe that settles the shape: **each list is a small query.** "Related" = published posts ranked by similarity, top few. "Later/Earlier" = the date-order neighbours. "Linked from" = rows whose links land here. §5c already built most of this machine — a set is sort-once, slice-everywhere; a relation is the same pipeline with a sort that is **row-relative** (a different order per post).

```toml
[collections.relations.related]
over     = "published"    # candidate pool: a set, or a derived relation
where    = "!(candidate in earlier) && !(candidate in later)"
rank     = "embedding_similarity(self, candidate)"   # double, bigger wins
limit    = 4
# also: match (glob, scopes self), min_rank (threshold), label ("@ref")
```

Pipeline per row: `over → where → rank (+ min_rank) → limit`. Drops happen before the window, so an exclusion never shortens the list.

### The shape war, closed

q52 weighed three shapes; the resolution takes B's spine with expression syntax. **(A) set-algebra strings** were rejected for *restating* other relations' definitions — the §5c disease. The expression form does not restate: `!(candidate in earlier)` refers to Earlier **by name** — "whatever Earlier shows, not that" — so changing Earlier cannot desync it. **(B)'s `exclude` key dies** as a second spelling of `where`, and B's closed `of` vocabulary dies for two of the four families — order and metric are plain expressions. **(C) relations-as-sets stays rejected**: a set is row-independent, a relation row-relative.

### The grammar is §5f's CEL — so `not in` is spelled `!(… in …)`

The draft wrote `candidate not in earlier`; CEL has `in` but no `not in`. §5f's contract — *grammatically valid CEL, never a dialect* — is what keeps the swap-in-a-real-crate escape hatch real. The contract outranks prettiness, and `!draft` already set the `!` house style.

What relations force into existence: arithmetic on doubles, unary minus, registered functions with row-typed arguments, and a **two-row environment**. §5f's tripwire ("a function wants other rows → that's a view") gains its one sanctioned, bounded exception.

### The environment: two rows and the finished lists

- **`self`** (the row being rendered) and **`candidate`** (the row under consideration). Field access is always qualified — a bare `tags` is ambiguous, so it is a load error.
- **Every relation name is a value**: a list of rows, where `x in name` means membership in that relation's **finished, limited list** — "already shown as X". For a threshold, call the function (`embedding_similarity(self, candidate) > 0.5`); names and functions are complementary.
- **Functions are registered in Rust** (§5f), never defined in config. Three are built: `embedding_similarity(row, row)`, `year_gap(row, row)` and `levenshtein(string, string)`. Args reach them as URLs, resolved through the engine's ctx. A function no config uses would type-check and then silently produce an empty group, so it stays a load error until something wires it.
- **Score direction: bigger always wins.** Distance functions wear a minus sign — `rank = "-levenshtein(…)"` — already house style (`order_by = "-date"`). No per-relation asc/desc knob.

### Graph and path are names, not expressions

Two of q52's four families cannot be computed from two rows' fields — they need the link graph or the tree. They become **derived relations**: names the engine always provides, usable two ways — referenced in `where` (`!(candidate in ancestors)`) or as the candidate pool (`over = "linked_from"`).

| family | operators | becomes |
|---|---|---|
| **order** | prev, next | an expression over a set |
| **metric** | similar | an expression over a set (`rank`) |
| **graph** | links_to, linked_from | derived names (`backlinks_map` computes the forward direction; the inverse is free) |
| **path** | parent, children, ancestors, siblings, descendants | derived names — the tree family q52 claimed |

Derived names exist whether or not anything renders them; only **declared** relations emit a group. q52's load-bearing separation survives — an operator supplies ROWS, the consumer decides presentation — and **sequencing caution survives**: trails and section trees are built, byte-verified consumers of the path family's idea; they stay on their own code until something needs them unified.

### Defaults ship the fixes

A collection declaring no relations gets these four; overriding is per NAME:

```toml
[collections.relations.earlier]
over  = "published"
where = "candidate.date < self.date"
rank  = "candidate.date"
limit = 1
# `later` is the mirror image

[collections.relations.related]
over  = "published"
where = "!(candidate in earlier) && !(candidate in later) && !(candidate in links_to)"
rank  = "embedding_similarity(self, candidate)"
limit = 4

[collections.relations.linked_from]
over  = "linked_from"
where = "!(candidate in ancestors)"
```

Three defects, found by eyeballing real pages, are why the defaults are not today's behaviour:

1. **Related re-shows the neighbours.** Similarity ranks the whole corpus and doesn't know the other lists exist; on a real post, two of Related's three entries were already on the page as Earlier and Later. Fixed by the `where` above.
2. **"Linked from: Home."** The homepage's recent-posts arrangement counts as a citation. Not fixable in this syntax — see below.
3. **"Linked from: its own breadcrumb parent."** Fixed by `!(candidate in ancestors)` — and the scoping is data, not an `if`: a blog post's trail is date archives, so it *has* no page ancestors and the clause does nothing there.

Retired by the defaults: the collection-level `adjacency` key and the `[related]` block. An explicit `over` is taken verbatim; only the defaults' fallback carries the filter.

### Evaluation, pinned

- **Order**: relations may reference each other, so they evaluate in dependency order; a reference cycle is a **config-load error**, never a render surprise.
- **Self is never a candidate** — a mechanism rule, not a `where` clause every site writes.
- **`min_rank` thresholds the rank value.** It exists because `min_score` applies to the *adjusted* score; without the key, the only spelling restates the whole rank expression inside `where`.
- **Determinism**: ties break by `(rank, date desc, url)` — the discipline `backlinks_map` already applies, and pagination proves it matters.

### Output: same parts, one rename

Each declared relation with a nonempty list emits one `relation` group — `{relation: NAME, label, items}` — an empty one contributes no group. **Render order is canonical, not evaluation order**: the engine evaluates in dependency order, but groups render in a fixed one — the four defaults `earlier, later, related, linked_from`, then site-defined names by name. Labels are `@refs` into `[i18n.strings]`, defaulting to `@NAME`. The q53 rename: names stamp **`data-relation`** (was `data-axis`, misnamed since the axis/relation split).

### The three sites

- **minimal**: **zero lines change.** Declares nothing, inherits the four defaults, gets every fix.
- **grack.com**: `adjacency = "published"` and `[related]` dissolve into:

  ```toml
  [collections.relations.related]
  over     = "published"
  where    = "!(candidate in earlier) && !(candidate in later) && !(candidate in links_to)"
  rank     = "embedding_similarity(self, candidate) - 0.01 * year_gap(self, candidate)"
  min_rank = 0.4
  limit    = 4
  ```

- **field-notes** (the falsifier): the same mechanical `[related]` migration; plus the relation the engine never hardcoded, on the TREE collection:

  ```toml
  [collections.relations.same_course]
  over  = "recipes"
  match = "recipes/**"
  where = "candidate.course == self.course"
  rank  = "-levenshtein(self.title, candidate.title)"
  limit = 3
  label = "@same_course"
  ```

  `match` is why relations carry a glob: `self.course` only type-checks against the recipes subtree's `.schema.toml`. The glob scopes which rows carry the relation *and* names the schema to check against.

### Problem 2 belongs to the link layer, and the scanner serves two masters

"Linked from: Home" is a fact about the **link**, not the page: the homepage cites you through its recent-posts *arrangement*; a real citation cites you through someone's writing. That distinction lives on the link, which the two-row model cannot see. The fix is in link-graph construction: `backlinks_map` scans the rendered fragment, which for the homepage **includes the spliced `{% view %}` output**, so every arrangement link counts as a citation. Mark the splice boundary and skip it — but only for the backlink consumer: `cited_urls` is one scanner with two clients, and the on-demand publisher must keep seeing arrangement links, or an image referenced only by a listing quietly unpublishes.

### Honest edges, named now

- **Locales: decided, built.** A pool is default-locale by construction (§6f), so a French page's candidates **pivot through `by_logical` to the row's locale**, dropping members with no variant there. Without it every translated page's relations were empty; with it fr-carbonara relates to fr-dal. The old `embed::rank` ranked within a locale and old `linked_from` was locale-blind; the pivot is the one §6f rule, applied uniformly.
- **Cross-kind fields.** A pool spanning kinds may compare only fields every candidate carries; the rest is a load error where checkable.
- **Two slices, both built 2026-07-23.** Slice 1: `where` + name membership, the config surface, defects 1 and 3 fixed, `adjacency`/`[related]` retired. Slice 2: `rank` expressions — the §5f build (arithmetic, unary minus, the two-row function registry, the `Double` type), with grack.com's year-penalty migration as its acceptance test. Problem 2 landed too: `{% view %}` splices carry marker comments the citation scan skips. `earlier`/`later` express the date ordinal from the `y/m/d` columns.
## 7. Clients of the database

Both `build` and `serve` use one render path: `build::render_site` produces `URL → bytes` in memory.

- **`grackle build`** — AOT materialization: render the map, write to disk.
- **`grackle serve`** — 🟡 **built (v1).** Resident render map via raw `hyper`. A `notify` watcher rebuilds on content change (~0.3s) and bumps version; injected script polls and reloads. Snapshot lives in `keepcalm` RCU cell: reads are lock-free, writer swaps whole snapshot with no blocking (verified: 20 concurrent reads through rebuild, all 200). **v1 re-renders everything** (still sub-second) and polls rather than streaming (§2 upgrades not yet built).
- **`grackle query`** — REPL/CLI over live DB (`urls`, `posts where tag=rust limit 5`, `explain <url>`). Doubles as migration validator.
- **`grackle urls --against _site-prod`** — URL-set parity. A **missing** URL exits non-zero; an **extra** is reported only. Derived assets exempt per q12.
- **`grackle diff --against _site-prod`** — Golden comparison: normalized HTML per post body with summary matrix (identical / equivalent / differs / missing). Bodies only.

## 7a. The example site: the falsifier for site-independence *(started 2026-07)*

`grackle/examples/field-notes/` is a **kitchen-sink falsifier**: a self-contained site that forces parked features in parallel.

### The second example is a yardstick, not a showcase *(2026-07-20)*

`examples/minimal/` is the opposite: **two posts, one page, smallest config.** Measured at introduction: **27 non-blank, non-comment lines**. A yardstick — the count should fall as defaults land; a rise wants a reason.

Two rules keep it honest:

1. **The example never gets special-cased engine code.** Anything it needs is a real feature or bug.
2. **It has no byte oracle, on purpose.** Verified by invariants (load-time constraints, null-theme completeness, route collision checks).

## 7b. The backtest: 36 real sites against the model *(surveyed 2026-07)*

35/36 sites fetched. Every blog-shaped site backtests cleanly; collections + routes + views + part maps cover them without strain. Two reported misses were false — they reflect under-communication in the model card, not gaps in the model.

### The headline: the core model holds

Blog sites demonstrably fit the model; feature parity is established.

### The gap clusters, and the questions they opened

| gap | driven by | carried in |
|---|---|---|
| **The link graph** — backlinks, then transclusion | andymatuschak, maggieappleton, gwern | q38 |
| **Set-scoped computed fields** | meal-plan rollups, paulstamatiou's counts, diataxis indexes | q39 |
| **Structured record fields** | ingredient lists, podcast chapters, cast lists | q40 |
| **i18n** | docs.astro, solar.lowtechmagazine (12 languages) | **§6f — built.** |
| **Client-side faceted filtering** | recipe sites, digital gardens (diet × cuisine × season) | q42 |
| **Media beyond image** | sive.rs interviews, podcast sites, fasterthanli.me, macwright's CDN | q43 |
| **Per-row scoped assets** | ciechanowski's per-article JS/CSS pairs | §5b |

### The confirmed non-goal, sized

**Memberships, paywalls, comments, ratings** — the dynamic-server non-goal (ch. 33) — is the single biggest cluster, measured: most *monetized* sites add these atop the static core. The design keeps the line: entitlements are edge/CDN concern; user-generated content is external embed.

## 7c. The inspector: the database explaining itself *(built 2026-07-19)*

🟡 **built 2026-07-19.** `grackle serve` reserves `/__debug__` from the binary: serve-only, closed namespace (§7).

**Four lenses, cardinality picks form:**

- **tree** — source and URL side by side. The difference *is* the route template.
- **rows** — a table per table, typed columns, flags visible.
- **views** — every declared query and its fan-out. Star views carry no `members` (they range over routes), so payload evaluates filter the same way.
- **diagnose** — anomaly first. The bar: it must be able to be wrong (an undated draft is not a finding; an undated publishable post is, threefold cost).

**The provenance strip** — source → route → the views that picked it up. A generic database viewer cannot show this: a claimed row has no route (§5h), a translated row has two (§6f), a view route has 66 members and no row.

The gutter draws current selection's correspondence with up/down arrows for scrolled targets and dashed connectors for collapsed ancestors.

## 7d. Fixture tests: a directory in, a directory out *(built 2026-07-25)*

🟡 **built 2026-07-25.** An audit found ~17 tests hand-building what the loader produces. A test testing a *site* belongs in a fixture; a test testing a *function* does not.

`crates/grackle/tests/fixtures/<name>/` is `site/` (real `grackle.toml` + content) and either `out/` (expected tree, in git) or `expected-error` (substring on load failure). One `#[test]` walks all and collects every problem before panicking. `UPDATE_EXPECT=1` re-blesses.

**One finding:** a fixture's `crumb-trails` revealed a real route behaviour — a post from a collection declaring NO `trail` still gets a year crumb, because every `kind = "posts"` collection feeds one table and the archive claims it.

## 8. Known-inexact from day one (accepted, iterate later)

| Area | Why | Plan |
|---|---|---|
| Code highlighting spans | Rouge ≠ syntect token boundaries | 🟡 half done: wrappers + inline classes via AST pass (45 → 1 diffs). Still missing: Rouge's pygments spans for real languages. |
| kramdown edge syntax | IALs, `markdown="1"`, footnotes | comrak `smart` + extensions; triage real diffs per-post. **`markdown="1"` in wild** (2 posts): comrak drops `<div>` into `<p>`; one hand-normalized, other left raw as widget test fixture. |
| Related posts | LSI unreproducible and unwanted | **Superseded** (§6b): embeddings replace outright. Deliberate improvement. |
| Feed body HTML | `feed_images`/`expand_urls` on rendered HTML | ✅ **done** (regex port). Byte-verified against reference. Markdown gap remains (§8a), feed-only. |

### Heading anchors: kept, deliberately *(2026-07-21)*

comrak injects `<a class="anchor">` inside every heading. **226 of them across 44 posts, each carries aria-label** — a heading affordance the Jekyll site never had.

## 8a. The markdown gap, and what measuring it taught

The kramdown→comrak gap is a **90.0% usable** (92.2% if smartypants matched) ceiling, **parser-side**.

Posts both liquid-free and untouched since reference build: 20 identical, 187 equivalent, 23 differ. Residue: `10 inline/prose · 5 list · 4 link · 3 table · 1 code block`. Every one is **parse-stage**:

- `Windows '95` vs `'95` — kramdown rendering decade abbreviation with opening quote, comrak with apostrophe (comrak typographically right).
- `<li>text</li>` vs `<li><p>text</p></li>` — kramdown per-item, CommonMark per-list.
- Raw HTML in prose: `<solution>` auto-closed by kramdown, left open by comrak.

Zero heading, zero footnote, zero image diffs. The 90/92% ceiling is **parser ceiling**; if chased, fork comrak's parser, not formatter.

### The reference build lied by 17 points

The original headline was **90.7%** against `_site-prod` built before Rouge was enabled. The reference emitted bare `<pre><code>` — exactly what comrak emitted. Rebuilt against *current* config, "usable" fell to **72.6%** with rouge on. Our output never changed; only the yardstick did.

### Three measurement rules

1. **A reference build is an input, and inputs have versions.** Rebuild from *current* config before quoting any number derived from it.
2. **Agreement is not evidence unless it can disagree.** A test that cannot fail is not measuring.
3. **Read deltas, not tallies.** `classify_cause` is a heuristic and over-attributes.

### Retiring the body oracle

The body diff is **no longer a cutover gate** (2026-07-21, Matt's call). `grackle urls` gates the URL set; everything else is verified by eye.

The reference is a wasting asset (48 of 327 posts edited for migration work), the harness hides real differences (diff::normalize strips comrak anchors structurally, so 90% is computed with the difference removed), and the remaining gap is parser ceiling (~92%) already decided against (§9a).

What survives: `diff` as an *investigative* tool. What ends: treating its matrix as the gate.

### The 97-post blind spot *(open — q21)*

Related and still true: **`_site-prod` can no longer be regenerated** (§5c) — `{% view %}` is not Liquid, so Jekyll fails the whole build and refreshing the reference needs `git stash push index.html` first (q22). Losing the ability to refresh the reference is exactly the capability that caught the 17-point lie.

### Two SCSS findings worth keeping

- **`grass` rejects nested `@import` that libsass accepts.** `_sass/_post.scss` has `pre > code { @import "rouge"; }`; grass errors. Fixed by resolving `@import` textually before handing grass flattened source.
- **grass and sassc agree**: 2232 selectors vs live build's 2231 — one formatting difference, not semantic.

## 9. Crate layout *(as built; the original sketch is in git history)*

A cargo workspace of six members under `crates/`. The split is one dependency direction; **`grackle-db` depends on nothing in the workspace.**

`model -> db`. `source -> model, db`. `grackle -> all`. Nothing points back. ~17.5k lines across the workspace, ~216 tests.

### Why `db` is a crate and not a module

The boundary is worth paying for. **`grackle-db` cannot name a `Row`**, so a filter feature cannot quietly become a blog feature. It holds a mini database: rows answerable by `filter::Row`, keys stable across reloads, functions as extension points, views as values.

### The ordering rule *(2026-07)*

`path` ascending, unless the view names a column; `path` is the last tiebreak either way. A collection whose rows carry dates says `order_by = "-date"`. Adjacency is the exception: `neighbors_in` reads *position in a sequence*, so "later post" is the entry before, and its default stays newest-first.
## 9a. Dependencies: the inventory is `Cargo.toml`, this doc keeps decisions

What is depended on is answered by `Cargo.toml` alone. What stays here are the *decisions*:

- **No template engine.** The site measured out at ~3 real templating constructs, all Rust components (§5d).
- **No expression engine.** The config language is hand-rolled against a CEL grammar contract (§5f).
- **comrak over pulldown-cmark.** The mutable AST is load-bearing — the Rouge code-block shapes and §6d's block split live there.
- **No vector index, no rust-bert.** 327 vectors is a brute-force dot product; embeddings run on ONNX, not libtorch (§6b).
- **Raw hyper, no axum**, with a `keepcalm` RCU cell for the resident snapshot; no SSE, live reload is a poll (§7).
- **`ignore` is load-bearing.** The marker scan has no other defence against `_site*`/`vendor` (§4c).
- **`lol_html`, taken narrowly.** Only for resolving links in rows whose source is HTML, not the rule table §6d sketches.
- **`salsa` declined** — hand-rolled typed invalidation keys suffice at this scale (open question 1).

The bar for a new dependency: taken for a measured reason, and recorded here only when the decision itself is interesting.

### Why we do **not** write our own AST → HTML renderer

Everything we want is reachable without owning comrak's renderer: mutate the AST per node type, use escape hatches for the parts we control. The tripwire: if the escape-hatch list ever exceeds ~⅓ of node types, we have written the renderer accidentally and badly, and should write it deliberately.

### Neither code-block adapter fits (measured)

The obvious adapters don't fire where they matter: `CodefenceRendererAdapter` only fires with non-empty info strings (corpus is indented, not fenced); `SyntaxHighlighterAdapter` fires but comrak hardcodes the closing tags.

### What §6d's blocks change here

`lol_html` shrinks: img/iframe, `<style>`, and code shape happen at the node before HTML exists — no re-parse, no selector matching. `lol_html` drops back to user-authored `.rewrite.toml` rules over rendered output.

## 9b. Seams audit: is responsibility still split right?

Taken after refactoring, with the whole codebase freshly re-read. Verdict: **the load-bearing boundaries hold, and the leaks that exist are all one disease.**

### What holds

The pipeline's layer per module is real: config declares, `db` resolves and constrains, `views` materializes, `parts` produce, binder + theme arrange, CSS does geometry, `build` orchestrates, `serve` hosts. Recent features entered without bending anything — relations arrived as one more producer push, and summaries arrived as a config field.

The predictor of this health: **everything declared is load-checked** (filters, fragments, fields, widgets, slots), so a responsibility placed in config stays there.

### The one recurring disease

§5c named it: *the config declared what the renderer ignored*. Three more pockets have closed — producers hardcoding routes config owns, the feed pass selecting its view by string match, and the sitemap predicate evaluated three times. One remains:

- **Three definitions of "not content"** (→ q34). §4c's three layers govern the tree walk, but `slots.rs` and `serve.rs` carry private skip lists that can silently drift from `exclude`. Both walks should derive from the §4c layers.

### Accepted asymmetries, named so they don't read as leaks

- The CLI's `query search` indexes raw markdown where build indexes rendered HTML (documented at `search_docs`).
- `render.rs` has become "head facts + escaping + XML serializations"; if stage B touches the feed, the serializations can move out.
- `post_trail` is still single-posts-table; a second posts collection remains future work.
- `default` survives as the conventional theme name, and search assets live in the default theme.

### Round 2 *(2026-07-18, after landings, records, links and i18n)*

Boundaries held under load; example config shrank while gaining features. Key finding: the landing pass and bare passes both re-shaped rows separately — unified them via `row_preview`/`object_preview`.

### Round 3 *(2026-07-21, after the crate split)*

The split itself was the audit: boundaries you have to declare to Cargo are ones you cannot half-hold. Lessons: awkward dependencies are mislabelled layers; `pub` in libraries silences dead-code detection; declared-and-ignored keys are invisible until deleted; test fixtures need identity meaning.

### Since, and what is left *(2026-07-21)*

Three merges unified distinctions that were never real: two row flows became one; base table became a filter; last positional assumptions deleted. What remains: objects dispatch, loader collection choice, config validation, presentation policy.

### Surveys/audits worth re-running

- Seams audit post-landings, records, links and i18n: `build.rs` gravity well (~1,800 lines); trail family evicted to `trails.rs`; semantic drift in main config.
- Seams audit post-crate split: revisit boundary declarations as workspace grows.
- Seams audit post-merge passes: monitor kind branches and positional assumptions.

### Still owed

- **The objects dispatch.** `build_object_view` stays separate by design (§5b), and object rows are `rendered: false`. Folding it in would require three parameters; what was stale has been deleted; `group_by`/`paginate` still bail there.
- **The single tree** (§3's endgame: one table, views as partitions). Measured obstacles: `store.rs` skips `.`/`_` names by convention; six underscore directories need explicit excludes; `filename_formats` is per-collection where it would be per-rule.

## 10. Phasing (each phase has a checkable exit)

| Phase | Deliverable | State |
|---|---|---|
| **5** | **exactness iteration** | **exit criterion changed, 2026-07-21** (Matt): **URL parity by machine, the rest by eye.** `grackle urls` gates the URL set — protecting 20 years of inbound links — and body diff stops being a gate. |
| 7 | §6d blocks | 🟡 **stage A** — one parse, summary as a computed field, `data-truncated`. Stage B: notes stream + sidenotes (q18) and the rewrite stage |

## 11. Open questions (to iterate on)

Only OPEN questions live here; a settled question moves its design into the section that carries it and leaves one line in the ledger below, so `qNN` references elsewhere in this document always resolve. Numbers are never reused.

1. **Dependency tracking**: hand-rolled typed invalidation keys vs `salsa`. Leaning hand-rolled — at this scale precision bugs are cheaper than framework complexity.
2. **Row version**: content hash vs mtime+size vs mtime-then-hash pre-check (specced).
4. **Highlighting fidelity** — *half-settled*: wrapper/inline-code shape is done; only token spans remain (coarse Rouge-class mapping vs syntect classes). Gap is under-measured: 4 of 6 highlighted posts are liquid-skipped.
6. **Drafts**: replicate `_drafts` preview in `serve` from day one, or post-phase-3.
11. **Iframe policy**: §6a resolves and rewrites `<iframe src>` for bare names but doesn't thumbnail. Do iframes need sandbox/loading attributes injected?
13. **Embedding model pinning.** Cache key includes model identifier (`_cache/embed/{model}/`). Silent re-embed on upgrade (friendly) or explicit `grackle reindex` (predictable)?
14. **`<style>` auto-scoping default (§6c).** Scoping fixes a latent leak on multipost index pages but changes behavior on 3 existing posts. Default-on with opt-out, or default-off with opt-in?
21. **Tighten `diff`'s liquid skip (§8a).** 97 of 327 posts excluded, many falsely; 30% of corpus unmeasured.
22. **`_site-prod` refresh (§8a).** Jekyll fails on `{% view %}`; can no longer regenerate reference. Script refresh, or move behind a flag that stashes automatically?
23. **The `hero` part — the remainder.** Built via book club; still arriving with first-image fallback and mindstorms group hero (explicit beats derived).
25. **Per-block facts (§5e).** Block-level directive surviving as `data-` attribute so theme can span it. Needs decided authoring syntax — IALs are kramdown, not CommonMark.
26. **Dimension facts — the remainder.** Object rows carry width/height at load (queryable, §5b). Remains: post *bodies* — `{% image %}` gains dimensions at §6d rewrite stage.
28. **Mindstorms restructure vs URL parity (§5 audit).** Gallery restructure retires 17 URLs carrying no `noindex`. Needs redirects or parity exemption; fix accidental indexability before restructure.
30. **Pagination × subdivision (§5c).** A grouped view can subdivide; paginated one cannot yet. Year archive could paginate while months subdivide — row-set semantics cohere but namespace shares. Collision (hard error today) vs pattern-space overlap (should warn or declare).
33. **View-name policy in `build.rs` (§9b).** Settled: (a) listing `noindex` is a view declaration; (c) dead layout names renamed to `listing`; (f) row `layout:` dissolved. Remains: (b) `"blog_index"` fallback dies when view declares layout; (d) `template` no longer templates — it claims a legacy file; (e) sitemap filter's second evaluation.
34. **Three "not content" lists (§9b).** §4c's layers govern tree walk only; `slots.rs` and `serve.rs` carry private skips. Both should derive from §4c. Serve's `_cache/` stays its own (rebuild *writes* it).
37. **The `board` kind (§5c-adjacent, specced, deliberately pending).** A board is a query over queries. Would retire last hand-written arrangement on either homepage. Pending: (a) member declaration; (b) labels — per-member vs inherited; (c) routable or embed-only; (d) boards-in-boards; (e) board items vs opaque.
38. **Transclusion (§7b).** Render row X inline by reference. Backlinks half built; waits on real consumer, with §5d's no-control-flow rule.
39. **Set-scoped computed fields (§7b).** Fields derive from ONE row; survey wants aggregates — `count()`, `sum(minutes)`, date spans. Natural §5f extension but changes inheritance story.
40. **Structured record fields (§7b).** List-of-records type for ingredient lists, podcast chapters, cast lists, plus schema.org/JSON-LD emission. Extends §5b without changing shape. §6f's enum records took value-domain half; this is row-field half.
42. **Client-side faceted filtering (§7b).** Combinable facets can't enumerate as static views. Generalize search.bin architecture: ship typed facet index, run intersection client-side — a *client-side view* declared in config, materializing an index instead of routes.
43. **Media beyond image (§7b).** Audio/video schema field types (duration/player facts), podcast RSS enclosures, multi-format srcset renditions, externally-hosted originals.
47. **Listing views render no language switcher (§6f).** `translations` axis is a ROW relation; plain listing views don't get switcher. French reader at `/fr/blog/` has no way back. Locale-parallel routes exist; question is whether axis belongs to route at all.
48. **`type:` as row data, not presentation** *(Matt's shape)*. Row declares *what it is* (`type: recipe`), config maps to presentation (§4b rule). Test: *something other than renderer consumes it* — cross-tree filter, q40's JSON-LD, non-positional schema selection. Held deliberately; neither site needs one.
49. **Where a row's metadata comes from when the file can't carry front matter** *(measured 2026-07-19)*. **Derive first** — 14 of 57 raw HTML carry `<title>` database ignores. **Then declare** — per-file sidecar `.p01.png.toml`, fallback for rows that can't carry front matter. Open: precedence vs markers/defaults; whether sidecar makes passthrough row `rendered`; real consumer is alt text for 838 images nobody committed to.
    
    **What this must NOT do**: infer page-vs-component from absence. Heuristic measures well but is wrong twice: `demos/1996/mystery.html` (complete 1996 markup, no `<html>`); `demos/css-glass-pane/index.html` (880-byte demo, no title). Reading what exists is derivation; concluding from absence is guessing.

50. **Transplanting an imported page** *(Matt's case)*. Import raw HTML page, lift *meat* out and render through theme. No mechanism: front matter on full document nests it inside second `<html>`. Two operations, must not fuse: **extraction** (body children or selector-scoped region — scheduled at q49/§6d stage B) and **chrome** (row `shell:`, §5g). Left open: `light` may be honest destination for imports; theme wanting *less* furniture can only omit a part-hole (binder doesn't flag it). Deliberate omission byte-identical to forgotten one. **How does a theme say "I deliberately don't place this part"?** Must settle first.

51. **One row type: route-token supply** *(built; remainder)*. **Built 2026-07:** one `Row` type, one `SiteDb.rows` store with membership lists, `date`/`tags`/`theme`/`fields` on every row. **Remains:** two route-token suppliers still disjoint — path tokens (path/dir/stem/name/ext) for tree; inline `match` for posts (year/month/day/slug) — so `_posts/rust/hello.md` can't route to `/rust/hello/`. Fix: one supplier offering path tokens always plus extractor results. Validation exists (posts-path-only), move it. Also: most-specific-source rule for `_posts` inside `.`.
    
    Three rules merge bought (each from silent failure): `.schema.toml` may not redeclare base field (load error); ordering belongs to SET, not table; for additive capability, byte-identical proves nothing.

53. **Axes: alternative forms of a row** *(Matt, 2026-07-20; **built 2026-07-25**)*.

    | | **relation** (q52) | **axis** |
    |---|---|---|
    | points at | *other rows* | *other forms of THIS row* |
    | examples | prev, next, similar, links_to, parent, children | translations, thumbnails, serializations, an object's description page |
    | renders as | labelled group in body | `<link rel="alternate">` in head, plus inline affordance |
    | needs a reach? | yes — which set ranges over | no — row determines own members |

    **Mechanically: one row, several routes, keyed by a value** — and §4's constraint names an axis as the sole thing permitted to break "a row renders at exactly one route".

    ```toml
    [axes.theme]
    values = ["default", "loud"]   # the members; first is CANONICAL
    field  = "theme"               # the row field each member sets
    ```

    **Where a member lands is not declared with the axis** *(step 2, built 2026-07-25)*. A route template spends it with a `{theme}` segment — a collection rule for row routes, a `[routes.*]` path for view routes — so URL shape lives where every other URL shape lives, and **the rule that spends an axis is the rule that opts its rows in**. An `[axes.*] url` and an `[axes.*] match` both retired into that one idea: the rule already decides where a row lands and which rows it covers, so it was saying half of this twice.

    ```toml
      [[collections.rules]]
      match = "index.md"
      route = "/"                        # spends nothing: publishes once

      [[collections.rules]]
      match = "**"
      route = "/{theme}/notes/{slug}/"   # spends the axis: publishes per member
    ```

    **Canonical-bare, then back — as an opt-in** *(the default-axis case, built 2026-07)*. The canonical member first kept the row's own URL (mimicking the default locale's missing `/fr/`); then, once the template allocated the segment, every member wore one. Both were the wrong default to *force*. A rule or view now takes a LIST of templates — `route = ["/{theme}/{axis:locale}/", "/{theme}/", "/"]` — and the engine picks the shortest one that still spends every NON-canonical axis, so a canonical member drops its segment exactly when a shorter template offers to omit it and wears it otherwise. A single template (the ordinary case) is unchanged: every member wears its segment. Canonical stays a *declaration* — which member `rel="canonical"` names and which one a `*` view sees — and is now *also* what the path list may elide.

    **A correction this build produced, worth keeping.** The question claimed four instances of one shape and listed locale as built. Locale is not that shape: `dal.md` and `dal.fr.md` are **two rows**, one route each, paired after the fact by `by_logical`; thumbnails are derived artifacts and not rows; the md twin was a serialization. So the axis was a *new* mechanism, not the generalization of an existing one — and the argument for building it was accordingly weaker than the question implied. What redeemed it was that the second field cost nothing: `field = "shell"` is q44's md twin, working the day the axis landed, and that is the multiplicativeness the question was actually claiming.

    **One materializer, one product** *(step 3, built 2026-07-25)*. Grouped, paginated and single were three branches, each with its own locale loop and its own idea of how a route is built — and the grouped one carried a second copy of the pagination loop. They are one shape: **partition, slice, emit**. An ungrouped view is a grouped one with a single cell; a single-page view is a paginated one whose slice is the whole cell. The dimensions, outermost first, are axis (spent into the templates by the caller), locale, group, then page. Byte-identical on every site.

    The one thing deliberately not generalized: `limit` truncates only an unpaginated, ungrouped view, where it means "the feed's twenty". Over a partition it would silently start truncating group pages.

    **The four answers.**

    1. **Which rows multiply** — those a `match` glob selects, and only *rendered* rows. An axis publishes alternative forms of a document; a static file or an image has one form, its bytes. (A thumbnail is an axis in spirit but it is the image pipeline's, keyed by size and content-addressed.)
    2. **Identity across members** — one row, N routes, via `Route.row`. This is what the prerequisite bought: before it, a route's row was recovered as `by_url.get(r.url)`, which answers "one" by construction and could never have seen the second.
    3. **The canonical member** — the first declared, and it **keeps the row's own URL**; only alternates are templated. Exactly the shape the default locale has in sitting above the selector with no `/fr/`. Every member's `rel="canonical"` and `og:url` name the canonical form, because the head describes the *document* rather than the form. And a `*` view sees canonical members only: listing every member in the sitemap or search index would ask a crawler to treat six renderings of one document as six documents, which is what `rel="canonical"` exists to deny.
    4. **Composition** *(built 2026-07)* — axes over one row compose into the cartesian product rather than colliding. The constraint keys on the member-**tuple**, not one member, so a row (or a view) that spends `{palette}` and `{flavor}` lands at `/plain/sweet/…` through `/fancy/salty/…`, one route per tuple; `Route.axis`/`Row.axis` grew from one member to the list, and spending is per-axis — each must have its own segment or its members collide. Canonical is the tuple of first-declared members, and a `*` view sees a route only when EVERY member in its tuple is canonical.

        **Locale is folded in too**, which retired the "own mechanism" caveat. The unifying distinction is a REUSE axis vs a FILE axis. A reuse axis (theme) renders one row N ways, so every member reuses the canonical row's content and none is ever *missing*. A file axis (locale) gives each member its own content file — `index.fr.md` is the `fr` member's file — and a member with no file simply does not materialize (there is no "duplicate the default" policy: a reuse axis already covers the always-present case, so the only honest answer for a file axis's missing member is to skip it). Locale keeps its two-row model (`by_logical` still pairs the files) but is now exposed *through* the axis interface: `?locale=fr` selects a member the way `?theme=ledger` does — resolved through `by_logical` since the member is a different file, with the `.fr` suffix as the implicit value and `index.fr.md?locale=en` overriding it — and locale composes with another axis, `/fr/{theme}/notes/one/`. Locale is a positionable spent token: `{axis:locale}` (or bare `{locale}`) in a template places the segment wherever the author writes it — `/{theme}/{axis:locale}/` lands the segment after the theme's — and a template that spends no locale token falls back to the outer prefix, the shape a config without `{axis:locale}` has always had. So the "always the outer prefix" limitation is gone: it is the *default*, not the only option.

    **How a field takes effect, and why no field is inert** *(2026-07-25)*. An axis sets a named row field: `theme` and `shell` are wired into the render paths, and a field no path consults would once have multiplied URLs without changing a byte. The first answer here was "make that a load error naming the fields that mean something", and it was wrong — Matt's correction: a member can differ *presentationally* with no engine involvement at all, so the mechanism should make that work rather than forbid it.

    So a member stamps **two** things on the root, beside `data-subtheme` and `data-profile` — the engine stamps, the theme decides what it means:

    | stamp | for |
    |---|---|
    | `data-axis-theme="ledger"` | **selecting** — `[data-axis-theme="ledger"] h1 { … }` |
    | `--axis-theme: "ledger"` | **reading** — `h1::after { content: var(--axis-theme) }` |

    Both, because neither substitutes for the other, and the reason is a CSS rule worth writing down: **`attr()` reads only the attribute of the element a rule MATCHES.** The axis is on the root, so a descendant cannot reach it that way — `h1::after { content: attr(data-axis-theme) }` computes to the empty string. Verified in a browser rather than assumed. A custom property inherits and `content` accepts `var()`, so the value is legible from anywhere; it is stamped pre-quoted so it drops into `content` as-is.

    An axis declared purely to give CSS something to key on is therefore a legitimate use with no engine wiring, and `field` is what a member sets *if the engine knows that field* rather than a promise the engine must be able to keep.

    **A VIEW may be materialized across an axis** *(built 2026-07-25; a LIST of axes 2026-07)*. `[routes.x] axis = "theme"` (or `axis = ["palette", "flavor"]` for the product), and the path spends each with its `{name}` segment. The route allocates the URL space because that is where the rest of the URL is already decided — the axis declares its values and its field, not its shape. The axes are the OUTERMOST dimensions: locale, grouping and pagination all happen within each member-tuple, which is what made this a substitution rather than a rewrite.

    Two things fall out. A path that declares an axis and never spends it is a load error — the members would collide on one URL, and five of six would be lost. And **a landing view on an axis claims ONE row**: the "a row serves one landing" constraint is keyed on the view, so six materializations of one view are one claim. That is what retired per-group `content` before it was built: `theme-preview` went from eighteen landing declarations and eighteen copies of their prose to three of each.

    Multi-dimensional paths follow the same rule — `/{theme}/{year}/` is an axis segment and a group segment, each filled by the thing that owns it.

    **Linking to a member: `path.md?axis=value`** *(built 2026-07-25)*. A link resolves to a ROW and a row answers with its canonical URL, so a member had no spelling and the merged gallery could not name one — the first build of it failed on exactly that. The selector reads as a query string and resolves to a PATH, which is the point: a member's address is derived like every other URL here. Held to the same standard as every other link — an undeclared value is a load error naming the members, and a selector on a row the axis does not cover is one too. Only a *declared* axis name is read this way, so `?utm=x` stays the literal suffix it always was. A view route wears the same spelling — `view:notes_index?theme=ledger` — and naming an axis-materialized view without one is an error, because it lands at several URLs and there is no honest default among them.

    **Axis members now emit `rel="alternate"`** *(built 2026-07)*. `Head.alternates` grew from a hreflang-shaped `(lang, url)` pair to an `Alternate { href, hreflang?, media_type? }` — the "variable-length head entries" shape (§4e), a list that can repeat `rel` and carry a second attribute. Each member lists its OTHER forms: the locale axis carries `hreflang` (as before), a different-FORMAT form (the md twin) carries `type`, and a same-format restyle — a theme member — carries neither, because it is the same representation at another URL and `rel="canonical"` already names the one that counts. Whether a form is a different representation is read off the member URL's extension. The `light` tier still carries no canonical and, by the same minimal head, no alternates — an alternate at that tier would advertise nothing.

    A cost paid earlier stands: `data-axis` was renamed `data-relation` for relations, with `data-axis` kept for translations.


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
| 36 | one preview kind: `summary` (presence-driven); `card`/`card_list`/`gallery`/`figure` all folded in; `featured` slot on listing; `LAYOUTS` = listing/link_list/card | §5e |
| 5b→5e | config `[[parts]]` holds the part vocabulary/types/canonical order; a declared part with no engine producer is filled from the row field of that name (string/int→text, bool→fact, list→`stream:item`, image→url); an engine part wins name collisions; a type mismatch is a load error | §5e |
| image ref | an image field NAMES a row, checked at load (dangling = error; absolute url passes through); pixel `width`/`height` are header-read row columns, queryable against a literal | §5b, §6a |
| 41 | i18n: locale axis, `by_logical` pairing, translations axis, locale-parallel default-on, enum records | §6f |
| 44 | shells: root HTML shell engine-owned; atom/sitemap/search built-in; script shells as the bench; md specced; row tiers are pipeline exits (`none` is the shell layer's escape hatch, not an object and not a theme) | §5g |
| 10 | the drafts profile forces `noindex` site-wide — one profile key, not a per-row flag | §4a |
| 45 | landings: a view owns the URL, a row may own the words; claiming, the chain, theme provenance | §5h |
| 52 | relations are declared queries — `over`/`where`/`rank`/`limit` in §5f CEL; names mean finished lists; graph+path families are derived names; defaults ship the eye-check fixes; `adjacency` and `[related]` retire | §6g |
| 46 | `collection.crumb`/`index` dissolved — the URL climb is the sole source of a landing crumb, `trail` keeps the subdivision chain | §5h |
