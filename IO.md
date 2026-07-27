# IO.md — two databases: inputs and outputs

**Status: APPROVED TO LEDGER (2026-07-27)** — Matt greenlit serial
execution after the MERGE.md pipeline drains (F3 → G1 → G2 → batch
review 4). §10 is the work ledger; Matt may still edit the model sections
at any time and the ledger follows the document. Where this contradicts
DESIGN.md, this is the intended successor and DESIGN.md records what
shipped. Remaining **[open]** choices are settled in-item by the
propose-and-flag pattern (the executing agent proposes, records reasoning
in the ledger log, and Matt vetoes at review) unless marked Matt-only.
**Executing agents do not file background-task chips** (Matt, 2026-07-27):
an out-of-scope find goes into your §11 log entry and your report as a
proposed item — the orchestrator files it here, where it's sequenced and
reviewed like everything else. One ledger, no side channels.

The one-sentence model: **the site is two databases — the inputs you wrote
and the outputs it publishes — joined by a graph the build can hold in its
hand.** Everything else here is consequences.

## 1. The two databases

**Inputs**: rows, one per file the walk admits. Identity comes from front
matter — a literal block, or a sidecar file — and identity is a fact, not a
sorting hat: a file with identity is a governed row (schema-validated
fields, a place in the link graph); a file without is a row whose content is
its bytes. **One softening (Matt, 2026-07-27): an identity-less file that
rules send through a rendering shell becomes a *degenerate row* — a
warning, never an error — with a title implied from its slug** (the
engine-fallback rung: any rule default or front matter beats it). It
renders, it lists, it has a name everywhere a title is read; the warning
nudges toward a block without breaking a build. An identity-less file
routed `raw` is just an ordinary byte row — no warning, that's the normal
case. There is one walk. There is no tree machinery, no objects table,
no posts table — collections survive only as **named scopes**: a source
subtree plus its rules, extractors, schema and relations. "Posts" means "the
scope whose rules carry a date extractor."

**Outputs**: what the site publishes — one row per published artifact.
An output exists in two stages, and the law of the build is:

> **Facts at planning; content at materialization.**

An output's *facts* (url, shell, flags, provenance) exist the moment routes
are planned — before anything renders. Its *content* exists when a shell
materializes it. This is the staged-hydration law the inputs database has
always had (stat → front matter → body → rendered), applied symmetrically to
the other side. A query forces only the stages it reads.

**Build** is "pull every output." **Serve** is "pull this one" — the pull
model *is* the serve model, and on-demand rendering stops being a special
mode: it is an output whose content stage nobody had forced yet.

## 2. The join

The two databases join on explicit fields, in both directions:

| side | field | holds | cardinality |
|---|---|---|---|
| input | `output` | a record — the row's **canonical** output; `output.url` is its address; bare `output` is truthy iff the row lands anywhere | 0..1 |
| input | `viewed_by` **[open: name]** | every output that includes this row as a *member* (listings, archives, the feed) | 0..N |
| output | `inputs` | every input row that fed this output | 1..N |

Cardinalities, spelled: one input → one own output (a page); one input →
many outputs *viewing* it (the listings that carry it); one output → many
inputs (a listing's members). A static byte-copy is the degenerate case:
one in, one out, `output` set, no identity.

Consequences that fall out of the join rather than needing rules:

- **The old `rendered` boolean** is bare-`output` truthiness (house style:
  a bare field means "has one"). The name `rendered` retires.
- **Claimed rows** (a landing's content row) are visibly `!output` — the
  structural exclusion becomes a queryable fact.
- **Axis alternates** are *other outputs of the same input*. The axis
  design's sentence — "points at other forms of THIS row" — becomes
  literally true in the schema: a form is an output. The alternate set is
  already computed every build (it emits `rel="alternate"`); exposed as a
  derived name (**`alternates`**, beside `linked_from`/`ancestors`), it
  gives relation pivots row → alternate forms → `candidate.output.url` with
  no new syntax.
- **Arrangement vs citation**, which the backlink scanner learned the hard
  way (membership is not citation), becomes two honest fields: `viewed_by`
  is arrangement; `linked_from` stays citation.
- **`output.inputs` is the invalidation edge set.** The incremental-rebuild
  machinery's typed keys have been curating exactly these edges by hand;
  now they are a column. **[open: scope]** — does `inputs` hold member rows
  only, or the full row-level closure (referenced images, slot fills)?
  Lean: full row-level closure (it is what invalidation needs); non-row
  dependencies (theme files, config) remain the existing key types.

## 3. Facts replace `kind`

The route/row `kind` enum (`post`/`page`/`static`/`object`/`view`) was the
last table tag — a flattened product of independent facts, surviving from
before the one-store merge. It is deleted. The facts that replace it:

| fact | on | meaning |
|---|---|---|
| `front_mattered` | input | has identity — a block or a sidecar |
| `output` | input | lands somewhere (and where) |
| `shell` | output | the serialization it left through |
| `inputs`/`viewed_by` | both | the join (§2) |
| scope membership | input | which collection-scope admitted it (already a column) |
| view provenance | output | which view materialized it (already a column; "is this a view route" = the view column is non-empty) |

The old filters translate to what they always meant — with a
shipped/pending honesty marker per batch review I-A: the example sites'
search filters → `front_mattered` (**shipped**, I1); grack.com's search →
scope membership (**pending** I9's join); the sitemap → **pending, and
not a one-liner**: measured, grack.com's sitemap is *not* "the HTML
documents" and never was — it deliberately lists byte-copy `.html` files,
PDFs, static directory indexes and the `light_html` page (43 URLs beyond
`shell == "html"` today), so its honest future spelling is a disjunction
over shells + ext, or a declared byte change — Matt's call when it
arrives. The silent-empty-query knife (`kind == "posts"`, plural,
matching nothing forever) is dead already: I1's domain check (until I13
deletes the column outright).

Sidecars split identity from parsing, and that is a feature: a `.png` with
a sidecar is a governed row — schema-validated fields, alt text, a place in
the link graph — whose bytes are never parsed. Which facts a file has and
what the pipeline does with it are separate questions with separate answers.

## 4. Shells: one axis, two families

A shell is a function from content to final bytes. The tier ladder
(`none`/`light`/`html`) and the serializations (`atom`/`sitemap`/…) were
never two axes — the tier ladder was the document-shaped family of the one
axis, behind an artificial wall. The wall comes down. Two families, split
by arity, one input contract each:

| family | consumes | members | emits |
|---|---|---|---|
| **map shells** — applied per output | one output | `raw`, `html`, `light_html`, (future `md`) | one file per output |
| **fold shells** — sit on a query over outputs | a collection of outputs — the one shared projection (url, title, date, tags, facts, content where it exists) | `atom`, `sitemap`, `search`, `robots_txt`, script shells | one artifact |

Rules of the axis:

- **`raw` is the transparent shell**: it emits the output verbatim, no
  wrapper. It never parses and never needs to know whether a pipeline ran —
  the pipeline is upstream, producing outputs (processed body for
  identity-bearing inputs; bytes otherwise). Today's `none` tier, static
  passthrough, and object bytes are all this one shell.
- **`light_html` is the html shell with no theme root merged** — not a
  tier, not a null theme; one clause.
- **Identity is an input contract — softened to degeneracy** (Matt,
  2026-07-27): `html`/`light_html` *want* identity; an identity-less file
  they receive becomes a degenerate row (§1 — warn, slug-implied title)
  rather than an error. `raw` does not care; fold shells sit on views
  only. A row wearing `shell = atom` is still a load error naming what
  atom eats — arity is a hard contract, identity a soft one.
- **Defaults are declared, not built in**: the base config's rules carry
  `defaults = { shell = "html" }` on the front-mattered-page rule and
  `shell = "raw"` on the catch-all. The engine ships no shell opinion;
  `--effective` shows where every shell came from. Front matter, markers
  and rules cascade the field like any other (the machinery already exists
  and is typed).
- **A fold shell with no `from` reads all outputs.** `from = "*"` retires
  (hard cutoff). A fold's `from` may also name an inputs refinement
  (a set) — selecting inputs and following the join *is* selecting their
  outputs, so `[routes.feed] from = "published"` keeps meaning what it
  means. External/script shells are the one construct whose nature does not
  decide its database: they declare it (**`pulls = "inputs" | "outputs"`**).
- **The fold projection is the outputs schema** — not a convention imposed
  on five shells but what querying the table means. The script-shell
  payload (documented as "the projection the atom shell eats") stops being
  TEMP-by-declaration: versioning it becomes versioning the one contract.
- **Ordering falls out of the column rule, not strata**: the sitemap reads
  only facts, so it can list every output, fold products included; search
  reads content, so it sees only content-bearing outputs — by column
  availability, with no second pass and no stratification rule.
- **`robots_txt`** is a fold shell over output facts (Disallow lines from
  noindexed subtrees, sitemap pointer) — and it naturally sees the force
  block's rung-0 values, so the drafts profile flows into crawl policy for
  free. **[open]**: exact emission spec.

## 4a. Images: one input, many outputs

An image is an input row like any other (identity optional, via sidecar);
what is special about images is only that one input routinely fans to many
outputs — and the model already has the law for that (a form is an output):

- **The embed policy**: by default, an *embedded* citation (`<img>`,
  `<iframe>`, video — and generated affordances like a lightbox
  expansion) resolves to a **content-hashed address under `/static/`** —
  no config, the base ships the policy; immutable cache headers fall out
  of the URL shape, and identical bytes dedupe to one address by
  construction. Disabled, an embedded-but-unrouted asset is a load error
  naming the asset and the fix; subset, the policy carries a
  match/extension filter. **[open]**: the table's name (`[embeds]` vs
  `[static]`).
- **Authored links demand a route.** A markdown/`<a>` link to an asset
  resolves to its canonical routed URL, and an unrouted target is a load
  error with the fix spelled: "add a rule routing it (e.g.
  `route = "/{path}"`), or embed it instead." Bookmarkable addresses
  exist on purpose, every one declared.
- **Two address slots, not two routes**: an output's `url` is its
  canonical routed address; its **`strong_url`** is the hash address,
  present when the policy published it. When an untransformed embed
  shares bytes with a routed output, the hash address *is* that output's
  strong URL — embeds and affordances use it, authored links and
  `rel=canonical` use the canonical. A transformed embed's hash is the
  rendition's own (inputs + parameters), related to the original only
  through the graph. The "exactly one route" law is untouched: strong
  addresses are the content store made public, not routes.
- **The worked example** (the reason the cut is right): a page shows the
  thumbnail (hashed rendition) → expanding full-size in a lightbox uses
  the original's strong URL (no route needed) → the download link uses
  the canonical route, which the link checker demands — three URLs,
  three jobs, each citation form knowing its address kind.
- **The original, when it needs an address**: a declared route — the
  existing machinery, unchanged. A named rule pins one asset
  (`route = "/logo.png"`); a catch-all `/{path}` rule gives a whole corpus
  literal paths (grack.com's inbound-link parity becomes one explicit
  line in its own config). **Precedence: a routed output wins — citations
  link the declared address; the policy is the fallback for the
  unrouted.** The base's objects catch-all dies, and the base's "may not
  mint a URL the author did not ask for" rule gets strictly stronger:
  every human-addressable asset URL exists because a rule said so.
- **Renditions** (sizes, formats): more outputs of the same input,
  transform-bearing. **Parameters come from demand**: the citing edge
  (`{% image %}`, a body `<img>`) carries what it needs, the pull
  materializes exactly the renditions citations request, and an image's
  rendition set is the union of its consumers' asks. No config surface;
  eager rendition sets (srcset defaults) are a future opt-in on top.
- **The description page**: an image with a sidecar has identity, so it can
  wear an html output too — the "object's description page" from the old
  axis notes, landing free.
- Galleries that include it are `viewed_by`; documents that cite it hold it
  in their `inputs`; an image nothing cites and no rule routes eagerly
  never materializes — the pull model is the garbage collector.

**The hashing law** (required to keep §1's planning/materialization split
honest): a content-hashed URL hashes the **inputs plus the transform
parameters, never the output bytes** — the address must be computable at
planning, before any transform runs. Today's thumbnail cache already obeys
this (`blake3(image bytes + variant)`); the law codifies it.

