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

- ~~**The old `rendered` boolean** is bare-`output` truthiness (house
  style: a bare field means "has one"). The name `rendered` retires.~~
  **[STALE — corrected by review I-C, Matt's to rewrite]**: since I7c,
  `rendered` is the rendering LAW's output (`front_mattered || shell ∈
  DOCUMENT`), not output-truthiness — every routed byte copy has an
  output and is not rendered — and IR7/I8 built surfaces on the name.
  `output` lands BESIDE `rendered` at I9, not in place of it.
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

- [x] **I6. Extractors move to rules** (the one-row-type remainder):
  `filename_formats` per-rule, one route-token supplier offering path
  tokens always plus extractor results. Parity.

**I7 — SPLIT (2026-07-27, planning agent; full proposal in its report).**
The original brief follows as I7a-I7e. Brief corrections adopted from the
planning read: `kind =` does NOT retire in I7 (load-bearing in nine
non-membership places — dated indexes, adjacency, relation defaults,
schema dispatch, listing/gallery/script dispatch, RouteKind::Post which
grack.com filters deliberately; the role survives until I9's join);
DESIGN §9b's "six underscore excludes" obstacle is amended rather than
paid (the skip survives; scopes punch through); exactly ONE blockless
`.md` exists corpus-wide, so the degeneracy warning's blast radius is one
stderr line. Two laws are proposed-and-flagged for Matt's veto at review
I-C: **most-specific-source ordering** and **a scope owns its source**
(both verified to reproduce today's behavior on all corpus sites).

- [x] **I7a. Extension selection becomes rules.** `[[collections]]
  extensions` retires (hard cutoff; four configs migrate in-commit) for
  glob rules in the objects scope; `is_obj`'s pre-rule scan becomes "did
  an objects-scope rule claim this path". Defers: the walk merge, the
  ordering law, degeneracy, the object constructor. Propose-and-flag:
  extension globs compile **case-insensitive** (`is_obj` is; globset
  isn't; `assets/2004/06/after-theme-hack.PNG` is the one row that tells
  them apart — on a case-sensitive compile it leaves the object set and
  gains an eager URL). Parity: identical object row set on all six trees
  + asserted object count and by_name size; mutation: the .PNG pin, and
  one deleted glob empties the mindstorms gallery.

- [x] **I7b. Theme sources are not content.** A site-root `themes/` is
  engine vocabulary by POSITION (the class `.slots/`/`.section` already
  occupy; build reads themes from exactly one place). One line beside
  the config-file identity filter. Defers q34's other two skip lists.
  Parity byte-identical (every corpus site already excludes it —
  measured); mutation: the review I-B probe shape returns (a minimal
  site publishes /themes/mine/root.html). `include` stays the escape
  hatch. Corpus `exclude` lines stay (byte-inert tidy, unrequested).
  **Addition from I7a**: an objects collection's `exclude`/`include`
  configure NOTHING (`NotContent` reads the tree collection alone —
  proven by deleting theme-preview's `exclude = ["themes/**"]` on its
  objects scope and rebuilding byte-identical). Decide: error on the
  dead keys for non-tree scopes (the declared-and-ignored disease), or
  make them real; either way theme-preview's dead line goes.

- [x] **I7c. The gate becomes the fact; degenerate rows land.** One law:
  a row renders iff `front_mattered || shell ∈ {html, light_html}` — the
  second clause is the degenerate row (warn; slug-title pinned to the
  existing `slug.replace('-', " ")` derivation, run-not-reasoned against
  the caret page's title/og:title/search.bin). Loaders keep their shape;
  only the gate and title fallback move. **Two prerequisite byte-inert
  config migrations, measured pre-flight**: grack.com's `_drafts` rules
  gain `shell = "html"` (pairs with nothing in the base; resolves Null
  today, renders via the legacy layout fallback), and theme-preview
  (declines the base, declares no shell anywhere) gains declaration +
  rule defaults. Declared exception: ONE new stderr line (the caret
  degeneracy warning), named. Mutation: prettier de-hyphenation moves
  bytes on three surfaces (run it); delete the warning → silence; the
  control — field-notes' `demos/pane.html` is front-mattered AND
  `shell: raw`, so a shell-only law byte-copies it front matter and all:
  the test holds both halves of the disjunction.

- [x] **I7d. One walk, first rule wins.** The riskiest item, ALONE.
  `read_posts` + `store::load_dir` die; `walk_tree` is the one walk; the
  dot/underscore layer SURVIVES and declared scope sources punch through
  it (`_posts`/`_drafts` admitted because a scope names them; `_tools`/
  `_hidden` stay out because nothing does — §9b amended, not paid).
  Membership-by-precedence retires for one ordered rule sequence, with
  the order from the **most-specific-source law** (source specificity;
  sourceless extension-gated scopes above the root scope; ties by
  declaration order, site before base — declaration order ALONE is
  disqualified: theme-preview declares tree first and would eat its
  posts). **A scope owns its source**: a file under a scope's source
  that no rule of that scope claims is not content (reproduces today;
  keeps `_drafts/caret/`'s 18-file bundle invisible and its 16 relative
  img citations meaningless-as-today). The ordering must be observable
  (explain/--effective says which rule of which scope claimed a row).
  Keep the three-vector `insert_rows` interface (ordering-derived
  bytes: embeddings, related, tag order). Parity: all six trees byte-
  identical + a recorded `grackle urls` diff per site; mutations: drop
  scope-owns-source (the caret bundle enters — measure ON A BUILD),
  reverse two scopes, delete the punch-through (every post vanishes),
  minimal-site control.

- [x] **I7e. Objects dissolve; the Null shape collapses.** One row
  constructor for every row: former-object rows take rule/marker
  defaults and schema validation (propose-and-flag: markers DO reach
  them — refusing would re-mint the origin distinction; measured
  byte-inert, no corpus markers outside a fixture). The base catch-all's
  `shell = "raw"` reaches the 842 Null rows — log the new shell census;
  any row not `raw` is a finding. `by_name`/dimensions/`object_ix`
  re-key off extension; `RouteKind::Object` derives from the index so
  route filters don't move. The sitemap/search migrations are NOT taken
  — stated possible, flagged for Matt (§3's marker). Declared exception:
  CLI explain moves from `shell -` to `shell raw` for former objects
  (outside the byte gate). Mutations: by_name re-key (query stats
  collapses; gallery loses previews), locale selector on former-object
  rows (byte-inert today, latent — gets a test), marker-reach guard.

- [x] **IR6. The declaration walks skip `themes/` too.** *(I7b's finding
  3.)* The marker walk and the `.schema.toml`/`.section` vocabulary walk
  still descend `themes/`, so a theme shipping a `.schema.toml` would
  enter the site's field vocabulary — MERGE R1's `cover` leak, at a
  directory I7b declared engine vocabulary. Inert today (no repo theme
  ships one); mirror I7b's positional filter in `walker_declarations`
  (same `included` escape hatch), with an R1-style leak fixture proving
  closure both ways. Parity byte-identical; mutation-check.

- [x] **IR7. `explain` gains a `rendered` line.** *(I7c's proposal.)* The
  surface prints `front_mattered` and `shell` but not `rendered` — now
  the derived answer a reader most wants beside them (the law's output:
  `front_mattered || shell ∈ DOCUMENT`). One line in `debug::row_facts`;
  CLI-only; extend `io_explain.rs` (a degenerate row reads
  `front_mattered false / rendered true` — the pair that teaches the
  law). Mutation-check; standard parity.

- [x] **I8. Sidecars.** Identity from a sidecar file; governed rows for
  unparseable bytes; the identity/parsed split holds (`front_mattered`
  without content). Parity (no site uses one yet — fixture-driven).

*→ Batch review I-C.* ✓ done — findings in §11; verdict: sound, I-D clear.
**Veto digest: all seven rulings ENDORSED** (most-specific-source;
scope-owns-source with IR8 required as companion; markers-reach-objects;
implied_title-everywhere — vindicated by I8's sidecar probe;
the picture refusal unnarrowed; both-sources-error; root-scope asymmetry
KEPT per the reviewer's proposed answer, with IR8 as its observability).
Two follow-up items, run before I9:

- [ ] **IR8. The empty-claiming-scope warning.** *(Review I-C finding 1 —
  a real regression at the ownership law's edge.)* A typo'd glob on a
  sourced scope (`match = "**/*.markdwn"` over a populated `_posts/`)
  builds clean and silent with `posts 0` — pre-phase it was a load error.
  Scope-owns-source sends the unclaimed files out silently, and
  `dead_rules`' `found == 0` suppression (built for an ABSENT source)
  swallows the warning too. Fix: when a scope with a proper source was
  OFFERED at least one file and claimed ZERO, warn naming the scope, its
  source, and its rule globs. Warning, not error (an assets-only
  `_drafts/` is legal); keyed on found==0 so the caret bundle under a
  CLAIMING scope stays silent and stderr parity holds on all six.
  Residual honestly carried: a partial typo (one rule of several) stays
  silent; the census follow-up is `query stats`, not stderr, if wanted.
  Doc riders fold in: DESIGN §4b gains one sentence (the sidecar pair
  rule reaches non-content directories — the declaration walk's global
  reach); io_sidecar.rs's one stale mutation comment (describes the
  interrupted pass's intermediate state) fixed. Mutation: the probe site
  warns; absent-source and empty-dir shapes stay silent; parity.

- [ ] **IR9. An objects-scope rule may not declare `front_matter`.**
  *(Review I-C question 3 — the thrice-recorded corner, made live by
  I8.)* Pre-I8 the corner was vacuous (objects never peeked); post-I8
  the gate reads identity, so `front_matter = true` on an objects rule
  can claim a sidecar'd image while the next scope loses it. Config-time
  refusal, the I7b dead-key family: "an objects rule selects by shape;
  the identity gate belongs to scopes that parse." Kills the corner;
  ends the recording. Mutation both ways; parity.

### Phase I-D — the join and the graph

- [ ] **I9. The join fields.** `output` (record; canonical), `viewed_by`
  **[open: name — propose-and-flag]**, `inputs`; the `alternates` derived
  name; claimed rows visible as `!output`. Parity. **Amendments from
  review I-C**: (a) §2's `rendered` bullet is STALE (marked in place) —
  `rendered` is I7c's law and stays; `output` lands beside it, never in
  place of it. (b) State `output` for the three row shapes the phase
  created/sharpened: degenerate rows (land; front_mattered false),
  sidecar'd rows (identity without content; their output is bytes), and
  on-demand rows (URL computed, route minted post-load by
  materialize_referenced — the pull model says bare `output` is truthy
  only when referenced; say so explicitly and test it). (c) I7e kept the
  fact-keyed three-vector insert_rows interface and flagged I9 as where
  it "becomes a query or stays" — claim that decision, record it.

- [ ] **I10. The graph.** Planner builds nodes/edges upfront; cycle
  detection at load; invalidation keys derive from edges; serve becomes
  the pull (on-demand = unforced content stage). Parity + the serve
  behavior tests. **Amendments from review I-C**: the rung-0 residual
  stated at `force_route_fields` (routes minted post-load by
  materialize_referenced never see forced fields) is a graph-ordering
  question — close it or restate it here. Helpful fact: I8's version
  fold means sidecar edits already move `Row.version`, so invalidation
  edges get identity-changes for free.

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

**2026-07-27 — I6.** Landed as one commit. The item is a relocation, and the
two decisions worth vetoing are both about where a key LIVES rather than what
it does.

***[decided]* `filename_formats` stays legal at the collection level, as the
default its rules inherit.** The brief left it open and the corpus decided it:
twenty-four configs declare the key, every one of them at the collection level,
and grack.com's `_posts` declares it beside a rule that carries no route at all
(`hidden/**`, defaults only). Retiring the collection spelling would have been
a hard cutoff across all twenty-four for no capability — the capability is the
per-rule key, which is purely additive — and it would have put the same list on
two rules of the same collection wherever a scope has more than one. So the
shape is: **a rule's own `filename_formats` wins; absent one, the collection's**
— which is the bag-key-feeding-rule-defaults reading the brief named, and it
costs the merge laws nothing. `Vec` is `Shape::Atom`, so the collection key
merges as it always did (nearer writer takes it whole) and the per-rule key
rides its rule through the prepend, whole. **Resolution is first-writer-wins
across the MATCHING rules, not "whichever rule won the route"**: `filename_
formats` is a key like `defaults`, and §4's law is stated over keys. The two
readings differ only when one rule routes and another names the extractor, and
the law that already exists is the one that needs no new sentence. Zero corpus
churn: not one config line moved, which is the argument in its most checkable
form.

*One supplier, and the thing that made it small.* `RouteTokens` holds the path,
the row's date, the extractor's key and the slug, and answers one `get`. Both
loaders build one and call `render_all`; the tree loader's inline closure and
the posts loader's inline `match` are both gone. The tokens a route may spend
are now a fact about the type rather than about which loader you are in, and
the error that lists them reads the same list. **Path tokens are relative to
what the rule's own glob matches** — collection-relative in `_posts`,
root-relative in the tree — because the rule's `match` and its `route` should
read the same words; that is what makes q51's example say what it looks like it
says (`match = "rust/**"` → `/{dir}/{stem}/` → `/rust/hello/`).

*The extractor got honest about partial formats, and that was not optional.*
`FileKey`'s four fields are `Option`s now: a format yields what it NAMES. The
old key was built only when all four captures were present, so
`filename_formats = ["{slug}"]` — grack.com's `_drafts`, and the manual teaches
it — **matched nothing**, and the config survived by accident: the slug fell
back to the whole stem, which is exactly what `{slug}` captures. The accident
does not generalize, and the shape that proves it is `notes-{slug}`, which
would have kept the prefix in silence. Byte-inert on the corpus (the drafts
build is one of the six parity trees), and a unit test now holds the literal
case.

*The validation moved and generalized, and the mutation says what it buys.*
DESIGN.md §4 has always promised "undated row routed by a dated template →
error naming the file **and rule**"; the code named the file and the TEMPLATE.
It now names both, and the question generalized: any token the supplier cannot
fill for this row is the error, in whatever collection the rule lives. Measured
rather than assumed — **deleting the check is not silent and does not misroute**:
`template::render` still refuses an unresolved token, by *"template
`/blog/{year}/{slug}/` references unknown token {year}"*, a sentence about a
template rather than about a row. So the check buys the diagnosis, not the
refusal, and the test asserts the sentence (file, rule, reason, token) rather
than the failure. The dated case keeps an arm of its own because "this file
carries no date" is the diagnosis and "unfillable" is only the mechanism.

*One refusal deleted on purpose.* `collection {name} has kind=posts but no
filename_formats` is gone: a posts scope whose rules route by path tokens needs
no extractor, so that check would have refused exactly the config q51 exists to
allow. Its replacement is per row and per template, which is the rung the
question actually lives at. A test holds the shape (`a_posts_scope_needs_no_
extractor_when_no_route_spends_a_date`).

***[recorded]* Most-specific-source: handled, not decided — and left for I7.**
`_posts` sits inside the tree's `.`, and today nothing has to rule on the
containment because walk-level membership precedence keeps posts files out of
tree rules entirely (`store::walk_tree` skips `_`-prefixed names; DESIGN.md §3's
disjointness). Nothing in this item touches that, and the parity run is the
evidence. Building the general rule now would have meant inventing the
arbitration for a competition that cannot happen yet, and then re-deciding it
when I7 retires the precedence machinery for first-rule-wins — so q51's rider is
restated in DESIGN.md §11 pointing at **I7**, which is where the two sources can
first reach one file.

***[recorded]* The other half of one supplier waits for I7 too.** A post's route
date is front-matter-first (§4b, unchanged); a TREE row's route date can only be
the filename's, because that loader reads a page's front matter *after* routing.
Making the two identical means moving the front-matter read above the routing
block, which reorders which error a doubly-broken file reports — a change with
no caller today, in a loader I7 dissolves. Stated at the seam in code rather
than quietly left asymmetric.

*Six tests, five mutations plus a control, each red and each restored*
(`crates/grackle/tests/io_tokens.rs`): (1) q51's example, beside a dated row of
the same collection — mutation: drop `path_tokens` from the supplier and the
HEAD-era error returns while the dated row still routes, which is the
disjointness restored; (2) one template spending `{dir}`, `{year}` and `{slug}`
together, plus a `legacy/**` rule overriding the collection's format —
mutation: ignore a rule's own formats; (3) an extractor on a TREE rule (row
`slug` and `date` from the filename, not just the URL); (4) the moved
validation, naming file and rule — mutation: delete the check (the generic
sentence above); (5) the general unfillable-token sentence — mutation: stop
carrying the winning rule's pattern and both error tests lose the rule name;
(6) a posts scope with no extractor anywhere. The control is on the collection
default: `formats.unwrap_or(&[])` breaks dated routes corpus-wide *and* an
unrelated engine test, which is what says the default is still doing the work
every site relies on.

*Parity.* Five sites plus grack.com `--profile drafts`, HEAD's binary built in a
`git worktree` against this one, over the same content trees into separate
outputs so the binary was the only variable — byte-identical but for the feeds'
wall-clock `<updated>`, stderr identical for all six, file counts 8 / 8 / 83 /
242 / 1828 / 1829, unmoved since IR1. Build times interleaved (three runs each
on grack.com: 886/846/873 vs 844/860/824 ms), so the per-template token scan
costs nothing measurable. `cargo test` green (20 binaries); `cargo fmt --check`
clean under the pin; clippy 49 warnings, the warning SET byte-identical to
HEAD's rebuilt in the worktree; **zero re-blessing** — no fixture's expected
error moved, because the two messages that changed are reachable only from a
config no fixture writes.

*Docs.* DESIGN.md §4 gains **Route tokens: one supplier** (the token table, the
per-rule key with the collection as its default, the optional extractor, the
worked `legacy/**` snippet); its constraints bullet states the generalization
and what the check buys; §9b's still-owed list loses the `filename_formats`
obstacle from the single-tree entry; q51 keeps only its rider, pointed at I7,
per §11's convention that a settled half moves into the section that carries it.
Two config comments were corrected in passing, both byte-inert: `base.toml`'s
posts collection says the key is its rules' default, and grack.com's `_drafts`
block said *"so no `filename_formats` here"* three lines above a
`filename_formats` — pre-existing, and this item is what makes that key's
meaning worth stating correctly. **`manual/OUTLINE.md` untouched per §4, and for
once it needed nothing**: the collection-level spelling it teaches (line 795) is
still exactly right, so this is the first key change in the sequence that leaves
that file honest rather than one spelling staler.

*For batch review I-C.* Three things to probe. (i) **`{slug}` is now fillable on
every row**, tree rows included, where before it was a posts-only token — it
resolves to the stem when no format named one, which is what the posts loader
has always done and is why parity holds. It does mean a tree rule may spell
`{slug}` where it used to get an unknown-token error; the values are identical
to `{stem}`'s today, so nothing can tell them apart until an extractor names
one. (ii) **The extractor now reaches OBJECTS rules**, and an objects row takes
a `date` from its filename where a rule names a format. No corpus objects
collection declares one and the row column was `None` before, so this is
capability rather than change — but it is capability nobody asked for, and the
cheapest reversal is one line (`date: from_name` → `Default::default()`).
(iii) **First-writer-wins for `filename_formats` is independent of the route
rule**, which is the reading §4's law gives and not the only defensible one: "the
rule that routes the row names its extractor" would also be coherent, and would
differ exactly when a defaults-only rule sits above a routing rule — grack.com's
`hidden/**` is that shape, and declares no format.

**2026-07-27 — I7a.** Landed as one commit. A mechanism swap with no bytes in
it, and the two things worth vetoing are both about *scope*: how wide the case
ruling goes, and what a site can no longer say.

*The translation, per config — and the brief undercounted by one.* Five configs
declared `extensions`, not four: **theme-preview declares the same six at
`grackle.toml:171`** (the brief's "theme-preview's objects scope declares none"
is false against the tree — what it declares none of is a `source`, which is
true of every objects scope). Each list became the rule's own glob, verbatim
and in order:

| config | was | now |
|---|---|---|
| `base.toml` | six + `match = "**"` | `match = "**/*.{png,jpg,jpeg,gif,webp,svg}"` |
| `grackle.toml` (grack.com) | six + `ico`, three rules (`resource/**`, `{code,demos,writing}/**`, `**` on-demand) | each rule's glob gains `/*.{png,jpg,jpeg,gif,webp,svg,ico}` |
| `examples/field-notes` | six + `**` | `**/*.{png,jpg,jpeg,gif,webp,svg}` |
| `examples/raw` (`extends = "none"`) | six + `**` | same |
| `theme-preview` (`extends = "none"`) | six + `**` | same |
| four fixtures | `{png,jpg}` / `{png}` / `{png}` / `{png,jpg,jpeg}` | the same sets, as globs |

Verified against globset before relying on it: a leading `**/` matches zero
directories (`**/*.png` claims a root-level `x.png`), and a brace group
composes with `**` in the middle (`{code,demos,writing}/**/*.{png,jpg}` claims
`code/x.png` and `code/a/b/x.jpg`, and does not claim `codex/x.jpg`).

***[decided]* Case-insensitive, and on EVERY rule glob rather than the objects
scope's.** The brief flagged the ruling and named `assets/2004/06/after-theme
-hack.PNG` as the one row that can tell the two compiles apart. It is, and the
cost of getting it wrong was measured on a real build rather than argued: with
`case_insensitive(false)`, grack.com's object set drops **838 → 837**, `by_name`
**812 → 811**, and `/assets/2004/06/after-theme-hack.PNG` **appears in `grackle
urls`** — the eager tree catch-all claims what the objects scope let go, so a
published URL is minted that no rule asked for and the file loses its dimensions
and its index entry. That is a live-site change, and it is the whole argument
for the flag.

The scope of the ruling is the part a reviewer might narrow. Compiling only the
objects scope's globs case-insensitively would reproduce today exactly and
change nothing else; it was refused because it makes a rule mean one thing for
images and another for pages, decided by which scope declared it — and **I7d
merges the two lists into one ordered sequence**, where that distinction would
have to be justified or deleted. The positive case is that a `match` glob names
a KIND of file and the shift key is not part of the kind; on the
case-insensitive filesystems these sites are authored on it is not even
observable. The widening is real and stated rather than hidden: a front-mattered
`README.MD` would now render as a page where it used to byte-copy. Measured
inert on the corpus — 24 files repo-wide carry an upper-cased extension
(`.POV`, `.TGA`, `.EXE`, `.BAS`, `.WP`, …) and exactly one of them is claimed by
any rule of any site, the `.PNG`. Reversal is one line.

*The mechanism, and the one place it could not be the whole of `apply_rules`.*
`is_obj` is `obj_rules.iter().any(|r| r.matcher.is_match(rel))` — the GLOB only,
not the rule cascade. It cannot be the cascade: `apply_rules` consults a rule's
`front_matter` gate, and whether a file was peeked for front matter is decided
BY this answer (the peek is skipped for the ~800 binaries). That reproduces
today rather than approximating it — an objects rule gated `front_matter = true`
never routed anything before either, because an object's `has_front_matter` is
always false, so it claimed a row it could not route and the load failed on "no
rule supplies a route". The matchers are collected as a `Vec<&GlobMatcher>`
rather than read off `obj_rules`, because the closure runs inside the parallel
peek and a `CompiledRule` carries the `Cell<bool>` `dead_rules` writes.

***[declared]* A site can no longer NARROW the base's object extensions.**
`extensions` was an array and arrays are atoms (MERGE.md table A), so a site
writing `extensions = ["png"]` replaced the base's six. Rules **prepend**, so a
site's globs now ADD to the base's inherited `**/*.{png,jpg,jpeg,gif,webp,svg}`
and there is no spelling that takes one away short of `extends = "none"`. Every
corpus site declares a superset (grack.com's seven ⊇ the six; the other four
declare exactly the six, two of them with `extends = "none"` anyway), so the
merged membership set is identical everywhere and parity is the proof. It is a
real capability loss and it is Matt's to veto: the only fix is a rule-removal
mechanism, which I7d re-opens the whole ordering question for anyway. MERGE.md
table A's `[[collections]]` row was amended — `filename_formats` is its array
example now, with the retirement noted.

*What the shape guards did, which is what they are for.* Deleting the field
broke the build at `every_collection_key_has_a_law`'s destructure (A2) and then
failed `the_shape_covers_the_config_surface` until the `Shape::Struct` entry
went with it — both designed to fail on exactly this and both did.
`describe_collection` was the one message that had to be re-thought rather than
edited: it identified a sourceless collection by its extension list, and that
list is gone, so it names the rules instead (`"images" (no `source`; rules
"**/*.png")`). Listed rather than counted, because two objects collections in
one config differ in what their globs claim and that is the difference the
reader needs to tell them apart. This is the item's **one re-blessed fixture**
(`collection-two-objects/expected-error`).

*Three tests, four mutations plus a control, each red and each restored*
(`crates/grackle/tests/io_objects.rs` — built sites, not loaded ones, because
what membership BUYS is a header read, an index entry and on-demand routing,
and a test that asked the loader "is this an object" would pass against an
engine that published the file anyway). (1) the `.PNG` pin, on grack.com's shape
minimised — an on-demand objects scope over an eager tree catch-all — asserting
both halves: the upper-cased file is in the objects scope with its 2×3
dimensions, and its literal path is NOT published. Mutation: `case_insensitive
(false)` → both halves red, and the corpus measurement above. (2) the control,
which is what a narrowing item owes: case-insensitivity widens the case and
nothing else — `notes.TXT` is still not an object and still ships at its own
path (mutation: widen the rule to `**/*` → red). (3) the gallery, which is this
item's own edit run backwards: delete `jpg` from `gallery/**/*.{png,jpg}` and
the listing over the objects scope goes from two members to zero **while both
files still ship, at the same URLs, with the same bytes**, by the tree's
catch-all. The second half is asserted too, because the quiet is the point — a
build's file list cannot see this mutation, only a query over the scope can.
A fourth mutation crosses into the config surface: restore the `extensions`
field and both `an_unknown_config_key_is_a_parse_error` (the retired key, G1's
line) and `the_shape_covers_the_config_surface` go red.

*Parity.* Five sites plus grack.com `--profile drafts`, HEAD's binary built in a
`git worktree` against this one, into separate trees with seeded caches so
binary and config were the only variables — **byte-identical but for the six
wall-clock `<updated>` lines** (2 diff lines per feed, 0 of them anything else),
stderr identical for all six, file counts 8 / 8 / 83 / 242 / 1828 / 1829,
unmoved since IR1. **The object row SET, not just its size**, exported and
diffed per site: grack.com 838 rows identical (the `.PNG` present in both),
field-notes 15, theme-preview 9. Counts as the item asks them: objects /
`by_name` distinct names = 0/0 (minimal), 0/0 (raw), 15/15 (field-notes), 9/9
(theme-preview), 838/812 (grack.com, 26 ambiguous), 838/812 (drafts) — every
one unmoved. `cargo test` green (21 result lines); `cargo fmt --check` clean
under the pin; **clippy 48, one FEWER than HEAD's 49**, and the one that left is
the line this item deleted (`obj_exts.iter().any(|e| *e == ext)` →
"using `contains()` instead of `iter().any()`"); re-blessing limited to the one
expected-error above.

*Docs.* DESIGN.md §0's tour precedence line, §3's origin table, its objects
bullet and its three-step disjointness list, §4's example config (plus a new
paragraph stating the retirement and the case rule), §3's C7a paragraph, §5g's
"aren't `shell: raw` rows just objects" answer, and §6a's PARKED snippet (which
taught two dead keys and now says so). MERGE.md table A's `[[collections]]` row.
`manual/OUTLINE.md` untouched per §4 — it teaches `extensions` nowhere, so this
is the second key change in the sequence that leaves that file honest.

