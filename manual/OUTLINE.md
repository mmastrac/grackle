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
| computed `summary` | `/news/` listing |
| row `shell:` tiers | one imported/demo artifact shipped verbatim (§5g) |
| profiles + `_drafts` | chapters-in-progress drafted in the open, shipped under a `drafts` projection (§4a) |
| search shell | manual search is the obvious real need |
| themes: parts/slots/CSS | one theme that is *only its differences* over the base |
| the inspector | a screenshot of `/__debug/`'s source‖URL trees (§7c) |
| i18n ★ | *not* enabled v1; named as the next dogfood |

Two rules carried over from the example sites: **no engine special-casing
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
  (§7b). Point at ch. 33.
- Explicitly: you can read only Part I and have a working blog.

### 2. Install and the commands
- `cargo build --release` (a cargo workspace under `crates/`); a site is a
  directory with a `grackle.toml`.
- `grackle build`, `grackle serve` (watch + reload), `grackle query`,
  `grackle explain <url>`.
- `grackle urls --against <dir>` — URL-set parity, the migration
  instrument (ch. 24). A *missing* URL is a link that used to resolve and
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
  decides how much wrapper a row wears (ch. 19). A file with no front
  matter ships verbatim and is invisible to every query.
- **`[[collections]]` is an array of tables** — each is a *source* with a
  `kind` (`posts`/`tree`/`objects`), its name defaulting from the source
  directory. Objects are still a `kind`, though under the hood it's now
  one row store with three origins — a user just needs the three kinds and
  the disjoint membership order (posts → objects → tree).
- Errors, shown: two rows on one URL; dated route on an undated row; dead
  rule warning.
- `grackle urls` lists them.

### 6. Your first query: a set, then a route
- The pitch: you never write a loop. You name a set.
- **The whole model in one sentence: a *set* is a query; a *route* is a
  query that lands.** They are two config tables and the difference is one
  key:

  ```toml
  [sets.published]        # no path ⇒ never lands
  from  = "posts"
  where = "!draft && !hidden"
  order_by = "-date"

  [routes.blog_index]     # has paths ⇒ lands, paginated
  from     = "published"
  paginate = 5
  paths    = ["/blog/", "/blog/page/{n}/"]
  ```

- **`path` (or `paths`) is the switch.** Present → it's a route and can
  carry the landing keys (`title`, `crumb`, `shell`, `paginate`,
  `group_by`, `template`, `content`, `intro`, `featured`). Absent → it's a
  set, and those keys are meaningless (and rejected). Keys on both:
  `from`, `where`, `match`, `order_by`, `limit`, `layout`, `variant`.
- **`from` is the one composition keyword** (it replaced `over`). It names
  a collection, `*` (all rows), another set, or a route. What it names
  decides what it means — composing over a grouped *route* is subdivision
  (ch. 8), and the engine dispatches on the referent, not a second
  keyword.
- Why `published` is a separate set: one definition of "a post list",
  reused by the feed, tags, archives, home. (The real story: five
  hand-written Jekyll guards had drifted into three different answers and
  the feed was shipping drafts.)
- **`where` is the filter** (it replaced `filter`); bare field =
  truthiness. `match` is a *separate* key — a glob over the source path,
  for scoping a query to a subtree — so the filter language stays
  typed-fields-only.
- Error, shown: `unknown field 'drafts' (did you mean 'draft'?)`.

### 7. Listings that don't ship the whole blog
- The problem: full bodies hidden by CSS.
- `[sets.published.fields.summary] truncate = { max_blocks = 4,
  max_chars = 700 }`.
- Computed fields inherit down `from` chains; nearest declaration wins.
- `summary` is the one preview kind, by convention. No summary field ⇒
  full bodies (intended, not a bug).
- Measured: `/blog/` 160 KB → 15.7 KB.
- `truncated` becomes `data-truncated`, which the theme styles. First
  sighting of "a fact becomes an attribute".