## 5. The graph

Every shell has a concrete inputs → output mapping, so the build constructs
the full dependency graph **at planning time** — nodes from
facts-at-planning, edges from `output.inputs` — and detects cycles at load
(the relations precedent: dependency order or a config error, never a
render surprise). Four existing features become views of this one graph:

- **incremental invalidation** — the graph *is* the edge set the typed keys
  curated by hand;
- **`materialize_referenced`** (publish what the chrome cites) — a pull
  along edges;
- **serve** — walking the graph backward from one requested output;
- **relation/fold ordering** — topological order over the same nodes.

## 6. Themes: `root.html`, and no shell vocabulary

A theme ships **`root.html`** — a document-shaped file with a `<head>` and
a `<body>` — merged into the final HTML by the engine, which owns `<html>`
itself (lang, subtheme, profile, axis stamps) and the computed head (title,
charset, canonical, the config head tables, hreflang, the one stylesheet
link). A theme that ships no `root.html` inherits the base's; a body-only
`root.html` is exactly today's chrome fragment, so migration is mechanical.
The engine owning `<html>` is a *checked* claim, not an understanding
(**IR4**): a root that writes its own `<html>` or a doctype is a load error
naming the file and what to unwrap, because that wrapper hides the head and
the body from the split and turns the whole file into body chrome — the
fence below intact and bypassed, the theme's `<title>` shipping inside
`<body>` on every page.

**The head fence**: a theme's head may contain `<style>` and nothing else —
and even that is extracted into the site CSS at build (**built, I5**: it lands
in the theme layer of the theme's sheet, after `theme.scss`, compiled as SCSS
like every other file a theme writes; the head carries no theme styles and a
page keeps one stylesheet link). Everything else
(`<title>`, `<meta>`, semantic `<link>`, `<script>`) is a load error naming
the file and the element. The fence widens only when a real theme hits the
wall (`<meta name="theme-color">` is the known first candidate — the
allowlist principle is "presentational head elements," and it starts at
one).

**One CSS artifact.** All CSS — engine base, theme(s), site overlay,
extracted `root.html` styles, eventually per-post styles — is munged into
one engine-owned output; pages carry exactly one stylesheet link. Remote
fonts ride `@import` inside CSS. **The per-theme sheets ARE that chunking**
*(declared at I5)*: `/css/main.css` and `/css/<name>.css` are the one
artifact, chunked — a pure perf optimization the model never mentions, and
the reason the declaration cost no code. A page links one sheet and that
sheet is the whole cascade for that page, which is what "one artifact" means
from where a page stands.

**Multi-theme scoping** *(written at I5; the [open] it closes)*. A site with
several themes live — the theme axis publishes the same row under each — has
one artifact and many themes' rules inside it, so something has to keep
`ledger`'s `.title` off `terminal`'s pages. Three ingredients exist and the
answer uses two of them:

- **Chunking already scopes, and it is not the argument.** A page links its
  own theme's chunk, so today no page ever *receives* another theme's rules.
  That is an optimization doing a correctness job, which is exactly the shape
  this document exists to refuse — the model says one artifact, and the model
  must be sound when the chunking is turned off.
- **Per-theme sub-layers are the mechanism**: `@layer theme.ledger,
  theme.terminal, …`, declared once in chunk order, each theme's CSS in its
  own sub-layer. This is themes/DESIGN.md §3's nested-layer plan pointed
  sideways instead of down the `extends` chain — the same construct, ordered
  by theme rather than by ancestry — and it costs the emitter one declaration
  line. It settles *precedence* between themes deterministically without
  touching a selector.
- **The stamped root attribute is the scope**: `<html data-theme="…">`
  beside the existing `data-subtheme`, with each theme's CSS emitted under
  it. Layers order rules; they do not stop a rule from matching, so a
  merged artifact still needs the selector to say which pages it is about.
  The stamp is the one fact a page always carries about its theme, and
  `[data-subtheme~="…"]` already proves the technique. **The cost is
  honest and worth stating**: prefixing every theme rule with an attribute
  selector is a transform on theme CSS the engine does not do today, and it
  raises every theme rule's specificity uniformly — which is survivable
  precisely *because* the sub-layers, not specificity, decide the
  cross-theme case.

**No new mechanism, and no new authoring surface**: a theme writes what it
writes now. Both ingredients are emitter-side, land when merging is actually
built, and are inert until then — which is why I5 declared the chunking
rather than implementing the merge.

**What this implies for the `extends` chain** *(restating I4's flag 3)*: a
theme root's head `<style>` is not part of the fragment chain — `split_root`
runs per theme, before the merge, so a child may shadow the chrome, the head
style, or both, independently. The scoping model above says the same thing
one level up and settles what "independently" means for CSS: a chain member's
CSS — `theme.scss` and root-head style alike, in that order — occupies **one**
sub-layer, `theme.<member>`, ordered root-first. So a child's head style
outranks its parent's `theme.scss` by layer rather than by specificity, and
`revert-layer` walks the chain one step at a time; the two halves of one
member's CSS are ordered against each other by source position inside that
member's sub-layer, which is the ordering I5 landed. Shadowing (fragments,
by name) and ranking (CSS, by layer) stay separate questions with separate
answers, which is what makes "the two halves shadow independently" safe to
say.

**Vocabulary**: "shell" does not appear in a theme. The chrome part
contract (nav, site_title, axes, main, copyright) renames `shell` → `root`;
`data-kind="shell"` follows (or drops). The word shell then means exactly
one thing in the whole system: the serialization a route leaves through.

## 7. What dissolves

| dies | survives as |
|---|---|
| the `kind` enum | facts (§3) |
| the objects table | rules routing by extension + `raw`; name index and dimensions keyed off extension |
| the tree machinery | the one walk + the front-matter fact + rules |
| the `none` tier / static passthrough / object bytes | `raw` (§4) |
| `from = "*"` star views, the second pass | fold shells over outputs; the column rule (§4) |
| the row-shell vs view-shell wall ("one word, two axes") | one axis, two families |
| membership-by-precedence ("every file belongs to exactly one table") | first rule wins — already the law |
| `theme: shell.html`, the pending `head.html` fragment | `root.html` (§6) |
| collections as kinds | collections as named scopes |

## 8. Migration ladder (parity-gated, each step shippable)

1. **Expose the facts** — `shell`, `front_mattered`, the `output` record as
   filter columns; migrate the corpus's `kind ==` filters to what they
   mean. Small; kills the silent-empty-query class.
2. **Unify the shell axis** — one vocabulary, one validator, the
   family/arity checks; `light` → `light_html`; rules gain shell defaults;
   the wall comes down.
3. **`root.html` and the megacss** — themes migrate mechanically
   (body-only roots first), the head fence lands, CSS assembly unifies.
4. **Dissolve tree and objects into rules over the one walk** — the
   front-matter gate becomes the fact; extension selection becomes rules;
   extractors move per-rule (the known one-row-type remainder); sidecars
   land.
5. **The join fields and the graph** — `output`, `viewed_by`, `inputs` as
   columns; the planner builds the graph; invalidation rides it; serve
   pulls.
6. **Delete `kind`** — by now unread; remove from schema, inspector,
   export.

## 9. Open questions

1. **[naming]** `viewed_by` vs `views` for the input-side membership list
   (`views` collides with the query vocabulary).
2. **[scope]** `output.inputs`: member rows only, or full row-level closure
   (lean: closure).
3. **[spec]** `robots_txt` emission details.
4. ~~**[design detail]** multi-theme CSS scoping in the one artifact~~ —
   answered at I5: §6's multi-theme scoping paragraph (per-theme sub-layers
   for precedence, the stamped root attribute for scope; both emitter-side,
   both inert until merging is built).
5. ~~this document's name~~ — settled by use: `IO.md`.
7. **[shape]** renditions in the shell axis: a transform-bearing output
   (resize, re-encode) is map-shell-shaped but parameterized — whether
   that's a parameterized shell (`image:256w`), a distinct transform stage
   upstream of `raw`, or purely edge-carried demand with no named surface
   at all (the §4a lean) wants one decision when the migration reaches it.

## 10. The ledger

Execution begins when the MERGE.md pipeline drains. **MERGE.md §4's
process rules bind verbatim** (one fresh Opus agent per item, serial;
pathspec commits to master; never bare `git stash`; never touch
`manual/OUTLINE.md`; `cargo fmt --check` clean under the pin;
mutation-check every guard; retired spellings are HARD CUTOFFS (no
teaching errors until Matt says otherwise — no site ships); corpus
migrates in-commit under byte-parity gates). IO-specific additions:
every item updates DESIGN.md where it makes a section false (this
document must not create the doc-rot it was born from), and every item
notes its **[open]** resolutions in §11's log. **Migration sweeps grep
`[profiles.*]` overlay bodies too** — E2's atom law duplicated view
definitions into profile overlays (grack.com's drafts profile carries
`kind ==` twice and `from = "*"` three times), and while E2's every-load
dry run catches *syntactic* misses there (retired keys), a semantically
valid unmigrated copy will not fail — the grep is the guard (batch
review 4). Fable batch reviews at the
marked points; findings append to §11 and may file R-items.

### Phase I-A — facts beside the fossil

- [x] **I1. Expose `shell` and `front_mattered` as filter columns**, on
  the schemas where each is answerable; migrate the corpus's `kind ==`
  filters to what they mean (search routes; anything else grep finds);
  give the surviving `kind` column **enum value-domain checking** (a
  comparison against a value outside post/page/static/object/view errors
  naming the knowns) so the fossil is safe while it dies. Parity.

- [x] **I2. One shell axis.** Merge the row-tier and view-serialization
  vocabularies into one schema-typed `shell` field with one validator;
  `light` → `light_html` (hard cutoff); the family/arity checks
  (map shells on rows and per-member routes; fold shells on views only;
  identity required for the html family); base-config rules gain explicit
  shell defaults reproducing today's implicit behavior exactly. Parity.

- [x] **I3. `from = "*"` retires.** A fold shell with no `from` reads all
  outputs (at this stage: the route set — the facts half already exists);
  the star spelling is removed (hard cutoff); a fold's `from` naming a set
  selects those inputs' outputs through the join. **Sequenced after
  MERGE R6, and states its rung-0 side explicitly**: fold selection —
  whether over the output pool or through an inputs-set join — sees
  forced fields per R6's unified law, so a forcing profile legitimately
  changes fold membership (the robots_txt design depends on exactly
  this). Parity.