*For batch review I-C.* Four things. (i) **The case ruling's scope** is the call
to weigh — all rule globs, not just the objects scope's; the argument and the
one-line reversal are above. (ii) **The narrowing loss** is declared, not
solved. (iii) **theme-preview's objects scope declares `exclude = ["themes/**"]`
and that key is DEAD** — `NotContent` is built from the TREE collection alone
(`load.rs`, one `tree_c.map_or`), and `Collection::exclude` has no other reader,
so an objects collection's `exclude`/`include` configure nothing. Proven by
deleting the line and rebuilding theme-preview: byte-identical. Untouched here
(out of scope, and the site's tree collection excludes `themes/**` too, which is
what is actually doing the work) — proposed as an item, and it belongs beside
**I7b**, which is about that exact directory. (iv) **The extractor still reaches
objects rules** (I6's flag ii), and now those rules are extension-shaped globs;
nothing changed, but the two capabilities now sit on the same line of config.

**2026-07-27 — I7b.** Landed as one commit. Two rulings and no bytes: the
positional one the item is named for, and the dead-key one I7a handed it. The
interesting output of both is what the *measurement* said, because in each case
the reasoning and the observation disagreed about something.

*The ruling, and the one place it lives.* `build_tree_and_objects` gains one
filter beside the config-file identity filter: a path under a site-root
`themes/` is not content. The build reads themes from exactly one place —
verified, and it is two call sites of one path: `build.rs`'s
`Themes::load_all(&root.join("themes"), …)` and the per-theme CSS pass
`root.join("themes").join(name)`; `theme.rs`'s other three are the gallery test
helper and its own tests. So the directory is engine vocabulary by POSITION in
the same sense the config file is, and the two now sit as one layer of §4c
rather than as one identity filter and one thing every site has to remember.

**The evidence is that every site in the corpus already writes the rule.**
grack.com, field-notes and theme-preview all carry `exclude = ["themes/**"]`;
`examples/minimal` and `examples/raw` do not, and have no `themes/` directory
to be wrong about. A rule every site has to restate is the engine's — and the
one that does not restate it is not disagreeing, it is just not there yet.

*The disease, measured on HEAD's binary rather than on a mutant.* The review
I-B probe shape, built with the worktree's release binary: a minimal site with
a `themes/mine/` holding a `root.html` and a `theme.scss`, no `exclude` at all,
publishes **`/themes/mine/root.html` and `/themes/mine/theme.scss`** — the
theme's chrome fragment served as a page, and its stylesheet SOURCE served
beside the compiled sheet the same build already emits at `/css/mine.css`. The
new binary publishes neither, and the twin copy of the same two files at
`pages/mine/` still ships, which is the control the ruling owes: what those
bytes lose, they lose for their position and not for their name, their
extension or their shape.

***[answered]* `include` CAN override it, and that took no new machinery.** The
brief asked for the limitation to be recorded if it could not. It can:
`NotContent::keeps` gives `include` first say over `exclude`, so the positional
filter asks the same set the same way (`NotContent::included`, one accessor
over a globset that already existed). Probed live before it was tested — a
minimal site with `include = ["themes/**"]` and no exclude publishes both theme
files under the new binary. One pre-existing corner is worth writing down
because it looks like this rule's and is not: `include` cannot re-admit a
subtree *inside* an excluded one (`exclude = ["themes/**"]` +
`include = ["themes/mine/**"]` prunes at `themes/mine`, because
`walk_tree`'s directory filter asks `include.is_match("themes/mine")` and a
`themes/mine/**` pattern does not match its own root). That is R2-era
directory-pruning behaviour, unchanged by this item and unreachable from it —
a site using the hatch has no `exclude` to fight.

***[decided]* The dead keys: (i), the error.** `Collection::exclude`/`include`
have exactly one reader in the whole engine — `load.rs`'s two `tree_c.map_or`
lines, which compile the ONE `NotContent` the tree, marker and vocabulary walks
share — so a posts or objects collection writing either configures nothing.
(ii) was weighed and is not the same feature wearing a different scope: a posts
scope's `exclude` would have to mean "narrow my `source` walk" and an objects
scope's "narrow which files my rules may claim", which are two new semantics,
neither asked for by any site, and the second of which **a rule glob already
expresses** — narrowing what an objects scope claims is narrowing its `match`,
which is exactly what I7a just made the mechanism. So (i): a load error naming
the collection (through `describe_collection`, so a sourceless objects scope is
identified by its rules), the key, and where the patterns belong. theme-preview's
line went with it, and it is the repository's only one — verified by grepping
every `exclude`/`include` in every `.toml` and reading the collection each sits
under: **twelve lines, eleven of them on a `kind = "tree"`** (five configs,
seven fixtures), and after the removal eleven of eleven. Corpus `exclude =
["themes/**"]` lines STAY, per the brief — the ruling makes them redundant, not
wrong, and three fixtures carry the same now-redundant line for the same
reason.

*One message had to move with it, and it cost no re-blessing.*
`check_collection_kinds` told a site that a second collection's "rules,
`exclude`, `include` and `schema` would be silently dropped" — true for a tree,
and after this item false for objects, which may no longer write two of those
four. The list became per-kind. The two fixtures that assert this error
(`collection-two-objects`, `collection-two-trees`) hold only its first line, so
the tail moved with nothing to re-bless — checked before editing rather than
discovered after.

*Four tests, four mutations plus two controls, each red and each restored*
(`crates/grackle/tests/io_themes.rs` — built sites, not loaded ones, because
publishing is the disease). (1) the ruling, with the `pages/mine/` twin and
`/css/mine.css` asserted beside it — the theme still LOADS, this is a rule about
the walk; mutation: delete the filter and the probe's two URLs join the
published set. (2) the hatch; mutation: drop the `included` clause and it is
bolted shut with no spelling left. (3) the dead key in both spellings on both
kinds (`exclude` on objects, `include` on posts); mutation: delete the check's
call and both halves load clean, forever, which is the disease named. (4) the
control a narrowing owes — the TREE collection's `exclude` still decides what is
content; mutation: widen the check to every kind and this site stops loading.

*Parity.* Five sites plus grack.com `--profile drafts`, HEAD's binary built in a
`git worktree` against this one over the same content trees — **byte-identical
but for the six wall-clock `<updated>` lines** (2 diff lines per feed, 0 of them
anything else), `theme-preview` identical outright since it has no feed, stderr
identical for all six, file counts 8 / 8 / 83 / 242 / 1828 / 1829, unmoved since
IR1. Run twice, before and after a control-flow tidy in the new check. `cargo
test` green (22 result lines); `cargo fmt --check` clean under the pin; clippy
48, I7a's number; **zero re-blessing** — `git status` after the commit shows
nothing but the six files the item touched.

*Docs.* DESIGN.md §4c is now four layers, with a new subsection for the
positional one (the config file and `themes/`, the `include` hatch, and the
distinction `serve.rs` draws that is worth keeping straight: **not content is
not the same as not watched** — `is_content` deliberately treats `themes/` as
something to WATCH, which is the same fact from the other side, since a theme
edit changes every page). §3's two-collections paragraph and §4's objects
paragraph gain the tree-only key fact. §9's settled-ledger row 5 ("three
explicit not-content layers") is **left as written**, per G2's precedent that
ledger rows record what was decided when — it points at §4c, which now says
four. `manual/OUTLINE.md` untouched per §4, and checked rather than assumed:
its one `exclude` teaching is the script-shell gotcha (`add shells/** to
exclude`), which is the TREE collection's key on the site that ships it and
stays true word for word, and it teaches nothing about `themes/` at all. Third
change in the sequence that leaves that file honest.

*For batch review I-C.* Three things. (i) **The dead-key decision** is the call
a reviewer might reverse, and the argument against (ii) is above rather than
assumed. (ii) **q34 got sharper without being paid**: the literal `themes` now
appears in three places — `load.rs`'s new filter and `slots.rs`'s `SKIP`, where
it means the same thing, and `serve.rs`'s `is_content`, where it means the
OPPOSITE (an explicit exception so a gallery inside the grackle tree can
hot-reload). Recorded in DESIGN.md's q34 entry, because whoever ports those
lists has to keep the two senses apart and the count alone would hide it.
(iii) **The positional rule is content-only, deliberately.** The marker walk and
the `.schema.toml`/`.section` vocabulary walk still descend `themes/` — a
`.schema.toml` under a theme would enter the site's field vocabulary, which is
q34's disease one rung up (MERGE.md R1's `cover` leak) at a directory this item
just declared engine vocabulary. No theme in the repository ships one, so it is
inert today and unmeasurable from a build; the brief scoped this item to the
content filter, so it is filed rather than folded in.