- ★ note: `truncate = {…}` is still a stopgap struct shape (q31), even
  though the expression language (§5f) now exists — it was built for
  relations (ch. 28), and computed fields haven't been moved onto it yet.
  So a `summary` field is still the struct form; don't over-invest in it.

### 8. Tags and archives for free
- `[routes.tag_index]` with `group_by = "tags"` + `path =
  "/blog/tags/{key}/"`.
- Any typed field groups; list fields multi-key.
- `title` / `crumb` are templates over group params (`{key}`, `{year}`,
  `{month_name}`, …).
- **Subdivision**: a grouped route whose `from` names another grouped
  route refines the partition — `yearly_archive` → `monthly_archive`, keys
  accumulate down the chain. No new keyword; the engine sees `from` points
  at a grouped route and subdivides.
- Breadcrumbs fall out of the nesting; nothing declares them (§5h/q46).
- **Pages can be grouped too now** (q51 — one row type): a `date` on a
  tree page means `group_by = "date.year"` works over it. Mention lightly;
  the payoff is ch. 21/23.
- Limit, stated plainly: **pagination × subdivision is refused** (q30) —
  and the config error says so.

### 9. Feeds, sitemap, and `from = "*"`
- `[routes.feed]` + `shell = "atom"`; `[routes.sitemap]` + `shell =
  "sitemap"`.
- Introduce `shell` in one sentence: *how the result is serialized*.
  Full treatment in ch. 19.
- `from = "*"` = every routable row.
- **The footgun, called out loudly**: a `from = "*"` route ranges over
  *routes*, and a routed row is routed whatever its flags say — so any new
  one must repeat `!draft && !hidden` or it leaks. Profiles (ch. 23) do
  not rescue this. The sitemap's own `where` is the worked example; the
  cautionary tale lives in ch. 23.

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

### 12. Parts, slots, and the reason there is no `{% if %}`
- **Layout kind = which parts. Theme = which arrangement. Shell = which
  serialization.** Three independent axes; state once, repeat as needed.
- A layout kind emits a *part map*, not a page. The four kinds are
  `document`, `listing`, `feed`, `raw`; the previews (`summary`) and
  sub-parts are their own kinds in the vocabulary (ch. 32c).
- **The kind is inferred, never declared** — a row/page → `document`, a
  grouped-or-paginated route → `listing`, a feed/sitemap route → `feed`,
  an opt-out row → `raw`. You do not write `layout: document`. (One row
  type, q51, is why a *page* infers `document` the same as a post does.)
- ★ Do **not** teach `layout:` as the way to pick furniture. It is a
  surviving Jekyll word now scheduled to dissolve into `shell:`
  (q33(f)): `page`/`post` are one value, and the `_layouts/*.html` it
  names have been unread since §5e. Teach `shell:` (ch. 19); mention
  `layout:` only in the migration note and the reference.
- **The rule**: want an `if` → you're missing a fact. Want a `for` →
  you're missing a view. Both are design bugs. The load checker is the
  tripwire.
- Evidence, briefly: of ~60 Liquid constructs on grack.com, 3 were
  genuine display iteration.

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
- **Choosing one**: engine loads every directory under `themes/` (skipping
  `_`-prefixed), compiles each `theme.scss` to `/css/<name>.css`; the one
  named `default` is site-wide and keeps `/css/main.css`. Theme is **per
  row** — front matter, or cascade it to a subtree with a rule default
  (`match = "recipes/**"`, `defaults = { theme = "terminal" }`).
  ★ A `[site] theme = "…"` key is specced but absent — today the
  directory *named* `default` is the mechanism, so "make this the site
  theme" means renaming a directory.
- Nice consequence of per-row themes, worth one line: a row's **body** is
  rendered through its own theme, not just its shell — so a `recipes/`
  subtree under `terminal` is terminal all the way down.