*→ Batch review I-A.* ✓ done — findings in §11; verdict: sound, I-B clear.
Precedent written into law per its finding 5: **a retired value gets one
targeted sentence only where the generic diagnosis misleads** (I3's star
message is the exemplar; don't stretch it). One follow-up item:

- [x] **IR1. Three small strictness closures from review I-A.** (a) A
  registered `[shells.*]` script shell with no `from` is fed `rows: []`,
  not the route pool — the silent-empty disease one family over (proven
  live: `[shells.echo]` + a from-less route publishes an empty payload).
  Until `pulls =` lands (§4), reject absent-`from` on script shells: load
  error requiring an explicit `from`. (b) `check_domain`'s message tail
  says "can only ever be false" — wrong for `!=` (only ever *true*); one
  clause in filter.rs. (c) A routeless fold (`[sets.x] shell = "sitemap"`)
  dies late with "needs a route" — add the config-time companion check
  ("a set may not wear a fold shell"), F3's family. Mutation-check all
  three; parity; fmt clean.

- [x] **IR2. `grackle explain` hardcodes `kind post`** *(Matt, from the
  I2 agent's filed chip)*. `main.rs`'s `Query::Explain` arm prints
  `println!("kind post")` — a literal, ~line 486 — for EVERY row: tree
  pages, byte copies, objects (`grackle explain /humans.txt` says
  "kind post"). Pre-existing, verified against HEAD and current tree.
  Decide and implement: (a) print the real kind (the route's `RouteKind`
  via `db.routes` — a row itself has no kind field), or (b) delete the
  line — I13 deletes the enum outright, and a debug surface that lies is
  worse than one that is silent; per §3 ("facts replace kind"), (b) may
  be the honest answer — if so, say so, and consider printing the real
  facts (`shell`, `front_mattered`) in its place since they exist now.
  CLI-only surface, outside the byte-parity gate — but check whether any
  test or fixture asserts on the line first. MERGE.md §4 rules bind;
  test any guard added.

- [x] **IR3. Fix explain's doubled `layout` line** *(Matt, absorbing the
  IR2 agent's chip — runs after I4)*. `grackle explain <url>` prints the
  `layout` line twice for every row that has one. Cause: the
  `Query::Explain` row branch prints `layout` as a named line
  (`println!("layout {}", r.layout.as_deref().unwrap_or("-"))`), then the
  generic `for (name, value) in &r.fields` dump prints it again — `layout`
  is one of C1's four cascade keys declared in base `[schema]`, so it is
  also a declared column in `Row.fields`. IR2 handled the same shape for
  `shell`: the named line is authoritative (it answers even when the row
  resolved no value, which the dump cannot), and the dump `continue`s on
  the name. Apply the same `continue` for `layout` — one line beside the
  existing `shell` skip. **Check first whether `theme` has the same
  shape** (the third cascade key; currently printed by NEITHER path,
  which may be its own small gap — if so, give it the IR2 treatment: a
  named line + dump skip, recording the decision). No test or fixture
  asserts on explain's output (verified by grep during IR2); CLI-only,
  outside the byte-parity gate, but run the standard parity anyway.
  MERGE.md §4 rules bind; extend IR2's `io_explain.rs` test to pin the
  single-print (mutation: remove the continue → doubled line → red).

### Phase I-B — themes

- [x] **I4. `root.html`.** The binder accepts a document-shaped theme root
  (head + body); the head fence (style-only, load errors naming the
  element); `shell.html` migrates to body-only `root.html` across the
  base theme and gallery (mechanical); the chrome part kind renames
  `shell` → `root`; the `data-kind` stamp follows. Parity except the
  stamp rename, declared — **and per review I-A: the stamp rename touches
  the `<html>` tag of every page of every site**, so parity diffs modulo
  that one attribute and every HTML fixture re-blesses (verified ahead:
  no theme CSS/JS selector keys on `data-kind="shell"` today). Decide
  what a stale `shell.html` gets post-rename — silence would be silent
  chrome loss; a load error naming `root.html` is the house answer.

- [x] **I5. Head-style extraction into the existing CSS assembly.** A
  theme root's `<style>` lands in the theme layer of the existing
  per-theme sheets — which are hereby *declared* to be the megacss's
  chunked implementation (no URL changes, no assembly rewrite; the model
  changed, the bytes did not). The multi-theme scoping paragraph gets
  written as part of this item's doc updates. Parity.

*→ Batch review I-B.* ✓ done — findings in §11; verdict: sound, I-C clear.
Two follow-up items (small; run before I6):

- [x] **IR4. `split_root` closes the wrapper hole.** *(Review I-B findings
  1-2 — holes in I4's new guarantee, both probed live.)* (a) A top-level
  `<html>` (or doctype) in a theme `root.html` makes `wrapped` false, so
  the WHOLE document becomes body chrome — `<title>My Theme</title>` and
  metas ship inside `<body>` of every page, silently defeating both I4
  invariants; the most natural authoring mistake (pasting a full
  skeleton). Load error: "the engine writes `<html>` itself; unwrap to
  `<head>`/`<body>`". (b) Non-whitespace top-level TEXT beside
  head/body is silently dropped (the `else continue` is right for
  whitespace/comments, wrong for authored words) — error like the
  element case. (c) Rider: the top-level-`<style>` sibling error says
  "move it inside `<body>`" — for a style the right advice is the head.
  Mutation-check each; parity (all nine corpus themes are bare
  fragments — verified unaffected).

- [x] **IR5. The tokens warning stops lying twice, and §6c stops
  contradicting.** *(Review I-B findings 4+9.)* (a) `css_pass`'s
  "`_tokens.scss` that nothing imports" warning is false in two shapes:
  a tokens-only theme (the tokens ARE the sheet — pre-existing, GRAVEYARD
  ~138 era), and post-I5, a theme whose `root.html` head style imports
  tokens while `theme.scss` doesn't (the check reads only the
  theme.scss pass's `seen`; the head-style pass uses a fresh vec). One
  fix: consult both passes' `seen`, treat tokens-only as self-importing.
  Both shapes verified live; mutation-check. (b) DESIGN.md §6c (per-post
  styles, unbuilt) still says a row's `<style>` is "hoisted into
  `<head>` inline" — the opposite of the one-artifact rule and the
  `post` layer both CSS docs declare. One supersession sentence
  ("pre-IO prose; IO.md §6's one-artifact rule and the declared `post`
  layer govern; the decision belongs to the per-post-CSS builder") — do
  not decide the substance.

### Phase I-C — the single walk

- [ ] **I6. Extractors move to rules** (the one-row-type remainder):
  `filename_formats` per-rule, one route-token supplier offering path
  tokens always plus extractor results. Parity.

- [ ] **I7. The front-matter gate becomes the fact; tree and objects
  dissolve into rules over one walk.** Extension selection becomes rules;
  collections become named scopes; the membership-precedence machinery
  retires in favor of first-rule-wins. **Degenerate rows land here**
  (Matt's ruling, 2026-07-27, answering I1's flag): an identity-less file
  under a rendering shell renders as a degenerate row — warn, title
  implied from its slug at the engine-fallback rung. **Premise corrected
  by review I-A**: the caret draft ALREADY renders a slug-derived title
  today (the posts loader's pre-existing fallback, `slug.replace('-',
  " ")` — load.rs ~548), reaching `<title>`, `og:title`, the doc header,
  and the drafts-profile `search.bin`. So: re-measure on arrival; pin the
  engine-fallback rung's derivation AGAINST the existing loader behavior
  (same string → the parity exception is vacuous; any different
  de-hyphenation moves bytes on surfaces beyond the row's own pages);
  the new degeneracy WARNING changes stderr on grack.com builds — declare
  it to the parity method. **Two additions from review I-B**: (a) the
  single walk currently admits a site-root `themes/` directory as byte
  rows (probed: a minimal site publishes `/themes/mine/root.html`
  verbatim; corpus sites dodge it only via `exclude`/.gitignore) —
  decide explicitly whether theme sources are content; (b) dissolving
  objects makes rule defaults land on former-object rows, collapsing the
  842-row `shell` Null shape (I2's log) — the deferred sitemap/search
  filter migration becomes POSSIBLE here; per §3's marker the sitemap's
  honest spelling is Matt's call, so state whether I7 takes the
  migration or leaves it flagged. **Split on arrival** — the
  executing agent proposes the split as sub-items before starting, and
  the orchestrator sequences them. Parity throughout.

- [ ] **I8. Sidecars.** Identity from a sidecar file; governed rows for
  unparseable bytes; the identity/parsed split holds (`front_mattered`
  without content). Parity (no site uses one yet — fixture-driven).

*→ Batch review I-C.*

### Phase I-D — the join and the graph

- [ ] **I9. The join fields.** `output` (record; canonical), `viewed_by`
  **[open: name — propose-and-flag]**, `inputs`; the `alternates` derived
  name; claimed rows visible as `!output`. Parity.

- [ ] **I10. The graph.** Planner builds nodes/edges upfront; cycle
  detection at load; invalidation keys derive from edges; serve becomes
  the pull (on-demand = unforced content stage). Parity + the serve
  behavior tests.

*→ Batch review I-D.*

### Phase I-E — assets and the end of `kind`

- [ ] **I11. The embed policy and strong URLs.** `/static/` hashed
  default for embedded citations (**[open: table name —
  propose-and-flag]**); disable/subset; authored links demand routes with
  the fix-it suggestion; `strong_url` beside `url`; the untransformed
  twin rule; the `{hash}` route token; the base's objects catch-all dies;
  grack.com gains its explicit parity rule. The hashing law
  (inputs + parameters, never output bytes) stated in code. Parity for
  grack.com by its declared rule; minimal/examples adopt the new default.

- [ ] **I12. Renditions formalized as demand-driven outputs** — the
  citing edge carries parameters; the thumbnail machinery becomes the
  first transform; §9's rendition-surface **[open]** settled here by
  propose-and-flag. Parity.

- [ ] **I13. Delete `kind`.** By now unread: out of the schemas, the
  inspector, the export. The enum survives internally only if something
  structural still wants it — the item measures and says. Parity.

*→ Final IO review, whole-ledger, MERGE.md-final-review style.*

## 11. Ledger log

*Executing agents and batch reviews append here, MERGE.md §6 style.*
6. **[downstream]** `manual/OUTLINE.md` teaches several constructs this
   design retires (`bucket` already does not parse; `kind`, star views,
   tier vocabulary will follow) — the manual re-write rides the migration,
   Matt's pen.

**2026-07-27 — I1.** Landed as one commit. Two facts, one domain, two of the
four corpus `kind ==` filters migrated — and the two that stayed are the
item's most useful output.

*`shell` needed no code, and that was measured rather than assumed.* MERGE.md
C1 declared the engine's four cascade keys in the base `[schema]`, and a
declared field is a column: `Schemas::declared()` feeds both
`row_filter_schema()` and `declared_schema()` → `route_schema()`, and the
values live in `fields` as well as on the row's named field. Probed on a temp
site: `where = 'shell == "light"'` parses and selects, on rows and through
`Route.fields` on a star view. So §8 step 1's `shell` half was already
shipped; what I1 adds is the sentence saying so (DESIGN.md §4e) and the two
**[open]-adjacent** gaps, both **waiting on I2** and both probed, not
reasoned:

- **the row column holds the tier vocabulary, and only where someone wrote
  it.** `shell == "html"` selected **0** routes on a site whose every page is
  html — absent is Null and Null matches nothing. §3's target spellings
  (sitemap and search as `shell == "html"`) are therefore NOT reachable until
  I2's rules carry explicit shell defaults onto every row. Not forced here:
  materializing a default to make a filter read nicely, before the item that
  owns the defaults, is how a fact becomes a fiction.
- **a view route's serialization is not in the column at all.** `[routes.feed]
  shell = "atom"` is a route declaration, not a row field, so `shell ==
  "atom"` selected **0**. Merging the two vocabularies is I2's whole brief, so
  it waits there rather than half-landing here.

*`front_mattered` is identity, and the decision worth vetoing is that it is
NOT `rendered`.* The brief allowed "today it equals was-parsed"; the corpus
says otherwise. `_drafts/caret/why-is-a-cursor-called-a-caret.md` is a `.md`
in a posts scope with **no `---` block**: the scope hands it a date, a slug
and a route, so `rendered` is true, and the author wrote no identity, so
`front_mattered` is false. Defining the column as "was parsed" would have made
the name lie about a real row on the site the ledger exists to serve; defining
it as the block keeps the name honest and makes the disagreement *visible*,
which is the whole method. §3's table calls the fact "has identity — a block
or a sidecar", and I8 widens it to the sidecar without changing this bit's
meaning. **Decided and recorded** per §10's propose-and-flag: exposed on the
row schema AND the route schema (the migration target is a star view over
routes, so the route side is not optional), and a view route answers
**`false`, not Null** — it has no source file, so it carried nothing, and a
fold over the route pool needs a predicate that is total rather than one that
says "not applicable" to every listing.

*The two that migrated, and the two that did not.* `examples/field-notes` and
`theme-preview` moved `(kind == "post" || kind == "page")` → `front_mattered`:
same set, byte-identical `search.bin`. **grack.com's two did not** — the
`[routes.search]` and its full restatement inside `[profiles.drafts]`, which
the §10 grep is exactly for. There `kind == "post"` means *the blog corpus*,
which is **scope membership, not identity**, and `front_mattered` is the wrong
migration twice over: it would admit every page under `/code/` and `/writing/`
(a deliberate index change the config already declines), and it would drop the
blockless draft above. The honest spelling is a scope column on the output
side, which the join brings (**I9**); a `collection ==` column on routes was
considered and rejected as contortion — it would trade a domain-checked column
for an unchecked one, i.e. re-mint the silent-empty knife under a new name.
Both configs now carry the reason at the line.

*The domain check: general, because kind-specific was the more expensive
mistake.* `filter::Type::Enum(&'static [&'static str])` — a string column that
knows its values. Every type rule in the checker compares `.scalar()`, so an
enum behaves as a `Str` in ordering, concatenation, `in`, arity and mismatch
messages; the one thing it buys is `check_domain`, which fires on either side
of a comparison against a string literal outside the set. Kind-specific was
the alternative and was worse in the way that matters: `grackle-db` cannot
know what a route kind is, so the check would have had to live at each of the
three sites that parse a route filter (`views.rs`, `config.rs`'s profile
pre-check, `debug.rs`) — three chances to forget one, and forgetting is a
guard that is silently absent. Riding the schema means it is checked wherever
a filter is parsed, forever, and it is the mechanism §3 wants later. Cost: one
enum variant and ten `.scalar()` calls.

*Mutations, each restored.* (a) Delete either `check_domain` call: the db unit
test fails, and — probed on a temp site with the release binary — `kind ==
"posts"` **builds clean and selects zero URLs**, which is the disease exactly.
(b) Posts loader `front_mattered: raw.front_mattered` → `true` (the value
`rendered` takes there): the blockless post joins the identity set. (c) Tree
loader → `false`: `/about/` leaves it. (d) Drop the route-carry line: the
identity set empties. `RouteKind::NAMES` is a second spelling of the enum, so
a test holds it: its `match` stops **compiling** the day a variant is added,
because a domain that is wrong (rejecting a real value) is worse than one that
is absent.

*Parity.* Five sites plus grack.com `--profile drafts`, built from a `git
worktree` of HEAD with its own release binary and from this one, against the
same content tree (caches seeded so the only variables were binary and
config) — byte-identical but for each feed's wall-clock `<updated>` (6
atom.xml files, one line each), stderr identical for all six. `cargo test`
green; `cargo fmt --check` clean under the pin; `cargo clippy` warning set
identical to HEAD's; zero re-blessing beyond one new `expected-error`
fixture.

*For batch review I-A.* The `front_mattered`-vs-`rendered` split is the one
call here that a reviewer might want reversed, and the blockless post is the
evidence either way. It also lands a question in **I7**'s lap ("the
front-matter gate becomes the fact"): under §1 a file without a block has no
identity, so I7 must decide what a blockless `.md` in a posts scope *is* — a
governed row by scope, or bytes. Today it is a full post, and the answer moves
grack.com's output.

**2026-07-27 — I2.** Landed as one commit. One vocabulary, one validator, four
arity checks — and the corpus migration turned up the parity trap the brief
predicted, on exactly one row of one site.

*The merge, and what made it cheap.* `crates/source/src/shell.rs` holds the
whole axis: `MAP = [raw, html, light_html]`, `FOLD = [atom, sitemap, search]`,
plus every `[shells.*]` name, and four entry points (`check_row`,
`check_axis_value`, `check_view`, `check_registered_name`). The two checkers it
replaces were 6 lines in `load::cascade` and 8 in `Config::check`, and neither
knew the other's words — which is why "one axis" cost a module rather than a
refactor. **Arity is what separates the families**, not subject matter, and
each direction now says a sentence the old pair could not: a row wearing a fold
is told what that fold *eats* (§4's own sentence, now `"it eats a feed's worth
of entries … a row is ONE output"`), and a view wearing a map gets an **arity**
error rather than "unknown shell" — `html` is a perfectly good shell that
happens to wrap one output, and the old message could not tell a typo from a
category mistake.

*Two checks nobody asked for, one of which was load-bearing.* An `[axes.*]`
whose `field = "shell"` declares the serializations its members leave through —
and **those values had never been checked anywhere**: they do not pass through
a row's cascade, `build.rs` reads the member's value directly, and the `axis`
fixture's `light` would have gone on rendering the *fallback* tier in silence
the moment the vocabulary moved. That is the exact disease `cascade`'s check has
always existed to prevent, on the one path that never went through it. The
other (a script shell may not take a built-in's name) is cheaper: `check_view`
answers from the built-in vocabulary first, so `[shells.atom]` would be a
command nothing could ever run.

*The parity trap, sprung.* `demos/mindstorms/index.html` on grack.com carried
`layout: light` and no shell — so it reached the light tier through
`build.rs`'s legacy `_ => Theme::parse(layout)` fallback. The base's new
front-matter-rule default reaches it (rule defaults accumulate from every
MATCHING rule, not only the one that wins the route — grack.com's own rules
prepend but declare no shell, so the base's still land), which would have made
it `shell = "html"` and flipped it to the full theme: a real byte change on a
published page. Migrated in-commit to `shell: light_html`, which is what
`layout: light` always meant. It is the only row on any of the six trees that
sprang the trap, and it was found by grepping the corpus for `layout: light`
*before* the default was written rather than by the diff afterward.

*The index rule declares no shell, and that is the decision worth vetoing.*
The brief named three rules; the base has four. `**/index.{html,md}` routes
front-mattered pages and byte copies alike (grack.com has ~10 blockless
`index.html` files under `demos/`, `writing/school/` and `code/legacy/`), so
`html` there would be false for half its rows and `raw` false for the other
half. It needs neither: **a rule's defaults apply wherever it MATCHES**, and
`**/*.{html,md}` (front-matter-gated) and `**/*` both match an index file too —
so a front-mattered `index.md` takes `html` from the second rule and a static
`index.html` takes `raw` from the third, each by the same front-matter gate
that decides everything else about it. Probed on a temp site with the HEAD
binary before relying on it. This is also why I1's refusal generalizes: the
alternative was to declare `html` on the index rule and let it be true after
I7 makes those files degenerate rows — a fact that is a fiction until then.

*How a view route answers the column.* `views::view_fields` mints it:
`v.shell` if declared, else `"html"` — the serialization it left through, which
is what §3 says the fact is. `build_star_views` gained the same call (a star
route carried **no fields at all** before, `noindex` included). A per-member
route corrects it in `load.rs`'s route constructor: a member of an axis over
`shell` IS a different serialization of the same row, so `/tiers/light_html/`
answers `light_html` while its row answers `html`. Only `shell` is corrected —
`theme`, the other axis field, has no reader on the route pool to lie to.
Storage stayed `Route.fields` rather than becoming a first-class column, on
purpose: rung 0's `force_route_fields` writes `fields`, so a first-class field
would have shadowed a forced value and re-opened the seam R6 just closed.

*What `shell == "html"` selects now, and what it does not.* On the io_shell
probe site: every post, every front-mattered page (`index.md` included), and
every listing route — with the feed answering `atom` and the sitemap probes
answering `sitemap`, so each probe is correctly absent from its own result.
**Two shapes still answer Null**, both recorded rather than papered over:

- an **objects-collection row**, which never takes a rule default at all — the
  loader builds it from `Default::default()`, no cascade runs, and a `defaults
  = { shell = "raw" }` on the base's objects rule would be read by nobody. A
  test asserts this (and the mutation for it is *adding* that default, which
  moves nothing);
- a row governed only by a rule that declares no shell — an `extends = "none"`
  site's, or the caret draft's collection (grack.com's `_drafts` is a
  `source:_drafts` collection of its own, so it pairs with nothing in the base
  and inherits no posts rule).

*Which is why the sitemap and search filters did NOT migrate — deferred to I3
or later, per the brief's instruction to migrate only on proven parity.* The
sitemap's `dir || ext == "html"` and `shell == "html"` are **not the same set**
today: the sitemap lists directory URLs whose rows are objects or otherwise
shell-less, and it excludes nothing on the grounds of a shell. Rewriting it
would have changed grack.com's `sitemap.xml`, which is a live artifact. The
honest sequence is: I3 (`from = "*"` retires, folds read the output pool) and
I7 (objects dissolve into rules over one walk, at which point an image takes a
rule default like everything else) make the two sets converge, and the
migration is one line then.

*`theme-preview` needed nothing*, and its config comment stayed true: it
declines the base, declares no `shell` in `[schema]`, and no row or rule of its
sets one. `examples/raw` took the three `defaults` lines (it is the base
printed, and a test holds the two to the same URL set).

*Eleven mutations, each restored, each red.* (1) delete `check_row`'s call in
`cascade`; (2) drop `check_row`'s fold arm — the value is still rejected, by
the *wrong sentence*, which is the diagnosis this item exists to fix; (3) drop
`check_view`'s map arm; (4) delete the axis-over-shell loop; (5) delete the
registered-name loop; (6) delete the base front-matter rule's default —
`/about/` and `/guide/` leave the html set **while still rendering as themed
HTML documents**, the fact going quiet while the bytes do not move; (7) delete
the catch-all's — the raw set empties; (8) delete the posts rule's — both posts
leave; (9) `view_fields` stops minting `shell` — every listing leaves; (10) a
star route carries no fields — the four probes land in the `!shell` set; (11)
the member correction stops correcting — the light member answers its row's
`html` while rendering the light tier.

*Parity.* Five sites plus grack.com `--profile drafts`, built from a `git
worktree` of HEAD with its own release binary and from this one, into separate
trees, caches seeded so binary and config were the only variables —
byte-identical but for each feed's wall-clock `<updated>`, stderr identical for
all six, file counts 8 / 8 / 83 / 242 / 1828 / 1829 (G1's numbers, unmoved).
**One declared exception**: field-notes' `an-imported-artifact` post quotes the
retired spelling in its PROSE, and migrating that one word moved three files
(the page, `search.bin`, and one related-posts reordering — the post's
embedding changed, so its cosine rank against a neighbour did). Proved to be
the prose and not the engine by rebuilding that site with the word reverted:
byte-identical to HEAD's tree but for the two wall-clock feeds. `cargo test`
green (16 result lines); `cargo fmt --check` clean under the pin; clippy's
warning count identical to HEAD's (49, rebuilt in the worktree); re-blessing
limited to the `axis` fixture, whose member segment is the shell name
(`/tiers/light.html` → `/tiers/light_html.html`, 5 files plus a renamed one),
and the two wall-clock atom fixtures were reverted rather than re-blessed.

*For batch review I-A.* Three things worth a second opinion. (i) **The index
rule's silence** is the call a reviewer might reverse; the argument above is the
whole of it, and `the_raw_shell_is_the_byte_copies_static_indexes_included`
is the evidence. (ii) **`VIEW_DEFAULT = "html"`** puts a MAP name on a FOLD
declaration slot, which reads odd until you notice an undeclared view emits one
file per route — the doc comment says so, but if the axis wants a distinct name
for "the listing shell" this is where it goes. (iii) A **pre-existing lie found
in passing**: `grackle explain` prints `kind        post` as a hardcoded
literal (`main.rs`, `Query::Explain`) for every row — `/humans.txt` reports
`kind post`. Untouched (CLI-only, and I13 deletes the enum), filed as a task.

*For the queue.* `manual/OUTLINE.md` teaches the retired tier vocabulary in
eight places (640-641, 651, 851-852, 858, 1072, 1197, 1334, 1341 — including a
table whose rows are `none` and `light`), untouched per §4. That is the fourth
engine spelling to outlive that file, after `bucket` (F1), relations' `over`
(G1) and view `match` (G2). DESIGN.md's settled-ledger row for q44 (§9's
table) still names `none`; left as a settled row, per G2's precedent that
ledger rows record what was decided when.

**2026-07-27 — I3.** Landed as one commit. A respelling, and the interesting
work was in the two places a respelling is not the whole of it: what the retired
VALUE errors as, and where absent-`from` is *not* allowed.

*The pool did not move, and that is the claim.* `from = "*"` read the finished
route set; a fold shell with no `from` reads the same set, through the same two
passes. Six config lines were deleted and six trees came out byte-identical.
What changed is that the spelling now follows from §4's model instead of
naming a sentinel: a fold sits on a query over outputs, so "every output" is a
query it can serialize, and it needs no word to say so. The internal names
followed — `build_star_views`/`resolve_star_views` → `build_pool_folds`/
`resolve_pool_folds`, `From::is_star` → `View::reads_all_outputs` — because
leaving them would be this document's own disease, one release later. G1 kept
`is_star` on the grounds that it "is about the VALUE `from = "*"`, not about the
word `over`" (MERGE.md §6); the value is gone, so it went with it.

*What `from = "*"` says now, and why it says anything at all.* The star was a
value, not a key, so `deny_unknown_fields` never sees it — removing the arm
drops it through `check_base` with every other name that names nothing, and the
generic sentence there is *"sitemap: `from = "*"` is neither a collection, a set
nor a route (collections: drafts, objects, posts, tree; sets and routes: …)"*.
True, and useless: it sends its reader to go look for a collection called `*`
when the fix is to delete a line. Per the brief's allowance, the literal gets one
sentence of its own —

    sitemap: `from = "*"` names nothing — the star spelling is gone (IO.md §4).
    A fold shell reads every output by having no `from` at all, so delete the
    line: the `shell` (atom, sitemap, search) is what says this folds the pool.

— which is a REAL error about an invalid value, not a teaching error about a
migration: it does not promise the old spelling once worked, it says what the
value is (nothing) and what the construct is instead. Mutation-checked by
deleting the arm: the load still fails, by the generic sentence, which is the
diagnosis rather than the check.

*The other half was the item's actual content: absent-`from` is legal ONLY under
a fold.* `shell::check_absent_from` joins I2's four in the module that knows the
families. The reason it is a load error and not a comment is that **it does not
fail on its own**: `reads_all_outputs()` is one field (`from.is_none()`), so a
listing that forgot its `from` succeeds *as a fold* — `build_views` skips it,
`build_pool_folds` mints its route, and the site publishes an empty listing at
the URL the author asked a query for. Verified by building the mutant, not
reasoned: `/orphan/` comes out as a complete, themed, memberless page. Two
shapes in the corpus already exercised the rule, which is why it needed no new
fixture:

- **`profile-dry-run`**, whose whole point is a profile overlay adding
  `[sets.publised]` with only a `where`. Its expected error moves from serde's
  `missing field \`from\`` to the sentence that says why a set needs one — the
  requirement kept, its statement improved, and the one expected-error re-blessed
  in this item.
- **`excluded-schema`'s `[routes.covered]`**, which was `from = "*"` wearing
  `layout = "listing"` — a star view materializing an HTML landing over the ROUTE
  pool, i.e. exactly the shape the new law refuses. Migrated to a `sitemap` fold,
  which is what keeps its `where` in the route vocabulary the `cover` leak used
  to reach; the fixture still fails on `unknown field cover`, which is its job.

*The feed did not move, and the item says where the join lands.* A fold whose
`from` names a set consumes that set's ROWS — `[routes.feed] from = "published"`
and field-notes' `[routes.llms]` both — which is "those inputs' outputs" at the
only fidelity that exists today. **The join-mediated semantics land at I9**;
until then the amended brief's sentence is true by construction rather than by
mechanism, and `a_fold_over_a_set_still_reads_that_set` pins the half that must
not move either way: the route pool's sourceless artifacts (the sitemap, the
feed itself) can never appear in a feed, because a row query cannot reach them.

*Rung 0: nothing added, by design.* Fold selection sees forced fields on both
pools already — MERGE.md **R6** moved `force_route_fields` above
`resolve_pool_folds`, the engine's only `db.routes.select`, and
`profile_force.rs`'s `a_forced_field_is_read_by_both_pools_filters` guards it in
both directions (its route probe renamed from `star_probe` to `pool_probe` here,
nothing else). Stated in the log rather than re-tested, per the amended item: a
second guard on the same ordering would be a second thing to forget.

*Four tests, each mutation-checked and restored* (`crates/grackle/tests/io_folds.rs`):
(1) the unfiltered pool lists every route — the byte copy, the feed, both fold
artifacts, its own URL — and a predicate narrows *that* pool; mutating
`reads_all_outputs` to `false` makes `/all.xml` not exist, to `true` breaks the
base's own `blog_index`; (2) the feed over a set, unmoved; (3) absent-`from`
without a fold is a load error (delete the call → the silent empty listing
above); (4) `from = "*"` errors, by its own sentence (delete the arm → the
generic one).

*Corpus, and a correction to §10.* Migrated: `base.toml`'s sitemap (its comment
too), grack.com's `search` + `sitemap` + the `search` restated inside
`[profiles.drafts]`, theme-preview's two, field-notes' two, `examples/raw`'s
printed copy, and four fixtures. §10 says grack.com's drafts profile "carries
`kind ==` twice and `from = "*"` three times" — those are counts for the FILE,
not the profile: the overlay body holds exactly one of each (the restated
`[profiles.drafts.routes.search]`), and the other two star spellings were the
site's own `[routes.search]` and `[routes.sitemap]`. The grep is still the guard
the paragraph says it is; only the arithmetic was off.

*Parity.* Five sites plus grack.com `--profile drafts`, built from a `git
worktree` of HEAD with its own release binary and from this one, into separate
trees, caches seeded so binary and config were the only variables —
byte-identical but for each feed's wall-clock `<updated>` (6 atom.xml files, one
line each), stdout/stderr identical for all six modulo timings, file counts 8 / 8
/ 83 / 242 / 1828 / 1829 (G1's numbers, unmoved through I2 and I3). `cargo test`
green; `cargo fmt --check` clean under the pin; clippy 49 warnings, identical to
HEAD's rebuilt in the worktree; zero re-blessing beyond `profile-dry-run`'s
expected-error, and the two wall-clock atom fixtures were reverted rather than
re-blessed.

*Docs.* DESIGN.md §4a (the rung-0 paragraph's "in a `from = "*"` view over
routes" clause), §4e twice, §5's route-fields prose, §5c's key census (a new
paragraph — the census is where a reader looks for what `from` may be), §5g's
search-shell example, §6f's locale exemption, §7a's export note and the three
q53 axis sentences that named a `*` view. TODO-1.0.md's star-route defect item
was **re-verified rather than re-worded away**: it is still accurate — the pool
is still the route set, a routed row is still routed whatever its flags say, so a
site-declared fold still has to restate `!draft && !hidden` — and only its
spelling moved. `manual/OUTLINE.md` teaches the star spelling in five places
(274, 281, 287, 832, 1131/1140, including a whole section titled after it),
untouched per §4; that is the fifth engine spelling to outlive that file.

*For batch review I-A.* Three things to probe. (i) **The targeted `*` message** is
the call a reviewer might reverse — MERGE.md §4 bans teaching errors for retired
spellings, and this is a message that exists *because* of a retired spelling even
though it is provoked by an invalid value. The argument for it is above; the
argument against is that the generic sentence was never wrong. (ii)
**`check_absent_from` treats every registered `[shells.*]` script shell as a
fold**, so `[routes.x] shell = "llms"` with no `from` would read the route pool —
untested, because no site writes it, and IO.md §4 says a script shell will
eventually DECLARE its database (`pulls = "inputs" | "outputs"`). Today the
permissive reading is the only consistent one (`check_view` already accepts
registered names as folds), but it is a decision, not a derivation. (iii) **A
routeless fold** (`[sets.x] shell = "sitemap"`, no `path`) passes the new check
and then dies in `build_pool_folds` with "view x needs a route" — pre-existing
behaviour, carried forward unchanged, and arguably one of F3's "a set never
lands" family.

**2026-07-27 — Batch review I-A (Fable), covering I1, I2, I3.** Verdict:
**sound; I-B clear.** Six mutations re-executed, each red as logged; the
shell column probed live on grack.com (546 html / 187 raw / 635 Null, the
Nulls decomposing exactly into I2's two recorded shapes; mindstorms
answering `light_html`); no latent divergence beyond the documented
Null-while-themed drafts shape. Findings: (1) *should-fix → IR1(a)*: a
from-less script shell is fed `rows: []` at render — I3's flag (ii)
justification was false at the render layer, proven live. (2) *should-fix
→ I7 brief amended*: the caret draft already renders a slug title today
(pre-existing loader fallback, blamed to 1651892) — the declared parity
exception was likely vacuous and possibly under-scoped; re-measure on
arrival. (3) *model text marked shipped/pending → §3*: grack.com's
sitemap is not "the HTML documents" and never was (43 URLs beyond
`shell == "html"`, measured — PDFs, byte-copy html, static indexes, the
light_html page); the honest future spelling is a disjunction or a
declared change, Matt's call at its item. (4) *→ IR1(b)*: check_domain's
message tail wrong for `!=`. (5) *accepted + precedent recorded in §10*:
I3's targeted star message is an invalid-value error, not a teaching
error — the rule is "one targeted sentence only where the generic
diagnosis misleads". (6,7) *accepted, verified*: the index rule's
silence (first-writer law, deterministic, both halves load-bearing) and
VIEW_DEFAULT="html" (a route-column value, not a fold declaration).
(8) *→ IR1(c)*: the routeless fold. (9) *verified*: grack.com's search
migration honestly waits for I9; cheap adjacent truth noted (`_drafts`
rule could carry `shell = "html"` today, byte-inert, shrinks a Null
shape). (10) *clean*: no half-built identity code from the mid-flight
degeneracy amendment; doc spot-checks pass; `grackle explain`'s
hardcoded `kind post` lie still waits for I13 as filed.

**2026-07-27 — IR1.** Landed as one commit. Three closures, no bytes: five
sites plus grack.com `--profile drafts` came out byte-identical but for each
feed's wall-clock `<updated>`, file counts 8 / 8 / 83 / 242 / 1828 / 1829
unmoved through I2, I3 and this.

*(a) The narrowing, and why arity was the right reading of the wrong
question.* I3's flag (ii) argued that a registered `[shells.*]` name is a fold
by arity, so `check_absent_from` should accept it — and it is, and that is not
the question the check asks. The question is **which pool**, and the two
families answer it in different code: the engine's folds fill
`route_members` in `resolve_pool_folds` and read it back in `build.rs`, while
the script pass reads `r.members`, the ROW projection, which a pool fold never
fills. So the permissive reading fed a from-less script shell `rows: []` and
said nothing. Re-measured here in both directions rather than taken from the
review: with the arm restored, `[shells.echo] command = "cat"` plus a from-less
`[routes.probe]` builds and publishes
`{"route":"/probe.json","rows":[],"schema":"grackle-shell/0",…}`; with the
narrowing, it is a load error naming the view, the shell and the fix. **One
message got worse-shaped and had to be fixed with it**: the listing error
offered "or declare a fold shell: atom, sitemap, search; registered script
shells: …" as the alternative to naming a pool, and half of that list is no
longer an alternative to anything. The registered clause is gone from that
sentence and stays in `check_view`'s, where it is still true.

*(b) The tail names a constant, so it has to name the right one.* `!=` against
an out-of-domain literal is the same authoring mistake as `==` — an exclusion
that excludes nothing — with the opposite value, and "false" sent its reader
to hunt a predicate that never fires when theirs always does. Three arms
rather than two: an ORDERING comparison against an out-of-domain literal is
not constant at all (`kind > "pages"` splits the domain), so inventing a
constant for it would have been the same error one step subtler. It says what
is true instead — the literal is a value the column never holds — and it stays
an error, because that is what the domain check is for. The levenshtein hint
is untouched: it is a fact about the literal, not about the comparison.

*(c) The verification came first, and it is the item's actual output.* The
question was whether any legitimate routeless-fold shape exists — embedded
folds, say. **There is no such thing, and the reason is structural**: all four
fold passes in `build.rs` (atom, sitemap, search, script shells) iterate
`db.routes` and reach the view through the route carrying it, so a fold with
no route is unreachable by construction; a routeless view reaches only
`db.views`, via `insert_routeless`, and its one consumer — `{% view %}`
embedding — dispatches on `layout` and renders through `variant`. **Nothing
anywhere reads `shell` off a routeless view.** Measured against the HEAD
binary, the two live outcomes were bad in different ways: `[sets.x] shell =
"sitemap"` died mid-build with "view x needs a route", while `[sets.x] from =
"posts" shell = "atom"` built clean, reported its two base artifacts, and
published nothing for `x` at all — the silent half, and the one worth the
check. Both are config-time now, beside F3's set-theme error and keyed on
`declared_set` the same way. **Fires on FOLD shells only, deliberately**: a map
shell on a set is an arity mistake and `check_view` owns that sentence, so the
new check steps around it rather than stealing it (a test holds both).

*One residual, recorded rather than closed.* `declared_set` is the key the
brief named and the right one, but it is not quite the whole class:
`resolve_default_content` can take a *declined* `[routes.*]` entry's path away
before `validate` runs, which leaves a routeless view that never declared
itself a set. Keying on `!is_materialized()` would have caught that too. Not
taken — the shape requires a fold shell on a view that also offers
`default_content`, which is incoherent config in two directions at once, and
widening the key would have made a message that says `[sets.…]` fire on
something written under `[routes.…]`. If it ever shows up, this is the note.

*Mutations, each red and each restored.* (a) restore the
`registered.contains` arm in `check_absent_from` — `a_script_shell_with_no_
from_is_a_load_error` fails, and the probe site builds and publishes the empty
payload (both halves checked, because the test alone would not prove the
disease); (b) collapse `check_domain`'s `match op` to the single "false" arm —
the `!=` and ordering assertions fail; (c) delete the `declared_set` fold
check — `a_set_may_not_wear_a_fold_shell` fails on the first of its three
shapes.

*Parity.* Five sites plus grack.com `--profile drafts`, HEAD's binary built in
a `git worktree` (caches seeded) against this one, over the same content trees
so binary and nothing else varied — byte-identical but for the six wall-clock
`<updated>` lines, stderr identical for all six. `examples/field-notes`'
`/llms.txt` — the corpus's only script shell, and the only live user of the
rule (a) narrows — is byte-identical, as it must be: it has a `from`.
`cargo test` green; `cargo fmt --check` clean under the pin; clippy 49
warnings, HEAD's number; **zero re-blessing** — no fixture's expected error
moved, `profile-dry-run`'s included, because the clause that left the listing
message only ever appeared on a site that registers a script shell and no
fixture does.

*Docs.* DESIGN.md §5c gains two paragraphs beside I3's (the script-shell
exception, and "a fold lands at a route"), and its I3 paragraph loses the now
false "plus registered script shells"; §5g's script-shell section says the
view must name a `from`; §5b's `kind`-domain example records that the tail
follows the operator; the sets-vs-routes key census notes that two of its ten
route-only keys (`theme` by F3, `shell` by this) are now checked rather than
observed. `manual/OUTLINE.md` untouched per §4.

**2026-07-27 — IR2.** Landed as one commit. **Decision: (b), deletion — and the
argument for it is not "I13 is coming", it is that (a) had no correct value to
print.** A row has no kind. The arm could have reached `db.routes` for one, and
what it would have printed is a fact about the OUTPUT under a heading in a
block of row facts — true for one release, and a caller for I13 to unpick. §3
says the enum is a flattened product of independent facts; the honest fix is to
print the factors.

*What replaced it, and why three lines rather than the two the item named.* The
item said `shell` and `front_mattered`. The third is `collection`, and it is the
one that answers the original lie directly: `kind == "post"` never meant "is a
post", it meant **scope membership** — §3's table says so and `Route.kind`'s doc
comment says so — so the line that used to say `post` for a `.txt` now says
`collection entries`. All three are filter columns, which is the property worth
having: every line of the block is a name a reader can put in a `where`.

*The measurement that changed the shape of the fix.* `shell` looked like it
needed no line at all — `explain` already dumps `r.fields`, and I1 recorded that
the value lives there as well as on the named field, which is why
`explain /humans.txt` printed `shell raw` at HEAD. Counted rather than assumed:
of grack.com's 1396 rows, 366 resolve `html`, 187 `raw`, 1 `light_html`, and
**842 resolve nothing** (every object among them). The dump prints a field only
where the row has one, so the surface was answering "which shell" with silence
for 60% of the corpus while answering "which kind" with a lie for 100% of it. An
absent line is not an answer, so `shell` is printed explicitly off `Row.shell`
and reads `-` when Null; the dump skips `shell` so the 554 rows that have one do
not print it twice.

    url         /humans.txt        url         /code/…/dice2.png
    collection  entries            collection  objects
    shell       raw                shell       -
    front_mattered false           front_mattered false

*The grep came first and found nothing*, which is what let the line go without
re-blessing: no test, no fixture, no script and no doc reproduces `explain`'s
output — DESIGN.md §0 shows the command being *invoked*, never its lines, so
nothing in DESIGN.md became false here and nothing needed amending. The route
branch of the same arm is untouched: its `kind` comes from `tag(r.kind, …)` and
is real until I13.

*Guard.* `crates/grackle/tests/io_explain.rs` asserts the block for two rows
that disagree in all three facts — a post with a block (`posts`/`html`/`true`)
and a `.txt` copied verbatim (`entries`/`raw`/`false`) — through the loader,
because a unit test over a hand-built `Row` would prove only that `format!`
interpolates and would pass against an engine that never gave a byte copy the
`raw` shell. The formatter moved to `debug::row_facts` to be reachable from
`tests/` at all (`main.rs` is not in the lib), beside `value_text`, which
`explain` already shares with the inspector. Mutations, each red and each
restored: hardcode any one of the three — `"posts"` (the original lie), `"html"`,
`true` — and the byte copy's assertion fails on that line.

*Parity, run though nothing here is in a build path.* Five sites plus grack.com
`--profile drafts`, HEAD's binary built in a `git worktree` against this one over
the same content trees — byte-identical but for the four wall-clock `<updated>`
lines, stderr identical for all six, file counts 8 / 8 / 83 / 242 / 1828 / 1829,
IR1's numbers unmoved. `cargo test` green; `cargo fmt --check` clean under the
pin; clippy 49 warnings, HEAD's number; zero re-blessing.

*One thing found in passing, not fixed.* `explain` prints `layout` twice for
every row that has one — once as a named line, once from the field dump, because
`layout` is both a `Cascaded` key the engine reads by name and a declared column
(MERGE.md C1). Noise rather than untruth, and the same shape `shell` would have
grown here, so it is filed rather than folded in: the `shell` skip is keyed on
the one name this item is about.

**2026-07-27 — I4.** Landed as one commit. The item's whole shape turned on one
decision — that the `<body>` wrapper is OPTIONAL — and everything cheap about
the migration follows from it.

*The migration was `git mv`, nine times, because the wrapper is optional.* A
`root.html` with no `<head>` and no `<body>` at its top level **is** the body
chrome, which is exactly what a `shell.html` always was. The alternative —
requiring `<body>` — would have made every theme in the repository write a tag
none of them means anything by, and would have made this item a nine-file edit
under a byte-parity gate for no gain. `binder::split_root` states the three
accepted shapes: a **fragment** (the body), a **document** (`<head>` and/or
`<body>` at the top level and nothing beside them), and **head-only** (a
`<head>` with no `<body>`). `the_body_wrapper_is_optional_and_inert` builds the
same chrome both ways and asserts the two pages are equal character for
character, which is the small statement of what the corpus parity run says at
scale.

*Head-only needed no rule, and that is the argument for splitting at the source
rather than after the merge.* A theme contributing no `root` BODY simply drops
out of `own` in `Theme::from_sources`, and the by-name fragment merge then keeps
the base's chrome — so "adds styles, inherits the chrome" is the cheapest real
theme the design allows and costs one `own.remove(i)`. Mutating it to keep an
empty body fragment instead publishes `<body>\n\n</body>`: the chrome and the
page's own content, both gone. The split runs on the SOURCE (the parser gained
one field, `Element::inner`, the span of an element's children) so the body half
arrives at `Fragments::load` as an ordinary fragment source and the head half
never enters the part vocabulary at all — it is presentation, not an arrangement
of parts.

*The fence, and the measurement that changed what the test claims.* A theme
root's `<head>` may hold `<style>` and nothing else; every other element is a
load error naming the file, the line and the element. The brief predicted the
mutation would show silent DROPPING. It does not: with `check_head_fence`'s call
deleted, the tag is **published** — `<meta name="theme-color" content="#123456">`
came out in the head of all three pages of the probe site, because `Theme::page`
emits the head half verbatim. That is worse than dropping and is the whole
argument: a theme `<title>` would give every page two, a theme
`<link rel="canonical">` would compete with the engine's, a theme stylesheet
`<link>` would break the one-artifact rule — all valid HTML that no build would
ever complain about. Measured with the real binary on the mutant, not reasoned.

***[decided]* The head-style interim: emit it, don't shelve it.** The brief
allowed either "carry and ignore with a recorded note" or "emit inline verbatim
after the computed head". Emitting is the least-surprising reading by the
project's own standard — a declared thing that is parsed, validated, and then
silently discarded is the disease every guard in this ledger exists to refuse,
and it would have been odd to build the fence and then commit the sin the fence
is against. Placement is after the engine's facts and last in the head, which is
what a `<style>` at the end of a head means anywhere else: the computed head is
never displaced, and the theme's rules outrank the one stylesheet link above
them. **I5 moves it into the CSS assembly**, and this is the byte that will
move — for fixtures only, since no theme in the corpus has a head style.

*The stale file: a real error, and not the one the brief expected.* The amended
brief called silence "the disease" here. There was no silence available: the
part kind renamed, so a leftover `shell.html` reaches `Fragments::load` as a
fragment naming no layout kind and the load already fails. Deleting the new
check proves it — *"fragment names no layout kind `shell` — kinds are: root,
document, …"*. True, and useless: it sends its reader hunting for a kind when
the fix is a rename. So the targeted sentence stands, on §10's precedent (one
targeted sentence only where the generic diagnosis misleads) rather than as a
guard against silent chrome loss. Keyed on `shell.html && !root.html`, per the
brief; a theme carrying both still gets the generic message, which is accurate
there.

*The kind rename, and the one word that kept its spelling.* `parts.toml`'s
`[[kind]] name = "shell"` → `"root"`, and with it `base.rs`'s manifest,
`theme.rs`'s three reads (identity-slot derivation reads the kind's schema and
the root fragment's slots — a `.slots/copyright.md` in the new tests is the
cheapest proof both followed), `parts.rs`'s vocabulary pin, and `slots.rs`'s
dead-fill warning text (which no corpus site emits and no fixture asserts —
checked before touching it). **`render::root_shell` keeps its name**: §6 bans
*shell* from theme vocabulary, and that function is the engine's own skeleton,
named in DESIGN.md §5g as "the root shell". Renaming it would have been a diff
across five call sites to satisfy a rule about what a THEME may say.