**2026-07-27 — I7c.** Landed as one commit. The law is four words wider than
the gate it replaces, and everything interesting in the item is in what the
PRE-FLIGHT measurements said before a line of it was written.

*The two migrations, measured first and byte-inert both.* I2's method, and the
reason for it is that a config migration under a byte-parity gate is only
honest if the binary is held still: each config change was built with the
**unchanged HEAD binary** against the same content trees, before the law
existed. (1) grack.com's `_drafts` rule gains `shell = "html"` — that
collection is a second posts SOURCE with no twin in the base (`_posts` merges
with the base's posts collection and takes its `defaults = { shell = "html" }`;
`_drafts` pairs with nothing), so its four rows resolved no shell at all. Six
trees, byte-identical but for the wall-clock feeds. (2) theme-preview gains
`shell = { type = "string" }` in `[schema]` and the base's three rule defaults
spelled out — `html` on the front-mattered page rule and on the notes rule,
`raw` on the catch-all, **silence on the index rule for the base's own reason**
(a rule's defaults apply wherever it MATCHES, so a front-mattered `index.md`
takes `html` from the rule beside it). Byte-identical outright; it has no feed.
Only then did the law land on top.

*The census, and it is the item's most useful output.* Rows, per site, exported
from the HEAD binary and counted rather than reasoned about. **Rendered with a
Null shell — i.e. reaching `build.rs`'s legacy `Theme::parse(layout)`
fallback — was 19 rows in two sites**: grack.com's four drafts and every one of
theme-preview's fifteen. minimal, raw and field-notes: zero. After the two
migrations: **zero everywhere**. Shell census after, for the record:
theme-preview 15 html / 9 Null (its objects); grack.com 370 html / 187 raw /
1 light_html / 838 Null (its objects — I7a's number).

*Who still reaches the legacy fallback, which the brief asked for by name.*
**No corpus site, and it is not deletable.** Mutated to `panic!` and to an
`eprintln` and the suite answered: the fixture suite and the temp sites the
tests write reach it ~26 URLs' worth (`/manual/…`, `/recipes/`, the axis and
locale fixtures), because a site whose rules declare no shell still has
front-mattered rows, and those render by the law's FIRST clause and then need a
tier. So the arm has exactly one shape left — *renders by identity, no shell
resolved* — and it is a real rung, not a fossil: deleting it would declare a
default while removing the code that applies it. What IS a fossil is the
`layout: light` branch INSIDE it (`Theme::parse`), which no row on any site or
fixture now takes. Recorded at the line and in DESIGN.md §5g; **I7d/I13.**