- **Subthemes ride after a colon**: `theme: "ledger:dark"` stamps
  `data-subtheme="dark"` on `<html>`; CSS subselects `[data-subtheme~=…]`.
  They compose — `marginalia:dark:wide`.
- **Recolour without touching a theme**: a site-owned root `.style.scss`
  setting `:root { --accent: … }` sits in a layer above theme CSS, and
  because the token names are a cross-theme contract it survives theme
  *switches*, not just updates. Cheapest real customization there is. ★
  (`.style.scss` overlays themselves are still specced — ch. 26.)
- Dogfood/tooling callout: `theme-preview/` is a site of structurally
  identical subtrees, one per theme, so `/ledger/notes/` and
  `/miroir/notes/` are the same rows in the same shapes — compare in two
  tabs. `grackle --config grackle/theme-preview/grackle.toml serve`.

### 14. Writing a theme: the hole algebra
- **A theme is only its differences.** It inherits the base; a fragment
  replaces the base's of the same name, and every kind you decline keeps
  the base arrangement. Three of the six gallery themes are four files.
- A theme is a directory of data: `_tokens.scss`, `theme.scss`,
  `shell.html`, `<kind>.html`, `<kind>--<variant>.html`. All optional.
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
- **An arrangement may decline a part; `canonical()` may not.**
  Completeness is the *parts layer's* obligation, not the theme's —
  `terminal` drops tags from its summary on purpose, a card is a jacket
  with no prose. The base's own exemptions are declared with reasons.
  Worth one worked example because it teaches rule 2's exact edge:
  `summary.src` is exempt because rule 2 deletes an element with an empty
  **content** slot, and an `<img>` has only *attribute* holes — so a plain
  summary trying to show a cover would emit a broken image on every text
  row.
- **The grouped-parts tax**: rule 2 deletes an empty part's element, not a
  wrapper *your fragment* invented. Group two parts in a meta bar and you
  pay `:not(:has(*)) { display: none }` — direct-child-scoped.