*The stamp, and how the parity claim was made mechanical.* `data-kind="shell"`
→ `data-kind="root"` on the `<html>` of every page — 1365 pages across the six
build trees, 84 fixture files. Both halves were proven by SUBSTITUTION rather
than by inspection: the fixture expectations were sed-ed by that one rule and
the suite run green (never `UPDATE_EXPECT`), and the six BEFORE trees were
normalized by the same rule and then `diff -rq`'d against the AFTER trees. What
remains after normalizing is six wall-clock `<updated>` lines and nothing else.
Verified ahead, as the brief asked, and re-verified with a grep: no stylesheet
or script anywhere in the repository keys on `data-kind` with the value `shell`
(the base's page geometry moved to `[data-frame]` long ago — GRAVEYARD.md
records the move).

*Parity.* Five sites plus grack.com `--profile drafts`, HEAD's binary built in a
`git worktree` against this one, into separate trees, caches seeded so binary
and theme files were the only variables — byte-identical modulo the declared
substitution but for the six wall-clock `<updated>` lines; stdout/stderr
identical for all six modulo timings; file counts 8 / 8 / 83 / 242 / 1828 /
1829, unmoved through I2, I3, IR1, IR2 and this. `cargo test` green; `cargo fmt
--check` clean under the pin; clippy 49 warnings, the warning SET byte-identical
to HEAD's rebuilt in the worktree (one nit of my own was fixed to keep it so);
re-blessing limited to the declared attribute on 84 fixture files.