*The degenerate row is one row, and it already rendered.* Review I-A's finding 2
was right to flag the parity exception as possibly vacuous: `_drafts/caret/…`
has been publishing `<title>why is a cursor called a caret</title>` for as long
as the loader has had its slug fallback, so making it a degenerate row moved no
bytes at all. The declared exception is therefore exactly ONE stderr line, on
grack.com's default AND drafts builds, and nothing else. The corpus's other
blockless `.md` files are the three `README.md`s under `demos/`, `code/legacy/`
and `writing/school/` — all `shell = "raw"` by the catch-all, all still byte
copies, all silent. That is the law's second clause doing its job by NOT firing,
and it is why "shell decides" and "identity decides" are both wrong.

***[decided]* The implied title is the rung for every RENDERED row, both
loaders — not for degenerate rows only.** The posts loader has applied it to
every post since before the ledger; making the tree agree is what turns I7d into
a relocation instead of a reconciliation, and it is byte-inert because **no
rendered row on any of the six trees lacks a title today** (measured, not
assumed). A byte row still gets none: its content is its bytes, so there is no
name to imply. The other reading — degenerate-only — would have left the two
loaders saying different things about the same question for one item.

*The derivation pin, RUN and not reasoned.* `slug.replace('-', " ")`, shared by
both loaders as `implied_title`. The mutation is a prettier form (title-case
each hyphen-separated word), built as a real binary against the real corpus, and
it moves **four files across the two profiles** — one more than the brief
predicted: the caret page's `<title>`, `og:title` **and its rendered
`<h2 data-slot="title">`**; the drafts profile's `/blog/page/66/index.html`, the
archive page that lists the draft; and `/search.bin`. The extra surface is the
listing, and it is the one a reasoned answer would have missed.

***[decided]* The rule key survives; its meaning narrowed.** `front_matter =
true` on a rule was the loader BRANCH — it is how a file became a page rather
than bytes, and the tree loader wrote `rendered: f.has_front_matter` to say it a
second time. It is now a **selector over one fact**: "this rule claims files that
carry identity". Whether the row is a document is a separate law over two facts,
stated once in `shell::renders`. DESIGN.md §4 gains *The gate is a fact, and the
rule key selects on it*; §5g's fallback paragraph, its `front_mattered`-vs-
`rendered` paragraph (§5) and the "why exactly these tiers" 2×2 parenthetical
were all made true rather than left standing — that parenthetical had said the
fourth corner was "mechanically unreachable", and it was one rule default away
the whole time.

*One asymmetry found and deliberately NOT fixed.* `read_posts` hands
`apply_rules` a constant `true` for the front-matter gate, so a blockless draft
is offered to `front_matter = true` rules it does not satisfy. Byte-inert (no
posts rule of any corpus site writes the key) and left alone with the reason
stated at the line: the shape that fixes it is the one walk, where there is a
single answer to hand over. **I7d.**

*Four integration tests plus four unit tests, five mutations, each red and each
restored* (`crates/grackle/tests/io_gate.rs`, `shell.rs`'s module tests).
(1) the degenerate row — a blockless `.md` under `shell = "html"` renders at
its rule's URL with the slug title, plus a `light_html` twin because the law
reads the FAMILY and a test naming one member would pass against an engine that
forgot the other; mutation: drop the shell clause and both byte-copy their own
markdown. (2) **the control** — a front-mattered `shell: raw` file, the
pane.html shape, plus an identity-less `raw` byte row beside it that must earn
no warning; mutation: drop the identity clause and the `---` block ships (and on
the REAL corpus, field-notes' `/demos/pane/` goes 521 → 571 bytes opening
`---\ntitle: Glass pane`). (3) the derivation pin, on `<title>` and `og:title`,
with an underscored and an ALL-CAPS name saying the rule is about hyphens alone.
(4) the rung — front matter beats the implied title, and only the blockless page
is degenerate; mutation: swap the arms. (5) `shell::degenerate` never fires →
silence returns while both pages still ship, which is the softening losing its
nudge. A sixth crosses into the vocabulary: widen `DOCUMENT` to include `raw`
and three unit tests go red at once.

*Parity.* Five sites plus grack.com `--profile drafts`, HEAD's binary built in a
`git worktree` against this one, into separate trees from the same content so
binary and config were the only variables — **byte-identical but for the six
wall-clock `<updated>` lines** (checked line by line: no atom diff is anything
else), stdout identical modulo timings, file counts 8 / 8 / 83 / 242 / 1828 /
1829, unmoved since IR1. **Stderr identical on four sites and one line longer on
grack.com's two**, the declared caret warning. `cargo test` green (23 binaries,
483 tests); `cargo fmt --check` clean under the pin; clippy 48, I7a's number
(one warning of my own was fixed to keep it there); **zero re-blessing** — no
fixture moved, and `git status` after the commit showed only the eight files the
item touched.

*Docs.* DESIGN.md §4 (the new gate section + a `Constraints` bullet for the
warning), §5's two `front_mattered` paragraphs, §5g's fallback paragraph and the
2×2 parenthetical. `base.toml`'s pages comment says the law and says what the
base does NOT do with it (it routes identity-less files `raw`, which is the
normal case). `manual/OUTLINE.md` untouched per §4, and checked rather than
assumed: it teaches neither the `front_matter` rule key nor `rendered`, so this
is the fourth change in the sequence that leaves that file honest.

