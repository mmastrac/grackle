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

The old filters translate to what they always meant: the sitemap's
`dir || ext == "html"` becomes `shell == "html"` ("the sitemap lists the
HTML documents"); the search route's `kind == "post" || kind == "page"`
becomes the same. The silent-empty-query knife (`kind == "posts"`, plural,
matching nothing forever) becomes unwritable — the column is gone.

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

- [ ] **I2. One shell axis.** Merge the row-tier and view-serialization
  vocabularies into one schema-typed `shell` field with one validator;
  `light` → `light_html` (hard cutoff); the family/arity checks
  (map shells on rows and per-member routes; fold shells on views only;
  identity required for the html family); base-config rules gain explicit
  shell defaults reproducing today's implicit behavior exactly. Parity.

- [ ] **I3. `from = "*"` retires.** A fold shell with no `from` reads all
  outputs (at this stage: the route set — the facts half already exists);
  the star spelling is removed (hard cutoff); a fold's `from` naming a set
  selects those inputs' outputs through the join. **Sequenced after
  MERGE R6, and states its rung-0 side explicitly**: fold selection —
  whether over the output pool or through an inputs-set join — sees
  forced fields per R6's unified law, so a forcing profile legitimately
  changes fold membership (the robots_txt design depends on exactly
  this). Parity.

*→ Batch review I-A.*

### Phase I-B — themes

- [ ] **I4. `root.html`.** The binder accepts a document-shaped theme root
  (head + body); the head fence (style-only, load errors naming the
  element); `shell.html` migrates to body-only `root.html` across the
  base theme and gallery (mechanical); the chrome part kind renames
  `shell` → `root`; the `data-kind` stamp follows. Parity except the
  stamp rename, declared.

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
  implied from its slug at the engine-fallback rung (de-hyphenation
  details propose-and-flag). The live case is the blockless caret draft
  in `_drafts/` — today it renders with NO title, so gaining the implied
  one is a **declared parity exception** (that row's pages change bytes;
  everything else holds). **Split on arrival** — the
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
