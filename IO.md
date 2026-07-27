# IO.md — two databases: inputs and outputs

**Status: APPROVED TO LEDGER (2026-07-27)** — Matt greenlit serial
execution after the MERGE.md pipeline drains (F3 → G1 → G2 → batch
review 4). §10 is the work ledger; Matt may still edit the model sections
at any time and the ledger follows the document. Where this contradicts
DESIGN.md, this is the intended successor and DESIGN.md records what
shipped. Remaining **[open]** choices are settled in-item by the
propose-and-flag pattern (the executing agent proposes, records reasoning
in the ledger log, and Matt vetoes at review) unless marked Matt-only.

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

**The head fence**: a theme's head may contain `<style>` and nothing else —
and even that is extracted into the site CSS at build. Everything else
(`<title>`, `<meta>`, semantic `<link>`, `<script>`) is a load error naming
the file and the element. The fence widens only when a real theme hits the
wall (`<meta name="theme-color">` is the known first candidate — the
allowlist principle is "presentational head elements," and it starts at
one).

**One CSS artifact.** All CSS — engine base, theme(s), site overlay,
extracted `root.html` styles, eventually per-post styles — is munged into
one engine-owned output; pages carry exactly one stylesheet link. Remote
fonts ride `@import` inside CSS. Today's per-theme sheets become tomorrow's
*chunking*, a pure perf optimization the model never mentions. **[open]**:
the multi-theme scoping paragraph — with several themes live (the theme
axis), theme rules must scope under the stamped root attribute /
per-theme sub-layers; design detail, no new mechanism.

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
4. **[design detail]** multi-theme CSS scoping in the one artifact.
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

- [ ] **IR1. Three small strictness closures from review I-A.** (a) A
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

### Phase I-B — themes

- [ ] **I4. `root.html`.** The binder accepts a document-shaped theme root
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

- [ ] **I5. Head-style extraction into the existing CSS assembly.** A
  theme root's `<style>` lands in the theme layer of the existing
  per-theme sheets — which are hereby *declared* to be the megacss's
  chunked implementation (no URL changes, no assembly rewrite; the model
  changed, the bytes did not). The multi-theme scoping paragraph gets
  written as part of this item's doc updates. Parity.

*→ Batch review I-B.*

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
  it to the parity method. **Split on arrival** — the
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