*Six tests, each mutation-checked and each restored*
(`crates/grackle/tests/io_root.rs`): (1) the optional wrapper — require it and
the bare site fails as `<header>` beside no `<head>`/`<body>`; (2) the fence, in
four spellings (`<meta>`, `<title>`, `<link>`, `<script>`), the mutation above;
(3) the head style's presence and placement — delete the block in `Theme::page`
and it vanishes while the theme still loads clean; (4) head-only inheritance —
the `own.remove` mutation above; (5) nothing may sit beside a root's head and
body; (6) the stale `shell.html`.

*Docs.* DESIGN.md §5g rewritten (the three shapes, the fence, the interim, the
stamp, the `root_shell` naming call), plus its theme-directory listing (§5e) and
"ship a root, own the frame". themes/DESIGN.md §0 gains the platform fact, and
§3's chain paragraph, back-tested edge 2 and §6's rule follow. **Left alone
deliberately**: GRAVEYARD.md's and MERGE.md §6's `shell.html` sentences are log
rows recording what was decided when, per G2's precedent. `manual/OUTLINE.md`
untouched per §4 — it teaches `shell.html` nowhere, so this is the first engine
spelling in the sequence that does NOT outlive that file.

*For batch review I-B (after I5).* Three things to weigh. (i) **The interim
emission** is the call a reviewer might reverse, and I5 is where it gets
settled either way — if the reviewer prefers the head `<style>` to be inert
until I5 extracts it, the change is deleting four lines in `Theme::page` and one
test. (ii) **`split_root` runs on themes only, not on the base**, whose
`root.html` is body-only by construction; a base that ever grew a head would
need the call added, and nothing would tell it so. Not guarded, because the base
is a compiled-in engine asset with a manifest and a load test rather than a
theme someone edits. (iii) **A theme root's head is not part of the fragment
CHAIN** — themes/DESIGN.md §3's `extends` is unbuilt, but when it lands, "child
shadows parent's head `<style>`" is a decision nobody has made; the doc now says
the two halves shadow independently, which is the reading `split_root`'s
per-theme placement gives for free, and I5's multi-theme scoping paragraph is
where it wants restating.