*For batch review I-C.* Four things. (i) **The implied title for every rendered
row** is the call a reviewer might narrow to degenerate-only; the argument and
the measurement are above, and the change is one `match` arm in each loader.
(ii) **theme-preview's migration was not needed for correctness** — every one of
its rows is front-mattered, so all fifteen render by the first clause either way.
It was taken for the reason the brief gave (drain the legacy fallback and make
the site's serialization a declared fact), and it is the one config change in the
item that no row's behaviour depended on. (iii) **`front_mattered` is still
computed by a 4-byte peek at the head of the file** (`store::peek_front_matter`),
and it is now a *rendering* decision rather than a parsing one — a file opening
with `---` that is not valid YAML is a load error, which is right, but the fact
that the law's first clause is decided by four bytes is worth one reviewer's
glance. (iv) **A proposed item, not filed as a chip** per §10: `grackle explain`
prints `front_mattered` and `shell` but not `rendered`, which is now the derived
answer a reader would most want beside them — one line in `debug::row_facts`,
CLI-only, outside the byte gate.

**2026-07-27 — I7d.** Landed as one commit. Two loaders became one walk, and
the two flagged laws both landed — but not in the shape the brief predicted:
**the ordering and the ownership turned out to be two mechanisms doing two
jobs**, and the first version of this item had them doing one, which a mutation
run is what exposed.

*The two laws, as landed.* `load::walk_site` offers each file to every scope in
`scopes()`' order — each scope reading the path its OWN source makes of the
file, and skipping the file when it is not under that source — with each
scope's rules in its own order. **First rule past both gates claims**, and the
claiming rule's scope is the row's collection; that scope's rules then cascade
defaults exactly as they always did, because membership is the only question
the sequence answers. The order is the **most-specific-source law**: proper
sources deepest first, sourceless scopes (objects) next, the root scope last,
ties by site-before-base then table name. **A scope owns its source** is the
second law and it is a STOP, not a filter: when a scope whose source contains
the file is asked and claims nothing, the search ends there and the file leaves
the walk.

*Why the shape had to change, and it is the item's most useful output.* The
first implementation expressed ownership as ELIGIBILITY — under a proper
source, only that source's scopes are asked at all — which is correct, produces
identical bytes, and makes the ordering law **untestable**. The mutation said
so: reversing the scope order left every test green, because a file under
`_posts` never reached the tree scope to be misclaimed by it. Two laws where
one of them cannot be mutated is one law with a decoration, so the sequence now
asks every scope in order and ownership stops it. Both mutations are
independently red now, and the second one measures: with the sort key dropped
(name order alone — theme-preview's disease), **all 327 of grack.com's posts
move to `/_posts/…` URLs**, claimed by the tree's front-matter rule.

*The ties are unobservable, and that is why declaration order is not
recoverable.* `Config::collections` is a `BTreeMap` keyed by table name, so
literal declaration order is gone by the time the loader sees it. It does not
matter: two scopes tie only when their sources are equally specific, a scope
only ever sees files under its own source, and two scopes sharing one source
would share one table name and be one entry. The tie-break that landed is
site-before-base (read off `Rule::inherited`, `dead_rules`' own test) then the
table name — deterministic, and as near the brief's "declaration order" as a
config keyed by name can get.

*The config migration: every posts-scope rule names its extensions.* I7a's move
one scope over. `match = "**"` → `match = "**/*.{md,markdown}"` in `base.toml`,
grack.com's two posts scopes, theme-preview, field-notes, `examples/raw` and
thirteen fixtures — eighteen rules. It is what makes the ownership law
expressible: `store::load_dir` took `["md", "markdown"]` as an ARGUMENT, so the
statement "a posts scope claims markdown" lived in a function signature where
no config could see it. **grack.com's `hidden/**` deliberately did NOT
migrate**: it is a defaults-only rule scoping a directory, it says nothing
about kinds, and its glob is quoted in the dead-rule warning grack.com prints
on every build — migrating it would have moved stderr. The corner it leaves is
recorded at the line: a non-markdown file under `_posts/hidden/` would now be
claimed and then fail *no rule supplies a route* rather than being skipped. No
such directory exists (the rule has governed zero rows since it was written).

*One guard the brief did not ask for, and the reason it is not scope creep.*
**A tree `exclude` that names a scope's `source` is a load error.** `exclude`
says "not content" and `source` says "content"; before this item they governed
different walks, so the contradiction was not merely silent but harmless — the
dot/underscore skip kept `_posts` out of the tree anyway, so
`exclude = ["_posts/**"]` was a line that did nothing, and three fixtures
carried one. With one walk it empties the scope, and the first run of the suite
is how I found out: the three `per-group-*` fixtures failed with a grouped view
that had no rows to group. Silently emptying a blog is the disease this ledger
exists to refuse, so the line is refused instead and the three fixtures lost
it. Keyed on `NotContent::keeps_dir(source)`, which is the subtree spelling. One
residual, recorded rather than closed: a FILE-shaped exclude pattern reaching
inside a source (`exclude = ["_posts/*.md"]`) would still empty the scope
quietly; no corpus site writes one, and the check keys on the source because
the source is the statement that contradicts.

*Four unifications the merge forced, each byte-inert and each measured.* A
merged loader cannot keep two answers to one question, so each of these picked
one:

- **`body_bytes` on a blockless row that renders.** The posts loader read every
  post whole and had it; the tree loader read nothing and reported zero. The
  walk reads a file with a block, and re-reads a blockless one only if it turns
  out to render — so the caret draft keeps its real body size, which is what
  the corpus needs, and the fact stops depending on which loader found the row.
- **`permalink:` now reaches tree rows.** It was posts-only — the tree loader
  parsed the field and ignored it, contradicting DESIGN §4's "an explicit
  `permalink:` in the file wins outright". Measured before relying on it: no
  file in the repository carries one.
- **Front matter is read ABOVE routing**, so a `date:` reaches a dated route
  template on any row. That is I6's recorded other half of "one supplier", and
  the seam it named. It reorders which error a doubly-broken file reports; no
  fixture is such a file.
- **The shell error names the row relatively**, the tree loader's spelling
  (`feedish.md: shell = "atom" is a fold shell`), not the posts loader's
  absolute path. Chosen because a fixture pins it, and because it is the better
  message.

*What the objects globs still answer early, stated rather than hidden.* Two
facts must be settled before the ordered sequence runs, and neither is the
sequence's to settle because both come before the front-matter gate the
sequence consults: **the peek** (whether a file was peeked is what the gate
reads, so it cannot itself be gated — skipping the ~800 binaries is what keeps
the peek off the critical path) and **the locale axis** (an image is shared
across locales, §6f). Both are I7a's `is_obj`, unchanged: the objects scope's
globs, asked on their own. The two answers agree with the sequence's because no
objects rule of any site gates on front matter; one that did would take the
glob's answer here and the gate's answer there. I7a recorded the same shape
(such a rule claimed nothing before either).

*Observability: `Row.rule`, and `explain` prints it beside `collection`.* An
ordering law nobody can observe is an ordering law nobody can debug. `rule`
holds the `match` glob of the rule that CLAIMED the row — which is not always
the rule that ROUTED it, since a defaults-only rule claims files it does not
land — so the pair `collection` + `rule` answers "which rule of which scope"
directly:

    url         /humans.txt          url         /blog/2020/01/01/hello/
    collection  entries              collection  posts
    rule        **/*                 rule        **/*.{md,markdown}
    shell       raw                  shell       html
    front_mattered false             front_mattered true

`io_explain.rs`'s two-row assertion covers it (mutation: hardcode the glob and
the byte copy's line goes red).

*Six tests, six mutations plus two controls, each red alone and each restored*
(`crates/grackle/tests/io_walk.rs` — built sites, not loaded ones, because what
these laws decide is what the site PUBLISHES, and dropping ownership turns the
bundle into ON-DEMAND object rows that only a build materializes). (1) the
ownership law, with the draft's relative citation included so the on-demand
path is live; mutation: widen `eligible` so an owned path also reaches the
sourceless and root scopes → the `.gif` and the `.rtf` join the published set.
(2) the control ownership owes — the same two extensions at the ROOT are an
object and a byte copy, asserted through `claim()` so the scope AND the rule
are named. (3) the ordering, on theme-preview's shape (tree declared first);
mutation: sort by name alone → the post is claimed by `entries`. (4) the
punch-through beside `_hidden/` and `_includes/`, which is §9b's amendment in
one assertion; mutation: `punches_through` → `false`. (5) the minimal-site
control: no declared source, so nothing punches through and an undeclared
`_posts/` is just another underscore directory — green under every mutation
above, which is what makes it the control. (6) the exclude/source
contradiction; mutation: delete the `keeps_dir` loop → the site loads clean and
publishes a blog with no posts. A unit test in `store.rs` pins the
punch-through's whole-component comparison in both directions (grack.com has
`_drafts` and `_drafts_temp` side by side; mutation: a string prefix → the
second is walked).

*The corpus measurements, run rather than reasoned.* Dropping the ownership
stop, with the release binary against the real tree: rows **1396 → 1413**
(objects 838 → 853, static tree rows 187 → 189), and the BUILD publishes two
files it did not before — **`/caret/caret.xcf` and `/caret/caret2.rtf`**, at
`/caret/…` rather than `/_drafts/caret/…` because the tree's `{path}` token is
scope-relative. The fifteen images do not appear even then: the draft cites
them relatively from `/drafts/…/`, so the references do not resolve and the
on-demand rows are never materialized — which is the brief's "meaningless as
today", now measured from the other side. Deleting the punch-through: posts
**1396 → 1065**, dated **327 → 0**.

*Build time (the R10 concern: a per-file front-matter peek where the posts
loader read whole files).* Five interleaved runs on grack.com, HEAD's binary
against this one, I6's method — build **586/871/841/827/897 ms** vs
**838/852/841/824/806 ms**; the load's own `read+parse` **30.8/61.1/62.4/62.5/
65.8 ms** vs **35.8/65.8/64.8/64.3/60.9 ms** (each first run cold). Inside
run-to-run variance in both directions: nothing measurable. The peek was
already running over the whole tree; what this item adds is ~330 four-byte
opens under `_posts` and `_drafts`, and it removes 330 whole-file reads' worth
of front-matter parsing done twice.

*Parity [required, absolute].* Five sites plus grack.com `--profile drafts`,
HEAD's binary built in a `git worktree` against this one, into separate trees
from the same content with caches seeded so binary and config were the only
variables — **byte-identical but for the six wall-clock `<updated>` lines**
(2 diff lines per feed, 0 of them anything else; theme-preview identical
outright, having no feed), **stderr identical on all six** (the I7c caret
degeneracy line and the `hidden/**` dead-rule line present on both sides of
grack.com's two), file counts 8 / 8 / 83 / 242 / 1828 / 1829, unmoved since
IR1. **The `grackle urls` set-diff was recorded per site and is EMPTY on all
six**: 7 / 7 / 222 / 63 / 1372 / 1373 URLs, zero diff lines. `cargo test` green
(24 result lines); `cargo fmt --check` clean under the pin; **clippy 47, HEAD's
48 minus one**, and the one that left is the arg-count warning on
`build_tree_and_objects`, the function this item deleted; **zero re-blessing** —
no `out/` file and no `expected-error` moved.

*Docs.* DESIGN.md §3's *Membership is disjoint* rewritten as *one walk, first
rule wins* (both laws, the order, the root-scope asymmetry, the explain pair);
§0's tour precedence line; §3's origin table; §4's C7a paragraph (a posts
scope's source now does three jobs, a tree's still does none); §4c's layer
table plus two new paragraphs (the punch-through, the exclude/source
contradiction); §4's route-token paragraph (I6's seam, closed); §9b's
still-owed single-tree entry — the walk half is built and both measured
obstacles are settled, the underscore one **amended rather than paid**; §9b's
"loader collection choice" remainder closed; **q51 moves out of the open list
into the settled ledger**, its rider decided. Config comments: `base.toml`,
grack.com (the objects scope's ordering, the `_drafts` bundle, the three
layers, `hidden/**`), theme-preview, field-notes, `examples/raw`.
`manual/OUTLINE.md` untouched per §4, and checked rather than assumed: it
teaches neither the membership precedence nor `_posts` as a walk of its own,
and the one thing it does teach here — `permalink:` overriding every rule — is
now MORE true than it was, since it reaches tree rows. Fifth change in the
sequence that leaves that file honest.

*For batch review I-C.* Five things. (i) **The two laws** are the calls to
weigh, and the ordering is the one with a reversal cost of one line
(`scopes()`'s sort key). (ii) **The root scope's asymmetry** — an unclaimed
file under a proper source leaves silently, an unclaimed file under the root
scope is *no rule supplies a route* — is a decision, not a derivation. The
argument is that a proper source is a NARROWING (`source` plus `match` is one
statement in two keys, and a `.png` beside a draft was never being refused, it
was never being asked about) while the root is the site. The other reading —
uniform silence — deletes a real refusal, and uniform refusal would put
eighteen errors on grack.com. (iii) **The exclude/source load error** is new
strictness the brief did not ask for; the alternative was a declared-and-
ignored key, and the residual (a file-shaped pattern inside a source) is
recorded above. (iv) **`permalink:` reaching tree rows** is a capability the
merge added rather than a change it needed; it is byte-inert on the corpus and
makes DESIGN §4 true, but it is worth one reviewer's glance. (v) **The objects
globs still answer two questions before the sequence runs** (the peek, the
locale axis); it is I7a's `is_obj` unchanged, but it is the one place where
"the sequence decides membership" is not the whole truth, and I7e is where the
object constructor it feeds gets folded in.

**2026-07-27 — I7e.** Landed as one commit. The branch that went was thirty
lines; what it was hiding was that **a row's origin decided what a row could
carry**, and the census is how you see it — 862 rows across three sites
resolved no shell at all, not because anyone declared one and not because
anyone declined to, but because nothing ever asked them.

*The census, and the one row that was a finding.* Rows per site, exported and
counted before and after, never reasoned about:

| site | before | after |
|---|---|---|
| `examples/minimal` | 3 html | unmoved (no objects) |
| `examples/raw` | 3 html | unmoved (no objects) |
| `theme-preview` | 15 html / **9 Null** | 15 html / 9 raw |
| `examples/field-notes` | 26 html / 1 raw / **15 Null** | 26 html / 16 raw |
| grack.com | 370 html / 1 light_html / 187 raw / **838 Null** | 370 / 1 / 1025 / **0** |
| grack.com `--profile drafts` | as above | as above |

Zero Null anywhere. The brief said any row that is not `raw` after the collapse
is a finding, and on the first run there was exactly one:
**`resource/favicon/favicon.ico`**. The base's objects glob names six
extensions; grack.com's three rules add a seventh (`ico`), and I7a's declared
consequence — a site's rules ADD to the base's, they cannot narrow them — means
the seventh extension had no rule beneath it carrying a default. So the one row
whose membership came from the site alone was the one row the base could not
answer for. Its three rules now declare `shell = "raw"` themselves, with the
reason at the line: **a scope that widens the base's membership owns the
defaults for what it widened it by.** It is a two-line config fix and a real
hole in how inheritance and defaults compose; a reviewer may want it generalized
(there is no check that a rule's glob is covered by *some* rule declaring the
engine-read keys, and there could not easily be one — silence is legal).

*The config migration was measured first, with the binary held still.* I7c's
method: `base.toml`, `examples/raw`, `theme-preview` and grack.com took their
`defaults = { shell = "raw" }` lines and were built with the **unchanged HEAD
binary** against the same content trees — byte-identical but for the wall-clock
feeds, stderr and URL sets identical on all six. That separates "the config
moved bytes" from "the code did", and I2 had already predicted the answer: its
`an_object_row_answers_no_shell_at_all` recorded that adding that very line
"moves nothing", because nobody read it. This item is what makes it read.

*What survives of "object", and I7d's flag 5 answered.* The extension fact:
the objects scopes' globs, asked on their own — I7a's `is_obj`, unchanged in
what it computes. I7d flagged that these globs "still answer two questions
before the sequence runs" and called it the one place where *the sequence
decides membership* is not the whole truth. The honest resolution turned out
not to be removing the pre-answer but **noticing it was never a pre-answer**:
the sequence answers *which scope claims this row*, and the globs answer *is
this row a picture*. Two questions, two answers, and the second one now keys
the three things that were ever really about pictures — `object_ix`, `by_name`,
and the header read that fills `width`/`height` — where before they keyed off
which vector the loader pushed the row into. `RouteKind::Object` still derives
from the index, so no route-kind filter moved (`query stats`: objects 838,
distinct names 812, ambiguous 26, route kind `object` 631 — every number
unmoved). The three readers were verified one at a time: the listing pass's
`ctx.objects` picture preview (asserted and mutated in `io_dissolve.rs`), the
gallery thumbnail eligibility in `thumbs_pass` (which keys on the VIEW's base
kind, not on the index — field-notes' `/photos/` and grack.com's mindstorms
gallery byte-identical, 260 thumbs both sides), and the narrow `object_schema`
dispatch in `views::Base::resolve` and `relations.rs` (which keys on the
collection kind — `where = "draft"` on a gallery is still a load error).

**The one corner where the two questions could disagree**, stated at the code
rather than guarded: an objects rule gated `front_matter = true` would be
claimed by whichever scope came next while still being indexed as a picture.
No site writes one, and I7a recorded that such a rule claimed nothing before
either.

***[decided]* Markers reach a former-object row.** The propose-and-flag call,
and the argument is that the only available reason to refuse is *which
constructor built the row* — which is the distinction this item exists to
delete. A `.hidden` beside a gallery means what it says. Measured byte-inert
rather than assumed: `query stats` reports **0 marker files on all five
sites**, so no image in the corpus sits under one. The guard is a fold over the
ROUTE pool (`where = 'hidden'` selects the image's URL), because a marker that
wrote a field nothing could read would be the declared-and-ignored disease with
extra steps.

***[decided]* The locale selector still does not run on one, and it is pinned
both ways.** One picture serves every locale (§6f), so `photo.fr.png` is a file
whose name carries a dot rather than the French edition of `photo.png`. Letting
the selector run is byte-inert on the corpus **today** — no `.fr.`-infixed image
exists on any of the six trees — and latent forever after, which is what earns
it a test rather than a note: measured on the mutant, the image is republished
at `/fr/gallery/photo.png` and its literal path leaves the URL set. Localized
images are a feature to ask for, not one to acquire the first time somebody
names a file that way; the reverse mutation (always take the object arm) is in
the test's doc too, because it costs the `.md` beside it its French edition.

*What the constructor gave the rows it used to skip, beyond the shell.* Each
was checked for a byte consequence rather than assumed inert: declared fields
and `images` (from rule and marker defaults — empty on the corpus), rung 0's
forced fields (the drafts profile's `noindex`, which object ROUTES already
carried via R6's `force_route_fields`), `logical` (read only by `by_logical`,
which gates on `rendered`), `route_templates`/`axis` (the route constructor
gives an unrendered row no axes), and the q45 claim check (an image cannot be a
view's content — it has no front matter, so the check that used to be skipped
now produces the better error). `body_bytes` stays 0 and `title` stays None,
both by the laws I7c wrote rather than by an arm of their own.

***NOT taken, stated.*** The sitemap and search filter migrations are reachable
now — the column is total, so `shell == "raw"` and `shell == "html"` finally
mean something on every row. They are a byte change to two live artifacts and
they stay Matt's call per §3's shipped/pending marker. DESIGN.md §4e records
that the first of I2's two Null shapes is closed and that the migration was
declined here rather than forgotten.

*Four mutations, each red alone and each restored* (`crates/grackle/tests/
io_dissolve.rs`, plus I2's remainder inverted in `io_shell.rs`). (1) The
partition keyed off the fact — send former-object rows to `pages` instead: the
gallery keeps its three members and loses every one of its links, because a
picture answers `title` with nothing and rule 2 deletes the `<a>` with its empty
label; `by_name` goes to 0 distinct / 0 ambiguous; the header read never runs.
The MEMBERSHIP half deliberately survives that mutation (the row's `collection`
is still the objects scope's), which is what makes the test about the index
rather than about the query. (2) Markers refused when `object_shaped` — the
origin distinction re-minted in one line — and the image leaves the hidden set
while the `.md` beside it stays. (3) The locale selector, mutated in both
directions, above. (4) The constructor restored, and separately the base rule's
`defaults` line deleted: Null returns either way, which is the two halves of
"the gap was in the loader" and "the fix is declared in config".

*Parity [required, absolute].* Five sites plus grack.com `--profile drafts`,
HEAD's binary built in a `git worktree` against this one, into separate trees
from the same content with caches seeded — **byte-identical but for the six
wall-clock `<updated>` lines** (4 diff lines per feed, 2 of them the timestamp
itself, 0 anything else; theme-preview identical outright, having no feed),
**stderr identical on all six**, file counts 8 / 8 / 242 / 83 / 1828 / 1829 and
the **`grackle urls` set-diff EMPTY on all six** (7 / 7 / 222 / 63 / 1372 /
1373), both unmoved since IR1. Stdout differs on the four small sites by one
line — `embedding N changed posts…` — which is cache warmth in the two trees
and not the binary: the outputs it produces are byte-identical. `cargo test`
green (25 result lines); `cargo fmt --check` clean under the pin; **clippy's
warning set byte-identical** to HEAD's rebuilt in the worktree; **zero
re-blessing** — no fixture and no `expected-error` moved.

*Docs.* DESIGN.md §3 loses its **origins table** and gains a **key-list table**
(what puts a row in each list, and what each buys) plus the sentence that says
a gallery selects by SCOPE while the index selects by the extension fact, and
where they could disagree; §3's objects bullet; §4e's I2 paragraph (the first
Null shape closed, the sitemap migration declined rather than forgotten); §5g's
tier table gains the amendment that `object` is not a fourth exit but the `raw`
one taken by an unrendered row — the two lines differ in `rendered`, and the
question *"aren't `shell: raw` rows just objects?"* used to be answerable by
reading `shell` and now is not; §9b's **objects dispatch** entry is struck
through with the record of both halves (the view half at the materializer
merge — its last sentence, "`group_by`/`paginate` still bail there", had been
false since — and the load half here), and §9b's single-tree entry says the
TABLE half is built and only the join remains. `crates/model`'s doc comments
for `object_ix`, `objects()` and `insert_rows` say facts instead of origins.
`manual/OUTLINE.md` untouched per §4, and checked rather than assumed: it
teaches neither the objects table nor `shell` on an image, so this is the sixth
change in the sequence that leaves that file honest.

*For batch review I-C.* Five things. (i) **The two rulings** (markers reach;
the locale selector does not) are the calls to weigh, and each is one line to
reverse. (ii) **The `.ico` finding** is the shape worth a second opinion: a
site that widens the base's membership silently inherits no defaults for what it
widened by, and the only reason it was visible is that the census demanded every
row answer. (iii) **The partition still passes three vectors to `insert_rows`**,
which is I7d's interface kept on purpose (ordering-derived bytes); the vectors
are keyed off facts now, so what is left is a shape rather than a decision —
I9's join is where it either becomes a query or stays. (iv) **`explain` moves
for ~60% of grack.com's rows** (`shell -` → `shell raw`), declared and outside
the byte gate. (v) **The one corner where the extension fact and the sequence
could disagree** (an objects rule gating on front matter) is stated in code and
guarded by nothing; it has now been recorded three items running (I7a, I7d,
here), which is either the right amount of honesty or a sign it should be a
load error.

**2026-07-27 — IR6.** Landed as one commit. The premise held, but not where the
item filed it, and the interesting part is the boundary the fix had to be asked
at rather than the fix.

*The premise, re-verified against the current tree.* I7c/I7d/I7e rebuilt the
loaders around it and moved none of it: `store::walker_declarations` is still
the ONE walk both declaration passes share (`Markers::scan`, and the
`.schema.toml`/`.section` pass in `load`), its only filter was still
`NotContent::keeps_dir` — the declared layer — and I7b's positional filter was
still where I7b put it, in the content path (`walk_site`'s
`not_content.included(&f.rel) || !under_themes(&f.rel)`). So the leak was live
exactly as filed. It is also **inert on the corpus and measurably so**: two
`themes/` directories exist under a site root (grack.com's and the workspace's),
and a `find` over the whole repo puts every `.schema.toml` and `.section` —
three real, five fixture — outside all of them, with zero marker files anywhere
(A5's corpus note, still true). Parity was therefore expected to be free, and
was.

*Why the prune is asked at `themes` and not at `themes/<child>`.* This is the
one design call in the item, and R2 already made it: a pruning walk gets no
second chance at the files inside, so the question has to be asked about the
directory *that contains them*. Ask it one level down — at `themes/mine`, which
is where a theme's own declaration obviously lives — and a `.schema.toml`
sitting directly in `themes/` is read, which is R2's first-level blindness
reproduced in a new place by someone who read R2. So the fixture's broken file
is at `themes/.schema.toml`, and that mutation is one of the five below.

***[decided]* The hatch applies uniformly, and the coherence question is the
site author's.** The brief asked whether `include`ing something under `themes/`
even means anything for declarations. It does, and the argument is not about
declarations: the value of one escape hatch is that there is one of it, and a
key that admitted bytes but not vocabulary would be two rules wearing one name —
a site would have to learn which of its `include`d files are "content-included"
and which are "declaration-included", a distinction the config has no word for.
So `include` opens both, and what a site does with a theme's field declarations
after saying so is its own business. It is asked as the DIRECTORY question
(`NotContent::included_dir`, `keeps_dir`'s empty-child idiom): `themes/**` — the
one spelling every site in the corpus writes — matches `themes/` and never
`themes`, so the file question would have bolted the hatch shut for the only
form anyone types. That is a second-order R2 and it is guarded by its own unit
test.

*The marker half is NOT unmeasurable, which is where this differs from R1.* R1
recorded that a marker under an excluded directory governs only excluded rows,
so nothing observable moves and there is no test to write. The same is true here
of *output* — no row under `themes/` is content since I7b — but `markers.found`
is a census, exported as `db.stats.markers`, and a marker the walk should not
have seen is one the census should not count. So the marker half gets a real
assertion (1 with the rule, 2 without) rather than R1's parity-is-the-test.

*One fixture, two load-level tests, five mutations, each red and each restored.*
The fixture (`theme-schema`) is `excluded-schema`'s two-part shape transplanted:
a host `[routes.covered]` whose `where` names `cover`, declared ONLY under
`themes/mine/`, and a deliberately-broken `themes/.schema.toml` at the boundary.
The expected error — `unknown field cover` — proves both halves at once: that
`cover` did not enter the vocabulary, and that its being *that* message rather
than a TOML parse error means nothing under `themes/` was read at all. The site
writes **no `exclude`**, deliberately, so it cannot pass by restating the rule
the engine is supposed to know. Mutations: (1) delete the arm → parse error on
the boundary file; (2) delete the arm AND remove the boundary file → the fixture
builds successfully, which is `cover` type-checking in the host's `where` — the
leak itself, seen alone; (3) ask at `themes/<child>` → the boundary file is read,
parse error; (4) drop `included_dir` from the arm → the hatch is shut; (5) weaken
it to `included` → the hatch opens for nothing a site would write. The two
`io_themes.rs` tests read the census (vocabulary, section list, marker count)
off a LOADED site rather than a built one — the inverse of I7b's discipline, and
for the same reason: a declaration publishes nothing, so a built site is the
test that cannot see it — with the twin at `pages/mine/` declaring the same
three things in the same words as the control.

*Parity.* Five sites plus grack.com `--profile drafts`, HEAD's release binary
built in a `git worktree` against this one over the same content trees —
byte-identical but for the feeds' wall-clock `<updated>`, stderr identical for
all six, file counts 8 / 8 / 83 / 242 / 1828 / 1829, unmoved since IR1. Run
twice, before and after a needless-borrow tidy. `cargo test` green (25 result
lines); `cargo fmt --check` clean under the pin; clippy 47, HEAD's number (the
first draft's `&rel.join("")` made it 48 and was fixed rather than carried);
**zero re-blessing** — `git status` after the commit showed nothing but the six
paths the item touched.

*Docs.* DESIGN.md §4c's position row now says "content *and* declarations"; the
positional subsection gains a paragraph for the declaration walks (with the
at-`themes`-itself reasoning) and the hatch paragraph gains the uniformity
argument and the two questions. **q34's census still reads three**, and that is
the item's one tidy: the literal was about to appear a fourth time, so it is
named once (`store::THEMES`) and the two readers that mean *not content* —
`load.rs`'s `under_themes` and the new prune — take it from there; `slots.rs`'s
`SKIP` and `serve.rs`'s opposite sense are untouched and still the thing q34 is
about. `manual/OUTLINE.md` untouched per §4, and checked rather than assumed —
this one needed the check, because unlike the last three it DOES teach both
words. Ch. 22 teaches positional `.schema.toml` and ch. 13 teaches `themes/`
(install by `cp -r`, one directory per theme), and they never meet: nothing in
it says a theme may declare fields, and its one sentence about where a
declaration reaches (ch. 29's "only type-checks against the recipes subtree's
`.schema.toml`") is about narrowing within the site, which is I7a's declared
loss and not this. Honest word for word, fourth in the sequence.

*For batch review I-C.* Three things. (i) **The uniform hatch** is the call to
weigh; the argument is above and the reversal is one clause. (ii) **`themes/` is
now the only positional name the declaration walks know**, and the asymmetry is
worth seeing: `.slots/` is not pruned there (it has its own walk with its own
`SKIP`), and a `.schema.toml` inside a `.slots/` directory would still be read.
Nothing ships one; it is the same class of latent shape this item just closed
one instance of. (iii) **The fixture's `themes/mine/pages/demo.md` carries a
`cover:` front-matter field that nothing declares** and loads clean because the
row is not content — I7b's rule and this one being true at once. If I7b's filter
were ever reversed, that file fails the load rather than silently publishing,
which is a nicer failure than the one I7b's own test would give.

**2026-07-27 — IR7.** Landed as one commit. A one-line surface change, and the
only decision in it is which of two identical answers to print.

*The three shapes, run against the real corpus rather than predicted.* The
block now ends with the law's output, directly beneath the two facts it reads:

```
$ grackle --profile drafts explain /drafts/why-is-a-cursor-called-a-caret/
  collection drafts / rule **/*.{md,markdown} / shell html
  front_mattered false / rendered true          ← the degenerate row

$ grackle --config examples/field-notes/grackle.toml explain /demos/pane/
  collection entries / rule **/*.{html,md} / shell raw
  front_mattered true / rendered true           ← the pane

$ grackle explain /humans.txt
  collection entries / rule **/* / shell raw
  front_mattered false / rendered false         ← the byte copy
```

**The pane answer the brief asked for is TRUE, and confirmed on the live site**
rather than reasoned from the law: `demos/pane.html` is front-mattered and wears
`shell: raw`, so it renders — the first clause — and the `raw` shell then emits
the result verbatim. It is the row that fails a "the shell decides" law, and now
it is the row that *says* so on one screen.

***[decided]* The line prints the STORED bit, not a re-derivation.** The two are
the same answer by construction — `load.rs` builds every row by calling
`shell::renders(f.has_front_matter, worn.shell)` and keeps the result — and the
agreement was **measured, not assumed**: a scratch test re-ran the law over every
row of all six trees and compared, **2864 row-loads, zero disagreements**
(grack.com 1396 × 2 profiles, field-notes 42, theme-preview 24, minimal 3,
raw 3). Given equality the choice is about what the line *means*: a
re-derivation prints what the law WOULD say, and the stored bit is what actually
decided whether this file was parsed. On a row where they ever diverged the
second is the diagnosis and the first is a distraction, so `explain` prints the
row and the recomputation stays where it belongs, in the loader. The census also
recorded the two clause-witnesses corpus-wide: **one degenerate row** (the caret,
on both grack.com profiles — the warning fires on the default build too) and
**one front-mattered non-`html` row per site that has one** (grack.com's single
`light_html`, field-notes' pane).

*Four rows, four mutations, each red on exactly the row that disagrees.* IR2's
pair (`true`/`true` and `false`/`false`) agree with both halves of the
disjunction and so witness nothing alone — which is why the test needed two more,
and why both are corpus shapes rather than invented ones: a blockless `.md` under
`_posts/` (the base's posts rule claims it with `defaults = { shell = "html" }`
and no front-matter gate — grack.com's `_drafts/caret/…` under the base config)
and a front-mattered `demos/pane.html` saying `shell: raw`, down to the path.
Mutations: hardcode `true` → the byte copy fails; hardcode `false` → the other
three; print `r.front_mattered` → **the degenerate row alone**, which is the
pre-I7c tree loader's answer caught by the one row it was ever wrong about;
print `is_document(shell)` → **the pane alone**. The loader stays in the loop for
the file's standing reason: `rendered` is stored, so a hand-built `Row` would
prove only that `format!` interpolates a bool.

*Parity.* Five sites plus grack.com `--profile drafts`, HEAD's release binary
built in a `git worktree` against this one over the same content trees —
byte-identical but for the two grack.com feeds' wall-clock `<updated>` (the four
other builds landed in the same second), stderr identical for all six, stdout
identical modulo the out-dir name and timings, file counts 8 / 8 / 83 / 242 /
1828 / 1829, unmoved since IR1. `cargo test` green (25 result lines);
`cargo fmt --check` clean under the pin; clippy 47, HEAD's number; **zero
re-blessing** — `git status` after the commit showed only the three paths the
item touched. The CLI delta is one added line, diffed old-binary against new.

*Docs.* DESIGN.md §4's gate section gains an observability paragraph, I7d's
`collection`/`rule` precedent (§4's "one supplier" neighbourhood) applied one law
over: the law is stated there, so where it can be *seen* belongs there too.
`manual/OUTLINE.md` untouched per MERGE.md §4, and checked rather than assumed —
fifth in the sequence: it introduces `grackle explain` as *the* debugging tool
(ch. 5, ch. 7) but never enumerates the lines it prints and teaches neither
`rendered` nor `front_mattered`, so nothing in it went stale.

*For batch review I-C.* Two things. (i) **The stored-vs-derived call** is the
item's only judgement, it is reversible in one expression, and the reason it is
not merely academic is I8: a sidecar widens what `front_mattered` means, and the
first shape that can make the stored bit and a re-derivation disagree is a row
whose identity arrives from somewhere the printed facts do not name. (ii) **The
block is now five lines and four of them are inputs to the fifth in some
reading** — `collection` and `rule` decide the shell, the shell and identity
decide `rendered`. Nothing else in `explain` is derived, and if a second derived
line ever joins, the block wants a visual separation the format string does not
currently have.

**2026-07-27 — I8.** Landed as one commit. The item is one new spelling of one
old fact, and everything interesting in it is about what the fact does NOT
imply — the two laws that used to read one bit and now read two.

***[decided]* The spelling is the pair, not the name.** q49 floated
`.p01.png.toml`, and the shape it names — `<file>.toml` beside `<file>` — is
what landed, because the model text assumes a sidecar sits beside the thing it
speaks for and nothing argued otherwise. The two alternatives were weighed and
are worse in the same way: a `.meta/` directory splits a file from its
description across two places (`git mv` moves one of them), and a
front-matter-block-in-a-twin (`photo.png.md`) would make the twin a row of its
own that the walk then has to un-claim. What is a decision rather than a copy
of q49 is **how a sidecar is recognised**: by the *pair*, not by the name. `X`
must exist beside `X.toml`, and a `.toml` naming no file is ordinary content
that still ships. That is what keeps `Cargo.toml`, `netlify.toml` and
`.schema.toml` out of the mechanism with **no exception list** — `.schema.toml`
would speak for a file called `.schema`, which no site has — and it is q49's own
rider ("must not infer page-vs-component from absence") applied to the
detection rather than only to the design. The residual is recorded rather than
closed: rename `photo.png` and its sidecar quietly becomes content, published at
its own URL. An error would have to fire on stray `.toml` files, which is
exactly the false positive the pair test exists to avoid; a heuristic over the
stem ("it looks like it names a file") is the guessing q49 forbids.

***[decided]* A sidecar IS a front-matter block, literally.** It deserializes
into the same `store::FrontMatter` struct, from TOML instead of YAML — so every
named field a block may write (`title`, `date`, `permalink`, `tags`, `shell`,
`theme`, `layout`, `toc`, `order`, `description`) works on day one, and `extra`
reaches the same `schema::validate` with the same undeclared-key error naming
the same knowns. The brief's "the identity it grants is exactly block-identity"
is then true **by construction** rather than by a parallel implementation that
drifts. It cost one `Clone` derive. The one authoring wrinkle is TOML's: a bare
`date = 2020-01-01` is a TOML datetime and must be quoted, which the type error
says.

***[decided]* Two sources on one file is a load error.** The brief's lean,
taken. The argument that survived contact is not "peers cannot be ranked" but
that **a sidecar exists for files that CANNOT carry a block**, so a file with
both has said one thing twice in two places that will drift — MERGE.md A5's
unrankable-disagreement shape, and A5's answer. Mutation-checked, and the
mutation is the reason it is an error: with the `bail!` deleted the site loads
and the BLOCK silently wins, which is a precedence rule nobody declared and
nobody could look up.

***The item's real content: `renders` reads the BLOCK, `degenerate` reads
IDENTITY.*** I7c stated one law over two facts and fed it `f.has_front_matter`,
where "has a block" and "has identity" were the same bit. I8 is where they come
apart, and taking the *narrower* one for the rendering law is the whole of §3's
"sidecars split identity from parsing": a block is IN the file, so a file with
one is a document whose remainder is a body; a sidecar is a second file and says
nothing about the first one's bytes. So a sidecar'd `.png` answers
`front_mattered true / rendered false`, `body_bytes 0`, and ships its bytes
through `raw`. The degeneracy warning asks the OTHER question deliberately — it
exists to nudge an *unnamed* row towards a name, and a sidecar is a name — so a
sidecar'd row under `shell = "html"` renders with no warning while the blockless
file beside it still earns one. Both parameters were renamed at their
definitions (`has_block`, `has_identity`), because a law whose correctness
depends on which bool a caller happens to pass is one refactor from being wrong.

***[decided]* `explain` says which, and both are spelled.** IR7 flagged that its
stored-vs-derived call "is not merely academic … the first shape that can make
the stored bit and a re-derivation disagree is a row whose identity arrives from
somewhere the printed facts do not name". That shape is here, and IR7's call is
vindicated: the printed `rendered` is the bit the row was built with, and a
re-derivation from the printed `front_mattered` would now say `true` for a
sidecar'd image. The fix is to make the printed facts name the source —
`front_mattered true (block)` / `true (sidecar)` — after which the law is
re-derivable from the block again. **Both spelled, not only the exception**: a
`true` whose meaning depends on the absence of a word is the shape this ledger
keeps refusing, and the cost is a CLI-only line that no test, fixture, script or
doc outside `io_explain.rs` reproduces (the grep, re-run). A separate `sidecar`
line was the alternative and loses on the thing the block is for: a reader
seeing `front_mattered true / rendered false` on adjacent lines needs the
answer on one of them, not two lines apart. The row carries `sidecar: bool`
rather than the sidecar's path, because the path is `rel` + `.toml` by
construction — there is exactly one name it can have.

***[decided]* Sidecars are read on the DECLARATION walk, and that is
load-bearing.** IR6's world made the choice available and R1 made it correct:
`store::walker_declarations` applies `exclude` to **directories only**, because
a file-shaped pattern is a statement about *content* and must not silently
unspeak a declaration. grack.com's `exclude` lists `*.toml`. Reading sidecars on
the content walk would therefore have let one pre-existing line delete every
sidecar on the site, silently, with the images still shipping — invisible in a
build's file list, which is the failure class this document exists to refuse.
Guarded by a test whose site writes exactly that exclude, and mutation-checked
by gating the offer on `not_content.keeps` (the content walk's file question):
the site loads clean and the image loses its identity.

*Scope interaction, and the filter that was needed.* The sidecar file itself is
not content — the statement `markers.is_marker` makes one declaration family
over, filtered by the set the declaration walk found rather than by a name
pattern. Checked against I7d's laws rather than assumed: an unclaimed `.toml`
under a posts source already falls out by **scope-owns-source**, but under the
TREE scope the `**/*` catch-all claims it, so without the filter
`/assets/kite.png.toml` is published — the row's own metadata served as a file.
That is the test's mutation, and its control is `netlify.toml`, which names
nothing beside it and still ships.

***[decided]* The description page is refused, not built.** §4a says an image
with a sidecar can wear an html output; the brief says do not build it. It needs
an output whose content is not the row's bytes — the outputs half of the model
(I11/I12) — so the shape is refused where the author wrote it, naming the file,
the shell and the fix. **Measured on the mutant rather than reasoned**: with the
check deleted the load dies on `reading …/kite.png: stream did not contain valid
UTF-8`, a sentence that names a file and no reason. It is §10's precedent (one
targeted sentence only where the generic diagnosis misleads) applied to an
UNBUILT capability rather than a retired spelling, and it is keyed on the
extension fact rather than on the sidecar — so a degenerate image (an objects
rule defaulting `shell = "html"`, reachable before this item and inert on the
corpus) gets the same sentence. One line to delete when I11/I12 lands; it is the
call a reviewer is most likely to want narrowed to sidecar'd rows, or dropped.

*One thing built that the brief did not name, and the reason.* **A sidecar's
change stamp folds into its row's `version`** (`f.version ^ sc.version`).
Without it, editing a sidecar changes a row's title and nothing notices — the
incremental machinery compares `version`, and a row whose identity lives in a
second file has to notice that file changing. Guarded by loading twice across an
edit to the sidecar alone; the mutation leaves the two versions equal while the
titles differ.

*The census, and where it lives.* `db.stats.sidecars` beside
`db.stats.markers`, printed by `grackle query stats`, for the marker census's
reason: a declaration family whose whole effect lands on OTHER files leaves no
trace in a build's file list, so a count is the only way to ask whether the
mechanism is in use. **0 on all five sites**, and the stronger statement was
measured rather than inferred: a scan of every `.toml` in the repository,
tracked and untracked, finds **no `X.toml` beside an `X`** — so no corpus file
could have become a sidecar by accident and parity was free by construction
rather than by luck.

*Two messages moved, neither re-blessed.* Identity's own errors now name the
file identity was WRITTEN in — the sidecar for a sidecar, the row for a block —
because that is the file the author has to edit (`schema::validate`,
`cascade_front`, `front_matter_date`); every rung below keeps naming the row,
because markers, rules and the profile are about the row. And q45's claim check
says "has no front-matter **block**" rather than "has no front matter", which is
the one sentence I8 made imprecise; the check itself is unchanged and still keys
on the block, so a sidecar'd file is not claimable as a view's content — a
corner recorded rather than built, since a claimed landing wants a body and a
sidecar is not one.

*Nine tests, eleven mutations plus controls, each red alone and each restored*
(`crates/grackle/tests/io_sidecar.rs`, plus two unit tests in
`crates/source/src/sidecar.rs`). Built sites where the claim is about what the
site publishes (the sidecar is not a row; the image still ships its bytes),
loaded sites where the claim is about a fact no output can show (the version
fold, the warning that must not fire). The mutations: identity drops the sidecar
term; `renders` reads identity instead of the block; the both-sources `bail!`;
the not-content filter; `degenerate` reads the block; the version fold; the
picture refusal; `validate` names the row instead of the sidecar; sidecars read
on the content walk; and the provenance word hardcoded each way (each red on
exactly the rows that disagree — `(block)` fails the image, `(sidecar)` fails
`io_explain`'s two). Controls in the same sites: a sidecar-LESS image, a
block-identity page, a lone `netlify.toml`, and a genuinely degenerate row so
that the asserted silence is silence that means something.

*Parity [required].* Five sites plus grack.com `--profile drafts`, HEAD's
release binary built in a `git worktree` against this one over the same content
trees into separate outputs — **byte-identical but for two wall-clock
`<updated>` lines** (the other four builds landed in the same second),
**stderr identical on all six**, file counts 8 / 8 / 83 / 242 / 1828 / 1829,
unmoved since IR1, and the **`grackle query urls` set-diff EMPTY on all six**
(7 / 7 / 63 / 222 / 1372 / 1373). `cargo test` green (26 result lines);
`cargo fmt --check` clean under the pin; clippy **47**, HEAD's number (two of my
own were fixed to keep it there); **zero re-blessing** — no fixture and no
`expected-error` moved, and the only assertions that changed are
`io_explain.rs`'s two, for the declared `(block)` provenance.

*Docs.* DESIGN.md §4b gains **Sidecars: identity for a file that cannot carry a
block** (the pair rule, the one-struct claim, governance, the both-sources
refusal, the declaration-walk argument, the version fold) — beside markers,
because a marker declares defaults for a directory and a sidecar declares
identity for a file; §4's gate section says the law reads the block and the
warning reads identity, and its observability paragraph gains the provenance;
§5's `front_mattered` paragraph now has **two** shapes where the fact and
`rendered` disagree, one per direction; §5g's 2×2 paragraph records that the
first bit split. **q49 keeps its number and half its openness**: the DECLARE
half is built and points at §4b, the DERIVE half (14 of 57 raw HTML files carry
a `<title>` the database ignores) is untouched and still wants a consumer, and
its three riders are answered in place rather than dropped — precedence (the
front-matter rung; a block beside it is an error), whether a sidecar makes a
passthrough row `rendered` (**no**, and that turned out to be the feature), and
the 838 images' alt text (reachable now, consumed by nothing until I11).
`manual/OUTLINE.md` untouched per §4, and checked rather than assumed — it is
the first file in this sequence that the item made *more* true and *less*: ch. 23
teaches the `.p01.png.toml` spelling, which is exactly what landed, so the
spelling it has taught since before the ledger is now the engine's; its
"★ Neither half is built" line and its q49 pointer are the halves that went
stale, Matt's pen.

*For batch review I-C.* Five things to probe. (i) **The picture refusal** is the
call to weigh: it refuses a shape that is coherent in the model and unbuilt in
the engine, which is a kind of error this ledger has not written before, and it
is keyed on the extension fact so it is wider than sidecars. The measured
alternative is above; the reversal is one `if`. (ii) **The pair rule's
residual** — a renamed companion silently demotes a sidecar to content, and
publishes it. (iii) **Identity now reaches the front-matter GATE**, so a rule
spelled `front_matter = true` claims a sidecar'd image where it did not before;
that is the capability working, but it means a site's existing rules can change
membership the moment a sidecar appears — and on a site whose search filters on
`front_mattered` (the two example sites, since I1) a sidecar'd image joins that
set. Inert corpus-wide (zero sidecars), stated because it is the first way a
sidecar can move bytes that are not its own. (iv) **A sidecar'd file is not
claimable** as a view's `content`, keyed on the block; recorded, not built.
(v) **`front_mattered` is the wider fact and `sidecar` is not a filter column** —
a query cannot yet ask "which rows got their identity from a sidecar", which is
the same shape `rule` has had since I7d (explain reads it, no `where` does).

**2026-07-27 — Batch review I-C (Fable), covering I6, I7a-I7e, IR6, IR7,
I8 — the single-walk phase.** Verdict: **sound; I-D clear.** Independently
re-verified: full parity grack.com both profiles against the pre-phase
baseline (byte-identical modulo wall-clock timestamps; stderr moves by
exactly the one declared caret line); urls set-diffs empty; the I7e census
and I8 zero-sidecar scan exact; THIRTEEN mutations re-executed red,
weighted to the resumed items (I7e, I8 — both coherent at the diff level;
the one artifact found is a stale test comment describing an interrupted
pass's intermediate state). Findings: (1) *should-fix → IR8*: a typo'd
glob on a sourced scope silently empties the blog — a regression at the
ownership law's edge (pre-phase it errored); the law is right, the
warning is missing. (2) the stale comment → IR8 rider. (3) the sidecar
pair rule reaches non-content directories (declaration-walk global reach,
consistent, one DESIGN §4b sentence → IR8 rider). (4) *model drift,
marked in place*: §2's "rendered retires" bullet was stale — rendered is
I7c's law; I9's brief amended so the agent doesn't execute the stale
text. (5) §1's "any rule default beats the implied title" is wider than
the engine (both refusal paths are loud; no disease). (6) cross-item
coherence verified three ways — the I7c×I8 probe (a sidecar'd row with
no title gets the implied name, no warning) is the strongest evidence
for the implied_title generalization. (7) pre-existing, census: the
serve inspector's posts/pages tables both list all rows. (8) docs
verified; ledger conventions followed; q51 settled properly.
**Veto digest: all seven rulings endorsed** — including the proposed
answer to the undecided root-scope asymmetry (keep it; IR8 is its
observability; uniform silence deletes a real refusal, uniform refusal
puts eighteen errors on a deliberate arrangement). IR9 filed (the
thrice-recorded objects/front_matter corner, made live-able by I8's
identity gate). Census additions for Matt: the I7a narrowing loss; the
root-scope ruling to confirm; OUTLINE ch. 23's stale "neither half
built" line (the DECLARE half shipped at I8; q49's DERIVE half still
wants a consumer); §2/§1 model text (Matt's pen); the inspector tables;
sidecar'd alt text writable but unconsumed until I11.