- Errors, shown: unknown slot (lists the kind's parts), flag-as-content
  slot, `data-fragment` on a scalar.
- Order note (narrow): unarranged kinds emit in `parts.toml`'s **declared
  order**, enforced at the schema position, not the order the engine
  produced them. Matters only for kinds nobody arranges.
- ★ `theme.toml` (head-fact selection, and `extends`) is specced, absent.
- ★ honest weakness: a *new* theme is data, but the part vocabulary is
  Rust — see ch. 33.

### 15. CSS does the geometry
- Slot names are the styling contract: `[data-slot=…]`, `[data-kind=…]`,
  `data-<fact>`. The renderer's classes are API, not implementation.
- Note the one recent rename: a relation group is keyed by
  **`data-relation`** (was `data-axis`), so per-relation CSS
  (`.relation[data-relation="related"]`) targets that. Translations, an
  *axis*, keep `data-axis`. (ch. 28 for the split.)
- **Canonical (unarranged) markup emits in `parts.toml`'s declared order**
  — the reading order a screen reader or the null theme sees. It's the
  schema's order, enforced, not the producer's incidental one (ch. 13).
- **The cascade order is declared in full: `@layer reset, base, theme,
  overlay, post`.** Only `base` and `theme` carry content today (`reset`
  ships inside `base`; `overlay` for `.style.scss` and `post` for per-post
  `<style>` are ★ unbuilt), but the order is stated now so those slot in
  rather than sorting last by accident. The point that matters: a theme
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

### 16. Variants: one kind, several looks
- **The preview kind is `summary`, full stop.** `figure` and `gallery`
  folded into it (2026-07) — one schema carries both the text-card fields
  (`title`, `date`, `content`, `truncated`) and the image fields (`src`,
  `width`, `height`). A card and a figure are the *same kind* wearing
  different fragments. This is the cleanest possible illustration of
  "kind = which parts, fragment = which look" and should be taught as
  such.
- `variant = "cards"` on a route → `summary--card.html`; resolution is
  `{kind}--{variant}` → `{kind}` → base → canonical. Real fragments in
  `field-notes`: `summary--card.html`, `summary--figure.html`,
  `listing--cards.html`, `listing--gallery.html`.
- **A variant a theme doesn't have degrades silently, and that's the
  design**: a row asking for `listing--cards` under a theme without it
  gets plain `listing`. Row-declared variants are *requests, not demands*
  — which is what lets any site render under any theme. The base ships
  `summary--figure` but no card/gallery variants, so four of the six
  gallery themes fall back in public; the preview site shows it side by
  side on purpose. (Contrast: a *fragment's own* `data-fragment=`
  override naming a missing fragment IS a load error — intra-theme, so
  correctly strict.)
- `data-fragment` as an explicit override on a stream.
- Galleries as the worked example (object rows, `order_by` required).
- ★ known silent failure: a variant fragment missing a hole drops that
  part with no warning (q45 leftover). Now understood as blocked, not
  merely unbuilt — a *deliberate* omission is byte-identical to a
  forgotten one, so the warning can't exist until a theme can say "I
  don't place this part" (q50). Document the symptom and the workaround
  (diff against canonical), not a promise.

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

### 18. Landings: a route owns the URL, a row may own the words
- A landing is a `[routes.*]` entry. Three tiers: bare (`title` only) →
  `intro = "…"` → `content = "path.md"` (mode B claim). `intro` XOR
  `content`.
- Mode B: the claimed row must place `{% view <owner> %}`, or the rows
  are unreachable — load error. (`{% view %}` still names the query; the
  keyword didn't change with the sets/routes split.)
- Per-key intros via `[records.<field>.<id>]` (`name`, `slug`, `intro`).
- The chain: URL nesting *is* parent derivation. Crumbs are climbed, not
  declared. `trail` remains only for group-key chains (q46).
- Dogfood callout: `/reference/` in this manual is a mode B landing.

### 19. Shells: how much wrapper the output wears
- Two scopes, same word, and the chapter must separate them in its first
  paragraph:
  - **Route shells** — `shell = "html" | "atom" | "sitemap" | "search"`
    on a `[routes.*]`: how a whole route is serialized.
  - **Row shells** — `shell:` in a row's front matter (built
    2026-07-19): how much wrapper *one page* wears.
  - These are genuinely two axes on one word — disjoint value domains,
    disjoint passes, a row never meets a route's shell. Say so once
    (§5g "one word, two axes").
- **The row tiers**, the chapter's centrepiece. `shell:` picks one of
  three; the fourth (`object`) isn't a `shell:` value because objects
  never enter the pipeline — worth showing all four so the head-size
  jumps are legible:

  | tier | selected by | head | body |
  |---|---|---|---|
  | `object` | extension (an asset) | — | bytes off disk |
  | `none` | `shell: none` | — | rendered parts, emitted verbatim |
  | `light` | `shell: light` | minimal (~85 B) | canonical parts, no theme |
  | `html` | `shell: html` / default | full (~739 B) | theme fragments |

- Closed vocabulary, checked at load and named with the file — a typo'd
  shell would otherwise render the wrong tier silently.
- **Correction worth stating** (the doc got this wrong twice, so readers
  will too): `light` is a *tier*, **not the null theme**. It bypasses the
  theme registry and takes a minimal computed head; there is no
  `themes/light/` directory. The null theme (ch. 13) is a *theme with no
  fragments* — full computed head, stylesheet link, everything but body
  chrome. And `shell: light` ≠ `theme: light`: a theme only chooses body
  chrome, never the head.
- **`none` is a capability, not a spelling**, and it's the chapter's
  worked example: an imported artifact (an old demo, a hand-built HTML
  page) can now carry front matter *and* still emit itself. Before, front
  matter nested the whole `<!doctype html>` inside a second document, so
  shipping it verbatim meant having no front matter — which meant it
  wasn't a row: no title, no metadata, invisible to every query. Now it's
  a row the database can see *and* a byte-exact artifact.
- Pair it with `hidden: true` — the honest way to keep an imported
  artifact out of the sitemap and the search index while keeping it
  linkable by source path (ties to ch. 10 and ch. 23).
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

### 20. The tree declares where, config declares what
- Marker files: `[markers] ".draft" = { draft = true }`, then `touch`.
- Same law as ch. 4, third appearance.
- What markers replace: `drafts/**` rules. What rules keep: routes, and
  patterns that cut *across* the tree (`**/*.scss`).
- Practical: hide a subtree from search with one `touch`.

### 21. Typed fields per subtree: `.schema.toml`
- `github_link = { type = "url" }`; types `string int bool list url image`.
- Buys **four** things now: front-matter validation, filter type-checking,
  slot/field checking, and — since the field-display arc landed — **the
  field renders**. A `list` field fills a stream of `item` parts, an
  `image` field fills a `url` part (thumbnailed like any object). So a
  typed field is presentation, not just a guard.
- Governed rows are strict (unknown key = load error naming the file);
  ungoverned rows stay tolerant.
- Worked example: `recipes/` with `course`, `time`; then group by it.
- ★ no list-of-records type, no JSON-LD emission (q40).

### 22. Hierarchy: the page's tree and the tree's tree
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

### 23. Drafts, hidden, and profiles
- The three flags, where they come from, what each means.
- `hidden` = routed but unlisted; `draft` = routed to `/drafts/{slug}/`.
- **Drafts live in `_drafts/`** — a *second source* for the posts table
  (ch. 5), not a second table: `[[collections]]` with `kind = "posts"`,
  `source = "_drafts"`, `filename_formats = ["{slug}"]` (a draft has no
  date until it publishes). Ordinary rows otherwise — routed, in the link
  graph, visible to the inspector — kept out of feeds and listings by the
  `!draft` filter the queries already carry.
- **Flags work on pages too** (fixed 2026-07-19) — same cascade as posts,
  front matter over marker/rule defaults. `hidden` reaches the row's
  route so star views filter it; `noindex` reaches the head. Worth a
  sentence on *why* this is called out: a page declaring `noindex: true`
  used to be accepted and silently dropped, which is the failure mode
  this whole system exists to prevent. Good place to teach "if a
  declaration seems ignored, `grackle explain` it."
- **Profiles are built** (the outline's loudest ★ closed 2026-07-19) — a
  projection, not a different database. A profile changes three things:
  which rows the queries admit (by patching a set's or a route's `where`),
  the output address, and a `data-profile` marker. `build` uses the
  default projection; `serve` defaults to `dev`. Full config below.

  ```toml
  [profiles.drafts]
  noindex = true                        # the whole projection, one key
    [profiles.drafts.sets.published]    # patch a set → patch a query
    where = "!hidden"
    [profiles.drafts.routes.search]     # patch a route → patch a landing
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

### 24. Bringing an existing site across

The migration chapter. Placed here because it needs routes (ch. 5),
shells (ch. 19) and flags (ch. 23) and nothing later. Written against
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
  `.noindex` marker does a whole subtree at once (ch. 20). This is the
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

### 25. Widgets, and the line at control flow
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

### 26. Blocks and rewrites
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

### 27. Per-post CSS
- ★ Entirely specced: a `<style>` block in the body, SCSS, compiled,
  cached, hoisted, auto-scoped, `style_scope: false` to opt out.
- Where CSS belongs, decision table: one row → per-post `<style>`; a
  subtree → `.style.scss` ★; the whole site → theme.
- Gotcha to document now because the failure is invisible: **scoped SCSS
  cannot declare `:root` custom properties**.

### 28. Relations: every neighbour list is a query
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
  over     = "published"        # candidate pool: a set, or a derived relation
  where    = "!(candidate in earlier) && !(candidate in later)"
  rank     = "embedding_similarity(self, candidate)"   # double, bigger wins
  min_rank = 0.4
  limit    = 4
  # also: match (glob — scopes self AND names the schema), label ("@ref")
  ```

  Pipeline per row: **`over → where → rank (+min_rank) → limit`**.
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
  (`over = "linked_from"`). Only a *declared* relation emits a group; a
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
  rows; an *axis* points at other **forms of the same row** (translations,
  thumbnails). Site-defined relations now stamp **`data-relation`** (the
  old `data-axis`, renamed in this change) — a deliberate theme-contract
  change. `translations` stays an axis and reaches the head as
  `rel="alternate" hreflang` (ch. 30).
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
- **The default pool won't leak drafts**: when `over` is omitted, the
  fallback is `[sets.published]` if you have one, else the collection
  filtered `!draft && !hidden`. State the rule crisply — **an explicit
  `over` is taken verbatim; only the default's fallback adds the filter**
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

### 29. Search
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
  actually discusses it. Relevant to anyone shipping `shell: none` rows
  or per-post CSS.
- Keeping things out of the index: `hidden: true` on the row, or narrow
  the route's `where`. The searchable set is a query, so this is the
  same lever, not a second one.
- ★ overlay strings not localized.

### 30. More than one language
- `[i18n] default`, `locales`, `selector = "suffix" | "prefix"`.
- The load-time split: everything downstream sees the **logical** path.
  i18n off is a byte-identical no-op.
- A translation is a row, not a site copy.
- The switcher is the `translations` **axis** (q53 — an alternative *form*
  of the row, not a relation to another row). Zero fragment changes for
  the body chip, **and** it now reaches the head as `<link
  rel="alternate" hreflang>` (built 2026-07-20) — the SEO-correct place, a
  fix over the earlier body-only behaviour.
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

### 31. The inspector: the database explaining itself
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
  two (ch. 30), a `from = "*"` route has 66 members and no row.
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
  — the best demonstration of what a projection *is* (ch. 23).
- ★ Honest edges: assets embedded in the binary (hacking the inspector
  needs a rebuild); route order is lexical, so the client owns display
  order (`/blog/page/10/` before `/blog/page/2/` otherwise).

*Exit check for Part III: a multi-section, multi-collection, searchable
site with typed content.*

---

## Part IV — Reference

Terse, generated where possible, no teaching. Each entry links back to
the chapter that teaches it.

### 32. Reference
- **32a. `grackle.toml`** — every key. `[site]`, `[[collections]]` +
  `[[collections.rules]]` (array-of-tables now; name defaults from the
  source dir), `[markers]`, **`[sets.*]`** and **`[routes.*]`** (the split;
  list the route-only keys vs the shared keys), `[sets.*.fields.*]`,
  `[profiles.*]` (+ nested `.sets.*`/`.routes.*` patches), `[widgets]`,
  `[records.*]`, **`[collections.relations.*]`** (`over`/`where`/`rank`/
  `min_rank`/`limit`/`match`/`label`), `[i18n]`, `[i18n.names]`,
  `[i18n.strings]`, `[shells.*]`, `[cache]`, `[static]`. (**`[related]` and
  the collection `adjacency` key are gone** — both dissolved into
  `[collections.relations.*]`. `[links]` is gone as a required
  block — strict is the default; `policy = "loose"` is the only reason to
  write one.) Mark built/specced per key. ★ flag the keys that don't mean
  what they say (`layout` on listings, `template` = "claims a file", row
  `layout:` deprecated toward `shell:`) pending q33 — replacement named in
  each case.
- **32b. Filter (`where`) language** — grammar, operators, truthiness
  table, the functions (`glob`, `under`), and the field vocabularies
  (row / object / route). Note `match` is a *separate* source-path glob,
  not part of `where`.
- **32c. Part kinds** — the table from `assets/parts.toml`; this *is* the
  theme API. Kind → parts → types (`text url html stream:<k> map:<k>
  flag`). Three things to state: it's **engine-owned data** (themes are
  checked against it; users don't edit it); the **declared order is the
  canonical fallback reading order**, enforced (so this table doubles as
  "what an unarranged kind renders like, and in what order"); and the
  merged preview kind — `figure`/`gallery` are gone, `summary` carries
  both card and image fields. Generate this section from the file, not by
  hand (see open question 8).
- **32d. Front matter** — reserved keys. One row type now (q51): `date`,
  `tags`, flags, `order`, `theme`, `shell`, typed fields apply to *any*
  row, not just posts. `shell:` (`none`/`light`/`html`); `layout:`
  deprecated.
- **32e. Tags in markdown** — `{% image %} {% view %} {% include %}`
  (parameterless) + widgets. `{% post_url %}` is **retired** — use an
  ordinary file-relative link. Unrecognised tags emit verbatim.
- **32f. CLI** — build / serve / query / explain / **urls** / diff, all
  flags. `--profile` is global.
- **32g. Error catalogue** — every load-time error, what it means, the
  fix. Sorted by message. High value: this is the page people land on
  from a search engine.
- **32h. Glossary** — row, collection, origin, **set**, **route**,
  landing, claim, part, slot, fragment, kind, variant, **row shell vs
  route shell**, **relation vs axis**, profile, projection, marker, scope
  chain, computed field.

---

## Part V — Understanding grackle

Optional reading. Explains the shape so users can predict behaviour
rather than memorize it.

### 33. What grackle is not
- Confirmed non-goals with reasons: comments, memberships/paywalls,
  ratings, live/external data, stateful interactive widgets as *modeled*
  content, control flow in templates, AST access, vector indexes.
- The honest workaround for each (edge/CDN for entitlements; ETL that
  commits data; raw passthrough + per-row assets).
- Says clearly: if you need these, use something else. That's fine.

### 34. Why it's shaped this way
- The four layers and their different rates of change.
- The recurring law, one more time, with all six of its appearances.
- Why load-time errors instead of 404s.
- The two honest weaknesses: themes need Rust for new part vocabulary;
  head facts aren't per-theme selectable ★.
- Pointer to `DESIGN.md` for anyone who wants the full argument.

### 35. What isn't real yet
- The ledger, in one table: `.style.scss`, `.slots/` typed fills,
  **authored `.rewrite.toml` rules**, **the notes/footnote stream +
  sidenotes**, per-post `<style>`, md shell, **computed fields on §5f**
  (the `truncate = {…}` struct is still the stopgap — the expression
  language exists now but this hasn't moved onto it), board kind, serve
  v2, pagination × subdivision, per-block facts, audio/video field types,
  faceted filtering, transclusion, profile `baseurl`, **the rest of the
  axis unification (q53** — thumbnails/md-twin as `rel=alternate`; locale
  hreflang already landed**)**, the route-token supply merge (q51's
  remainder — a post still can't route to an arbitrary path).
- **The whole theme-distribution story is specced** and deserves its own
  ledger block, because the built base makes it look closer than it is:
  `theme.toml` (both head-fact selection *and* `extends` inheritance
  chains), the `grackle theme` subcommand family
  (`add`/`update`/`list`/`new`/`derive`/`check`/`try`) with
  `themes/.lock.toml`, the `?theme=` dev override, and a `[site] theme`
  default key. Today: install is `cp -r`, "site theme" is a directory
  named `default`, and there is no update path. Also specced: the fuller
  cascade (`reset`/`overlay`/`post` layers — only `base, theme` ship).
- Each row: what it would look like, what blocks it, the q number.
- **Landed since earlier drafts** — out of the ledger, into the chapters
  named: row shells (ch. 19), page flags + **profiles** + **`_drafts` as a
  second source** (ch. 23), search-skips-markup (ch. 29), **the inspector**
  (ch. 31), **one row type / q51** (pervasive — pages carry dates, tags,
  flags, typed fields), **`[sets]`/`[routes]` split** (ch. 6), **strict
  links by default** (ch. 10), **q26 body-image dimensions** (ch. 26),
  **the narrow HTML-link rewrite** (ch. 26), **locale hreflang** (ch. 30),
  **declare-your-own relations + the §5f expression language / q52**
  (ch. 28 — it also built arithmetic, unary minus, the two-row function
  registry, and the `Double` type), **the base theme in the binary**
  (ch. 13/14 — this is the other big one: the null theme went from
  complete-but-unusable to a real default, and the gallery shrank from 109
  files to 32), and **syntax highlighting** (ch. 15 — built, unconfigurable,
  theme supplies the four token classes).
- **Still-open arrivals a reader would notice:**
  - q47 — no language switcher on listing routes (ch. 29).
  - q48 — `type:` as row data. Held until something other than the
    renderer consumes it; today the answer is subtree position +
    `.schema.toml`.
  - q49 — metadata for files that can't carry front matter (derive, then
    a `.p01.png.toml` sidecar). Ch. 23.
  - q50 — transplanting an imported page, and the blocked "deliberate
    omission vs forgotten hole" underneath it (ch. 16).
- Kept current or deleted. A stale version of this page is worse than no
  page.

---

## Open questions about the manual itself

1. **Chapter count.** 35 is a lot. Candidates to merge: 15+16 (CSS +
   variants), 26+27 (blocks + per-post CSS, both largely ★), 33+34.
2. ~~**Does Part I need a theme?**~~ **Answered by the engine, 2026-07-24:
   no.** The base theme ships in the binary, so ch. 3's first `serve`
   already looks like a real site with zero configuration. That was the
   single biggest structural worry in this outline and it's gone. Residual
   question: **should ch. 3 screenshot the base, or `cp -r` a gallery
   theme immediately?** Leaning screenshot-the-base — "you already have a
   site" is a stronger opening than "now install something."
3. **★-heavy chapters (26, 27) may be premature.** Option: hold them out
   of v1 and let ch. 35 carry them until §6d stage B fully lands.
4. **Release notes as posts** is the natural dogfood, but it means the
   manual has a publishing cadence. Acceptable?
5. **Where does the manual site live and deploy?** `grackle/manual/` in
   this repo, served at `grack.com/grackle/`? Own repo later?
6. **Ch. 23 (migration) is half ★ — does it ship in v1?** The built half
   stands on its own (the four tiers, `shell: none`, flags, `grackle
   urls` parity, wholesale legacy publishing). The unbuilt half is the
   ending: q49 metadata derivation, q50 transplant, q28 redirects — the
   questions a migrator asks first. Ship with the ★s loud, or hold until
   q49's cheap derive half (reading a `<title>` already there) lands.
7. **Does ch. 24 want a worked migration?** A before/after on a small
   imported tree would carry it better than prose. `field-notes` now has
   `demos/pane.html` (`shell: none`); a second row at the `light` tier
   would make the spectrum visible.
8. **Reference generation is now urgent, not nice-to-have.** This sync
   touched 31a (config keys), 31b (`where` functions), 31c (part kinds),
   31d (front matter) — every one of them churned under a refactor the
   manual didn't see coming. 32a/32b/32c/31d *must* be generated from the
   source (config structs, `assets/parts.toml`, the error enums) or they
   rot in a week, not a month. This is a `grackle docs` subcommand
   waiting to be specced — the strongest feature request to fall out of
   writing the manual.
9. **The vocabulary churned: `[views]`→`[sets]`/`[routes]`, `over`→`from`,
   `filter`→`where`.** Any prose already drafted against the old spelling
   is wrong. Worth a one-time grep gate in CI over the manual corpus for
   the retired spellings, since they read as plausible.