**2026-07-27 — IR3.** Landed as one commit. The item named `layout` and asked
me to check `theme`; the answer is that **all four cascade keys had the same
defect in two different spellings**, and fixing one name at a time would have
left the surface saying nothing about half of them.

*What the check found, against the item's guess.* `theme` is not "printed by
neither path" — it is printed by the DUMP, and only there: `explain
/recipes/red-lentil-dal/` said `theme recipes:spicy` at HEAD. That is exactly
`shell`'s pre-IR2 shape, so IR2's argument applies unchanged (the dump prints a
field only where a value landed, so its silence cannot be told from "no such
key"). `toc` is the same shape and one degree worse: `Row.toc` is a `bool`, not
an `Option`, so "never set" and "set false" are one row-level answer, and the
dump collapsed both into no line at all. Two of the four were doubled
(`layout`, and `shell` until IR2); the other two were silent. One family, two
symptoms, one cause — C1 declaring the four in the base `[schema]` made each of
them a named field on `Row` *and* a declared column in `Row.fields`.

*So the skip is keyed on the family, not on a name.* `schema::CASCADE` becomes
`pub` and `debug::row_fields` reads it, rather than growing a fourth
`if name == "…"`. The list is the reason the defect exists; a printer that
restates it by hand is one edit away from re-growing it when a fifth key lands.

    title       Anatomy of a Failed…      title       Red lentil dal
    layout      post                      layout      page
    theme       -                         theme       recipes:spicy
    toc         false                     toc         false
    tags        security                  course      dinner

*The formatter moved for a testable reason, not a tidy one.* A test cannot see
a doubled line unless the named line and the dump are in one string, so the
whole block below `title` is now `debug::row_fields` beside IR2's `row_facts`.
`shell` stays up in `row_facts` — it is there because it is one of the three
facts that replaced `kind`, and moving it would churn IR2's block and its test
to no end — but the skip covers all four, which the mutation run shows: delete
it and the post grows `layout`, `shell`, `theme` and `toc` a second time.

*Guard.* `io_explain.rs` gains a second test over the same fixture, extended so
the post resolves all three named keys plus one declared field that is NOT a
cascade key (`minutes`) — that field is what pins the skip as "skip these four
names" rather than "skip the dump". The `.txt` resolves none of them and is the
half the dump could never answer. Three mutations, each red and each restored:
the `CASCADE` skip deleted (doubled lines), the `theme`/`toc` named lines
deleted (the `.txt` loses them entirely), the unresolved sentinel changed. The
fixture also gained a per-test directory: two tests sharing one temp tree that
each `remove_dir_all` at both ends is a race, and they run in parallel.

*The grep was re-run and still finds nothing.* No test, fixture, script or doc
reproduces `explain`'s lines — DESIGN.md §0 invokes the command and never shows
its output, and DESIGN.md §4e's "the inspector and `explain` printing three
named bools → deleted" is about `draft`/`hidden`/`noindex`, which stay deleted;
the same table's next row already names the four cascade keys as "what the
engine still READS by name", which is what now gets four lines. Nothing in
DESIGN.md became false, so nothing needed amending.

*Parity.* All six trees (five sites + grack.com `--profile drafts`) built from a
`git worktree` of HEAD with its own release binary and from this one, against
the same content — byte-identical but for two feeds' wall-clock `<updated>`,
stderr identical for all six, counts 8 / 8 / 83 / 242 / 1828 / 1829, unmoved
since IR1. `cargo test` green (14 binaries); `cargo fmt --check` clean under the
pin; clippy 49 warnings, HEAD's number; zero re-blessing.

*Nothing found in passing.* The row branch of `Query::Explain` now prints every
`Row` field it has a use for except `description` and `order`, both `Option`s
that no site in the repo sets; whether they deserve lines is a question about
what `explain` is for rather than a defect, and I did not answer it.

**2026-07-27 — I5.** Landed as one commit. The extraction itself was small; the
three decisions around it are the item, and one of them is a claim about a
mutation I4 had already measured and that this item made false.

*Where it lands, and why after `theme.scss`.* A theme's own CSS is now two
files in a fixed order inside the one `@layer theme` block: the general sheet,
then whatever `root.html`'s head declared. The argument is not "later is
later" — it is that `root.html` is the file where a theme states its own
frame, so what it says about the frame should outrank the general sheet rather
than lose to it. The second argument is continuity, and it is the one that
makes the placement checkable: under I4's interim a `<style>` last in a
`<head>` was UNLAYERED and beat the stylesheet link outright, so a rule
written in the head won. Last in the theme layer keeps the same rule winning
against the same competitor, which is the strongest sense in which this move
is a relocation and not a behaviour change. One layer block, not two — a test
pins the count, because two `@layer theme` blocks would order by declaration
and make the claim true for the wrong reason.

***[decided]* SCSS, not verbatim.** Through `inline_imports` then grass, with
the theme directory on the load path — the same two steps `theme.scss` takes.
The cost is a pass over a few lines; the buy is that a head style may nest and
may `@import "tokens";`, reaching the theme's own partial or the engine
base's. Verbatim was the simpler code and the worse rule: it would have made
`root.html` the one file in a theme where the theme's own vocabulary does not
work, and that kind of exception is never discovered by reading, only by
hitting it. The other face of the decision is that a head style can now FAIL
to compile, which is a new error path and gets the treatment its neighbour
has: `scss:` on stderr naming `root.html`, an entry in `Stats::css_errors`,
and a publishing build that refuses. `serve` still gets its sheet, minus a
theme layer.

*The split moved to the source, so the tags never travel.* `binder::Root.head`
became `Root.style` and holds the `<style>` elements' CONTENTS, not the head's
markup — `head_styles` collects them in source order. Carrying the tags on
would only have meant stripping them in `build.rs`, and the type would have
been lying about what it held for the whole journey. `head_styles` tests
`el.tag == "style"` itself rather than leaning on the fence two frames up: a
function whose correctness depends on its caller's check is one refactor from
being wrong.

*The fence's mutation was RE-MEASURED, and it says something different now.*
I4 measured that deleting `check_head_fence` PUBLISHES the tag — `<meta
name="theme-color">` came out in the head of every page — and built the whole
argument for the fence on "worse than dropping it". After this item the head
half leaves as CSS, so the same mutation on a probe root carrying a `<meta>`,
a `<title>` and a `<style>` builds clean and publishes a page with **neither
the meta nor the title**, only the style, in the sheet. Measured with the real
binary on the mutant, not reasoned. The failure moved from quietly-wrong
output to quietly-no output; the fence is what makes it neither, and the test's
doc comment now records the new outcome rather than inheriting the old claim.
This is the item's one worked example of why a mutation's *observed effect* is
part of a test and goes stale like anything else.

***[declared]* The per-theme sheets ARE the megacss's chunking.* A
documentation event with no code in it, which is exactly what made it worth
declaring: §6 said the model is one artifact and chunking is a perf detail the
model never mentions, and `/css/main.css` + `/css/<name>.css` have been that
chunking since they existed. A page links one sheet and that sheet is the
whole cascade for that page — engine base, theme, head styles, site overlay,
in declared layers — which is what "one artifact" means from where a page
stands. Recorded in `css_pass`'s doc comment, DESIGN.md §5g and §6 here. No
URL moved, no assembly was restructured, and the parity run is the evidence
that the declaration cost nothing.

***[open] closed* — the multi-theme scoping paragraph** lives in **IO.md §6**,
directly under the one-CSS-artifact paragraph it qualifies, with §9's question
4 struck through and pointed at it and a mirror in **themes/DESIGN.md §3**
(inside the nested-layers block, which is the construct it reuses). The
argument in one line: chunking already scopes today, and that is the problem —
an optimization doing a correctness job — so the model needs an answer that
holds when chunking is off. Two ingredients, both emitter-side, both inert
until merging is built: **per-theme sub-layers** (`@layer theme.ledger,
theme.terminal, …`, themes/DESIGN.md §3's plan pointed sideways instead of
down the `extends` chain) settle precedence between themes deterministically;
the **stamped root attribute** (`data-theme`, beside the existing
`data-subtheme`) is the scope, because layers order rules but do not stop them
matching. Its cost is stated rather than hidden: prefixing theme rules with an
attribute selector is a transform the engine does not do today and it lifts
every theme rule's specificity uniformly — survivable precisely because the
sub-layers, not specificity, decide the cross-theme case. **I4's flag 3
restated there**: a root's head is not part of the fragment chain, and the
paragraph says what that implies — one chain member's CSS occupies ONE
sub-layer, with `theme.scss` and the head style ordered against each other by
source position inside it (the ordering this item landed), so shadowing (by
file name) and ranking (by layer) never do each other's job.

*Five tests, four mutations, each red and each restored*
(`crates/grackle/tests/io_root.rs`): (1) the style is in the sheet, inside
`@layer theme` after `@layer base`, and NOWHERE in the page — no
`rebeccapurple`, no `<style` at all, one `rel='stylesheet'`; it also pins that
a head style is not a `theme.scss` (the theme still gets the base's skins).
Mutations: restore I4's emission in `Theme::page` → the head carries it again;
pass `""` at `css_pass`'s call sites → the sheet loses it while the theme loads
clean. (2) the ordering pin — the same selector and property in `theme.scss`
and in the head, root wins, one layer block; mutation: `insert(0, …)` instead
of `push` → `from-scss` wins. (3) SCSS is real (nesting compiles, a
`_tokens.scss` partial resolves through `@import`) and a broken style records
one `css_error` naming `root.html`; mutation: drop the `Err` arm's push → the
broken style is silently absent from a sheet that publishes fine. (4) the
head-only-root test updated to look in the sheet. (5) the fence test's
re-measured mutation, above. A fifth mutation crossed the two halves: make
`split_root` hand on the head VERBATIM again and four tests go red at once,
because the `<style>` tags reach grass.

*Parity.* Five sites plus grack.com `--profile drafts`, HEAD's binary built in
a `git worktree` against this one, over the same content trees so the binary
was the only variable — byte-identical but for the six wall-clock `<updated>`
lines, stderr identical for all six, stdout identical modulo timings, file
counts 8 / 8 / 83 / 242 / 1828 / 1829, unmoved since IR1. `cargo test` green
(19 result lines); `cargo fmt --check` clean under the pin; clippy 49
warnings, the warning SET byte-identical to HEAD's rebuilt in the worktree;
**zero fixture re-blessing** — I4's head-style guards are temp-site tests
rather than blessed expectations, so the declared byte movement was an edit to
two assertions, and no `expected` file in the tree moved.

*Docs.* DESIGN.md §5g (the interim paragraph replaced by the extraction, plus
the megacss declaration); themes/DESIGN.md §0 (the platform fact) and §3 (the
CSS formula gains the head-style line, and the nested-layers block gains the
two paragraphs above); IO.md §6 twice and §9's question 4. `manual/OUTLINE.md`
untouched per §4 — it teaches neither `root.html` nor the CSS assembly, so
this is the second engine spelling in the sequence that does not outlive that
file.

*For batch review I-B.* Four things to probe. (i) **The ordering** is the call
a reviewer might reverse — `theme.scss` after the head style is the other
reading, and the case for it is that a theme's main sheet is the one an author
edits most and would then always win. The argument above is the whole of the
case for what landed, and `a_root_head_style_outranks_the_themes_own_sheet` is
the one-line change either way. (ii) **The scoping paragraph is
propose-and-flag** and nothing enforces it — it describes an emitter that does
not exist, so it is a design commitment with no guard, which is correct for an
unbuilt merge but means the next agent to touch CSS assembly is the one who
has to remember it. (iii) **A pre-existing wart, found and NOT fixed**: a
tokens-only theme (`_tokens.scss`, no `theme.scss`) gets `own = Some(tokens)`,
so `seen` never contains `"tokens"` and `css_pass` prints *"has a
_tokens.scss that nothing imports"* at every build — spurious, since the
tokens ARE the sheet. No theme in the corpus is tokens-only, so no site emits
it and stderr parity was unaffected; left alone rather than folded in, and
proposed here as an item. (iv) **DESIGN.md §6c (per-post styles, unbuilt)
still says a row's `<style>` is "hoisted into `<head>`" as an inline block,
"not a `<link>`"** — which is the opposite of §6's one-artifact rule and of
the `post` layer both CSS docs declare. Not touched: I5 did not make it false
(it was already in tension, and IO.md's header says IO.md is the successor
where they disagree), and rewriting it would decide a question that belongs to
whoever builds per-post CSS. Proposed as a doc item, or as a note on that
item.

**2026-07-27 — Batch review I-B (Fable), covering IR1, IR2, I4, IR3, I5.**
Verdict: **sound; I-C clear.** Six mutations re-executed, each red as
logged; both grack.com parity claims independently reproduced (the I4
substitution is genuinely the only byte moved — three diverse pages
raw-diffed; sitemap and search.bin byte-identical; I5 moved nothing);
explain probed on all three row shapes — coherent, every line
where-addressable, no doubled key. Findings: (1,2) *should-fix → IR4*:
the head fence is bypassable by an `<html>` wrapper (whole document
becomes body chrome, title and metas shipping in `<body>` — probed live),
and non-whitespace top-level text silently drops. (3) I5's re-measurement
adjudicated CORRECT: unfenced head elements now silently drop (the
mechanism: head_styles takes only style contents); the fence test still
guards. (4,9 → IR5): the tokens warning is false in two shapes (one
I5-created); DESIGN §6c gets a supersession sentence, substance undecided.
(5) the I5 ordering call endorsed — last-in-layer is the only placement
where I5 is a relocation, not a behavior change. (6) the grass path
behaves (imports resolve theme-then-base; errors name root.html).
(7) no IR3×I5 interaction; CASCADE is closed at four. (8) the scoping
paragraph is internally consistent; one unworked corner recorded (a
SHARED chain ancestor's sub-layer needs per-consuming-theme scope
emission — one sentence owed when the merge emitter is built).
(10) commit-message heredoc artifacts in IR2/I5 — history, cosmetic.
I7 brief amended: the themes/-as-content decision and the Null-collapse
pointer. I6/I8 unamended.

**2026-07-27 — IR4.** Landed as one commit. Three refusals, no bytes — and
the doctype question turned out to have an answer that needed no ruling.

*The wrapper hole, and what the mutant published.* An `<html>` at a root's
top level made `wrapped` false, so the file took the FRAGMENT path and the
whole document — head, metas, title — became body chrome. Measured on the
probe shape with the arm deleted rather than reasoned: the site builds
clean and every page comes out as the engine's document (doctype, `<html
lang="en" data-kind="root">`, the computed head with the real `<title>`)
whose `<body>` then opens a second `<html>`, a second `<head>`, `<title>My
Theme</title>` and the theme's `<meta>`, closing with two spare
`</body></html>` pairs. Valid enough that no build and no browser says a
word, which is why the check goes BEFORE the fragment/document test rather
than beside it: the two accepted shapes cannot make this check for
themselves.

***[decided]* The doctype is the same mistake, and that is the cheap
answer.** The brief allowed its own error; one message for both is better
because a doctype fails *two different ways* and the refusal has to rule on
neither. Measured on the mutant: in front of a fragment it publishes
`<!DOCTYPE html>` as the first bytes inside every page's `<body>`; in front
of a document it was silently dropped pre-IR4 (and now falls to IR4b's text
rule, which refuses it for the right reason with the wrong advice — the
arm's other justification). The engine writes the skeleton, doctype and
`<html>` both, so a theme that declares either has copied a page, and which
of its two failures is worse never has to be decided.

*The dropped words.* The `continue` that lets whitespace and comments
through was letting prose through with them — a theme's sentence gone with
no error and no output (mutant: builds clean, stderr silent, the words
nowhere). Named now, with the words quoted, because a line number alone in
a file whose halves are hundreds of lines apart is a search rather than a
fix. The line reported is the WORDS' — a top-level text run starts at the
newline after `</head>`, so the run's own line is the previous one.
`Node::Text` gained a `line` for this; it is the only reason it has one.

*The rider, which is a real bug and not a wording nit.* "Move it inside
`<body>`" for a top-level `<style>` is advice that lands the theme's CSS in
its chrome: unlayered, inline, on every page, and valid HTML no build would
complain about. The fence exists to take a style and I5 compiles what the
fence takes into the theme's sheet, so the advice now depends on the
sibling — `<head>` for a `<style>`, `<body>` for everything else.

*Mutations, each restored.* (a) The `<html>` arm → the probe shape above
builds with title-in-body. (b) The doctype arm → both halves as measured.
(c) The text test → the silent drop. (d) The advice made unconditional →
the `<style>` half red, the `<footer>` half green. (e) **The control, the
other direction**: drop `is_doctype`'s `!is_comment` carve-out and the
bare-fragment test goes red — a leading comment, which every theme may
write, told it had declared a doctype. A refusal that is too wide is the
mistake available here, and only a control catches it.

*Parity.* Five sites plus grack.com `--profile drafts`, HEAD's binary built
in a `git worktree` against this one, into separate trees from the same
content — byte-identical but for the six wall-clock `<updated>` lines,
logs identical modulo timings, file counts 8 / 8 / 83 / 242 / 1828 / 1829
unmoved through this. All nine corpus themes are bare fragments, so the two
new refusals cannot reach them, and the exactness is that fact rather than
a hope. `cargo test` green; `cargo fmt --check` clean under the pin; clippy
49 warnings, the set unchanged; zero re-blessing.

*Docs.* IO.md §6 gains the sentence that makes "the engine owns `<html>`" a
checked claim; DESIGN.md §5g gains the refused shape beside its three
accepted ones; themes/DESIGN.md §0 says it in a theme author's words.

*For batch review I-C.* One thing to weigh: the wrapper error is refused at
the TOP LEVEL only, so an `<html>` nested inside a theme's chrome is still
ordinary markup the binder passes through. That is deliberate (the hole was
the wrapper, and a nested `<html>` is not a wrapper) but it is the corner
where "the engine writes `<html>` itself" is still a convention rather than
a check.

**2026-07-27 — IR5.** One commit, no bytes. A warning that fired three times
and was right once.

*One question, asked of one file.* "Does anything import this
`_tokens.scss`?" lived inside the `theme.scss` pass and read that pass's
`seen` list, which is the whole bug: it is a question about every source the
theme compiles, and a per-pass list can only answer for one of them. Both
false shapes fall out of that, and the fix is one line of scope — pool both
passes' imports, then ask. Measured on the probe sites rather than reasoned:
the head-style shape warned, the tokens-only shape warned, and each stopped
when its own half of the fix landed.

***The tokens-only shape is not an import at all***, which is why pooling
alone does not cover it. With no `theme.scss` the partial IS the compiled
sheet — `own = Some(_tokens.scss)` — so no `@import` names it and none could;
the file is fully alive with an empty import list, the one shape where
"nothing imports it" and "nothing reads it" come apart. The advice was
unfollowable on top of being wrong: it names a `theme.scss` the theme does
not have. Treated as self-importing (`own == tokens`) rather than by
suppressing the warning when the list is empty, because the empty list is
also the true case's list.

*The channel, and why it exists.* `Stats::css_warnings` beside `css_errors`,
pushed where the `eprintln!` already was. A guard fixed into SILENCE cannot
be tested by scraping a process's stderr in-process, and the alternative — a
subprocess run of the release binary — is a test harness this suite does not
have for one warning. Nothing reads the field to decide anything, which is
the difference between this and `css_errors` (which `build` refuses on).
The message text is byte-identical to what it was; only its position moved,
after both passes instead of between them.

*Three-way, two mutations and a control* (`io_root.rs`,
`the_orphaned_tokens_warning_asks_the_whole_theme`). Silence in the two false
shapes is asserted *with* the bytes that make it true — `--edge: peru`
reaches the sheet in both, so the test says the tokens are live rather than
merely that nobody complained. Mutations, each red alone and each restored:
drop the head pass's `imported.append` → the head-style case warns again
(the I5-created half, exactly as the review found it); drop the `!tokens_only`
term → the tokens-only case warns again. The control is the third assertion,
and it is the one that matters for a narrowing item — delete the warning
outright and the first two shapes pass while the real case goes red, so this
cannot be a removal wearing a fix's clothes.

*§6c, one sentence and no more.* DESIGN.md §6c said a row's `<style>` is
"hoisted into `<head>`" as an inline block, "not a `<link>`" — the opposite
of IO.md §6's one-artifact rule and of the `post` layer `css_pass` has
declared since I5. The blockquote-after-the-heading form §5a already uses,
saying three things: pre-IO prose, who governs, and that the substance stays
undecided. The scoping default is named as the live question because it is
the one a reader would otherwise take as settled. Nothing else in the section
moved, and no other doc states the warning's rule — §5g and themes/DESIGN.md
§3 describe where a head style *goes*, not what the orphan check asks — so
this item's doc surface is one sentence.

*Parity.* Five sites plus grack.com `--profile drafts`, HEAD's binary built
in a `git worktree` against this one, over the same content trees into
separate outputs — byte-identical but for the wall-clock `<updated>` lines,
stderr identical on all six, file counts 8 / 8 / 83 / 242 / 1828 / 1829,
unmoved since IR1. The warning inventory could not move and the reason is a
fact rather than a hope: every corpus theme has a `theme.scss` (so none is
tokens-only) and no corpus `root.html` has a `<head>` at all (so none imports
from one) — checked, and the six stderr streams carry no tokens line before
or after. `cargo test` green (19 result lines); `cargo fmt --check` clean
under the pin; clippy 49 warnings, the count unchanged against the worktree;
zero re-blessing.

*For batch review I-C.* Two small things. (i) The surviving warning still
says "add `@import \"tokens\";` to theme.scss" — correct now, since the only
shape that reaches it has a `theme.scss`, but a theme whose CSS lives mostly
in its root head would be told to edit the other file. Left alone: changing
it would move stderr text for no live site, and the advice is right for the
shape that triggers it. (ii) `css_warnings` is a test-only channel by
convention, not by construction — nothing stops a later caller from reading
it the way `build` reads `css_errors`, and if one ever should, the doc
comment on the field is where that decision has to be written down.
