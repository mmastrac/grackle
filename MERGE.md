# MERGE.md — one precedence law, one atomicity law

**Status: work in progress.** This file is both the spec for the unified merge
model and the work ledger for landing it. Each work item below is executed by a
fresh agent, one at a time, committing directly to master. Review feedback from
batch reviews is appended to §6 and folded back into the plan.

This came out of a full review of the config system (2026-07-26): the system
documented one precedence law ("nearest wins; first writer per key", DESIGN.md
§5e) but ran on five — first-writer-per-key (rules), nearest-wins-per-key
(markers/schema/slots), whole-entry-shadow-by-name (registries), per-key
child-wins (bags), and wholesale replace (arrays, and by accident `[axes]`).
The five collapse into two laws plus one annotation, and the atomicity law is
*derivable from the config's own type structure* — which is what kills the
hand-maintained dispatch table in `merge_base` and makes the `[axes]` class of
bug unrepresentable.

## 1. The laws

> **Law 1 — Nearness.** Sources are ordered nearest-first; the nearest writer
> of a key wins it.
>
> **Law 2 — Atomicity.** Merging descends namespaces and stops at atoms; an
> atom is taken whole from the nearest writer. Atoms: scalars, arrays, enums,
> and *definitions* (a struct under a user-chosen name). Namespaces: maps, and
> structs under engine-chosen names.
>
> **Annotation (the one exception):** `[[collections]]` key on `source`
> (identity is physical), and their `rules` lists interleave by nearness —
> Law 1 expressed in list order.

Corollaries, stated so they stop being folklore:

- "Child wins per key" (bags) and "first writer wins" (rules) are the same
  law: the child/nearer source is enumerated first.
- Whole-entry registry shadow is Law 2: a definition is an atom.
- CSS `@layer reset, base, theme, overlay, post` is Law 1 mirrored — the
  platform enumerates farthest-first with last-wins.
- A derived fact (e.g. `unanimous_theme`) never outranks a declared one, at
  any rung. That is a ranking of *evidence*, not a merge law.

## 2. The spine

Every ladder in the system is a subsequence of this ordering. Rung numbers are
referenced throughout.

| rung | source | examples |
|---|---|---|
| **1** | the row itself | front matter |
| **2** | directory ancestry, deepest first | markers, `.schema.toml`, `.slots/`, `.style.scss`, buckets |
| **3** | the collection | rules (in file order), `[collections.*.schema]`, relations |
| **4** | the site config | `[site]`, `[schema]`, `[sets]`, `[routes]`, … |
| **5** | site filesystem conventions | `themes/default/`, root `.style.scss` |
| **6** | the engine | `base.toml`, base theme, `parts.toml`, `ENGINE_STRINGS`, `canonical()` |

## 3. The merge tables

### A. Config merge (`extends`: rung 4 over rung 6)

Law 2 applied to the TOML structure. "Depth" is not declared — it is where the
first atom sits.

| table | descends through | atom (shadows whole) | vs. today |
|---|---|---|---|
| `[site]` | the bag | each scalar | same |
| `[schema]` | field name | the field definition | same (documented as registry) |
| `[markers]` | marker filename | the payload table | same |
| `[sets.*]` | set name | the whole definition | same |
| `[routes.*]` | route name | the whole definition | same |
| `[axes.*]` | axis name | the whole definition | **changed** — today wholesale-replace by dispatch fallthrough |
| `[widgets]` | widget name | the template string | same |
| `[shells.*]` | shell name | the definition | same |
| `[profiles.*]` | profile name | the whole definition | same |
| `[records.*.*]` | field name → id | the record | same (depth 2 falls out of map→map→struct) |
| `[i18n]` | the bag, then `names`/`strings` by key | scalars; each name; each `LocalizedStr` (enum = atom) | same |
| `[html.head.*]` | `html` → `head` → element table → entry | the expression string | same (depth 3 falls out) |
| `[links]` | the bag | `policy` | **changed** (invisibly) — today wholesale |
| `[[parts]]` | — | the array | same (arrays are atoms). The *vocabulary* ladder is separate machinery (`parts.rs::Schemas::load`), not this merge: a site re-declaring an engine part at the same type is a no-op (engine kept); a retype is a load error. *(Amended per batch review 1.)* |
| `[[collections]]` | **by `source`** (annotation) | scalars & arrays whole; `relations.*` and `schema.*` as name-keyed atoms | same; `extensions` replaces wholesale *by law* — arrays have no keys |
| `[[collections.rules]]` | — | see table B | same — site rules **prepend** (Law 1 in list form) |
| `extends`, `root`, `gitignore` | — | scalar atoms | same |

### B. Row resolution (a row's field values)

One ladder, first-writer-wins per key. Atom = the field's value.

| rung | source |
|---|---|
| 1 | front matter (wins outright) |
| 2 | nearest marker, walking up |
| 3 | rules, file order (site's before base's) |

Atomicity notes that today are folklore and become law:

- **`route` + `on_demand` are one atom** — they travel together from whichever
  rule supplies the route.
- **Every key here is schema-typed.** `CASCADE_KEYS` (`theme`/`shell`/
  `layout`/`toc`) dissolve into base-`[schema]` declarations and lose their
  untyped side channel.

Field *declarations* run the spine one level up: positional `.schema.toml`
(rung 2) → `[collections.*.schema]` (3) → site `[schema]` (4) → base `[schema]`
(6). Atom = the field definition. **New: two *unordered* declarations of one
name with conflicting types are a collision error** — two positional files
where neither directory contains the other, or any two collections. An
ancestor/descendant pair is *ordered* (rung 2's internal nearness) and stays
nearest-wins per §5b. *(Amended per batch review 1: the line is nearness, not
rung membership. Residual: the ancestor still takes the global `declared()`
name — see §7.)*

### C. Presentation

| ladder | rungs (nearest → farthest) | atom |
|---|---|---|
| theme, per row | axis member field → front matter (1) → marker (2) → rule (3) → `[site] theme` (4) → `themes/default/` (5) → base theme (6) | **the full spec `name:tokens`** (why tokens don't merge across rungs) |
| theme, per view | view declaration (4) → claimed row (1) / member unanimity (*inference*) → site (4) | same |
| fragment, per kind | theme file → base theme file (6) → `canonical()` (6) | the file |
| variant | `{kind}--{variant}` → `{kind}` → `canonical()` | the file (a *request* ladder — fallback on absence, not merge) |
| slot fills | `.slots/<name>` nearest dir (2) → theme/base fragment content (6) | the file |
| CSS layers | `@layer reset, base, theme, overlay, post` — farthest-first, last-wins | the rule (Law 1 mirrored) |
| CSS tokens | same layers | each custom property |
| bare-name refs (§6a, specced) | siblings (2) → nearest bucket (2) → ascend | the resolved file; two hits at one rung = error, never merge |

### D. Strings

| ladder | rungs | atom |
|---|---|---|
| display strings | inline `LocalizedStr` at use site → `[i18n.strings]` (4) → `ENGINE_STRINGS` (6) | the `LocalizedStr` |
| inside a `LocalizedStr` | exact locale → default locale | the string |
| record display | `records.*.name` per locale → default → the id | the string |
| relation labels | declared `label` `@ref` → `@NAME` default | the ref |

### E. Not merge — quarantined on purpose

| thing | what it actually is |
|---|---|
| `content` vs `default_content` | absence semantics — promise vs offer |
| `unanimous_theme`, group-hero | inference ranking: derived never outranks declared |
| `theme.scss` presence declining skins | a gate — presence flips a bit |
| rule `front_matter = true` | a gate on rule eligibility |
| profile application | a projection: shadows named entries (registry law) after the spine resolves, then **re-validates** |
| `fields` flowing through `from` composition | Law 1 in query space, at materialization |

## 4. Process (how this ledger is executed)

- **One item per agent, fresh agent per item, serial.** Code agents are Opus;
  batch reviews are Fable. An agent reads this file, does its one item, and
  stops.
- **Commit directly to master, with pathspec commits only**: always
  `git commit -m "…" -- <files>` naming exactly the files the item touched.
  Never bare `git commit` or `-a` (the working tree may carry the user's
  in-flight edits and stashes). Commit messages end with
  `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- **Never touch `manual/OUTLINE.md`.** It is the user's file.
- **`cargo test` in `grackle/` must pass before committing.** Items marked
  *[parity]* additionally require the fixture suite unchanged (or
  deliberately re-blessed with `UPDATE_EXPECT=1` and the diff explained in the
  commit message).
- **Mutation-check every new guard** (repo law): a new error path or merge rule
  gets a test that fails when the guard is deleted.
- **Never run repo-wide `cargo fmt`.** The installed rustfmt (1.9.0) wants to
  reformat 13 files this work never touched; pathspec discipline does not
  protect against same-file churn, so format only the lines you wrote (or
  nothing). A toolchain pin is a §7 question for Matt. *(Added per batch
  review 2, finding 3.)*
- Check the box here and note deviations in §6 when an item lands. Do not
  reorder or rescope other items.

## 5. The work plan

### Phase A — make the current law enforceable

- [x] **A1. `deny_unknown_fields` sweep.** Add to `Rule` (config.rs:726 —
  the worst: `[[collections.rules]] theme = "x"` is silently ignored),
  `Site` (config.rs:604), `I18nCfg` (config.rs:353), `LinksCfg`
  (config.rs:535), `ShellDef` (config.rs:343). Make `.schema.toml` field
  tables reject keys other than `type` with an error naming the file and the
  knowns (schema.rs:148-152 currently reads only `["type"]` and drops the
  rest). Fix any configs/fixtures that break; add `expected-error` fixtures
  for at least the `Rule` and `.schema.toml` cases. Note: `Site.noindex` is
  `#[serde(skip)]` — confirm profiles can still set it after the change.

- [x] **A2. Exhaustive destructure in `merge_base`/`merge_collection`.**
  In `crates/source/src/config.rs` (~200-226, ~278-301), destructure the
  full `Config` (and `Collection`) so adding a field without deciding its
  merge law is a **compile error**, not a silent fallthrough to
  wholesale-replace. Annotate each key with its law (registry / bag / atom /
  annotation) in a comment. Fix the doc comment at config.rs:189-194 which
  omits `schema` from the registry list the code includes. Behavior-neutral
  except as prepared by A3. *[parity]*

- [x] **A3. `[axes]` becomes a registry; `[links]` merges per-key.** Axes:
  merge by axis name, whole-definition atom (today it falls through to
  wholesale replace — the bug that motivated Law 2's derivation). Links:
  per-key bag (invisible today; `policy` is its only key). Mutation-checked
  tests for both: a base-declared axis must survive a site declaring a
  different axis, and deleting the registry arm must fail the test.

- [x] **R1. The `.schema.toml`/`.section` walk respects the not-content
  layers.** *(From A1's review queue, prerequisite for A4.)* The walk at
  load.rs ~873-903 scans the whole root without consulting the tree
  collection's `exclude` — verified: `cover`, declared only in
  `grackle/examples/field-notes/books/.schema.toml`, type-checks in a
  grack.com `where` clause, because grack.com's `exclude = ["grackle/**"]`
  is ignored by this walk. Embedded sites' declarations become part of the
  host's vocabulary at the same rung, which would make A4's collision check
  fire across unrelated sites. Make the walk consult the same §4c
  not-content layers as the tree walk (this is also q34's disease — cite it
  in the commit). Verify the `cover` leak is gone with a test; byte-parity
  otherwise. *[parity]*

- [x] **A4. Same-rung schema collisions are errors.** `Schemas::declared()`
  (schema.rs:207-220) flattens every subtree schema with `or_insert`, so two
  `.schema.toml` files declaring one name with **different types** resolve by
  alphabetical directory order for the global filter vocabulary — silently,
  and possibly differently from the per-row nearest-wins `resolve()`. Make
  conflicting-type redeclaration at the same rung a load error naming both
  files and both types; same-type redeclaration stays legal. Mutation-check.

- [x] **A5. Marker determinism.** Two different marker files in one directory
  setting the same key resolve by walk order (markers.rs:54-57
  `slot.insert`). Same-directory conflict on a key = load error naming both
  marker files; different keys stay legal. (Nearest-wins across *levels* is
  the law and stays.) Mutation-check.

*→ Batch review 1 after A5.* ✓ done — findings appended to §6; verdict: sound
to build Phase B on. Two follow-up items:

- [x] **R2. Close R1's hole at the excluded subtree's root.** *(Batch review 1,
  finding 1 — land before Phase C.)* `NotContent::keeps` is consulted for
  directories only, and globset's `embedded/**` does not match the directory
  `embedded` itself — so `walker_declarations` descends one level into every
  excluded subtree, and a declaration file sitting *directly* there still
  leaks (proven: `exclude = ["embedded/**"]` + a broken `embedded/.schema.toml`
  fails the host's load; a valid one type-checks `leakfield` in the host's
  `where`). For grack.com this means a future `grackle/.schema.toml` would
  poison its vocabulary. Treat a directory `d` as pruned when `exclude`
  matches `d` or anything under it; extend the `excluded-schema` fixture with
  a first-level `.schema.toml`; fix `NotContent`'s doc comment ("the walks
  must reach the same verdict" is currently false at this boundary).
  Mutation-check; parity otherwise. *[parity]*

- [x] **A6. `.slots/` same-stem fills are unordered peers — error.** *(Batch
  review 1, finding 2.)* `slots.rs::load_dir` (~184): `.slots/nav.md` and
  `.slots/nav.html` in one `.slots/` directory both insert under stem `nav`,
  and unsorted `read_dir` order decides which fills the slot, silently —
  byte-for-byte the A4/A5 disease. Localized stems widen it (`nav.fr.md` vs
  `nav.fr.html`). Same-directory same-stem (per locale) = load error naming
  both files, A4/A5's sorted `conflict()` discipline. Different stems, and
  the same stem at different tree levels (nearest wins), stay legal.
  Mutation-check both directions; corpus check for live collisions first.

### Phase B — derive the merge from structure

- [x] **B1. A structural `Merge` mechanism.** Introduce the machinery that
  expresses Law 2 in types: maps descend per key (value = atom if a struct
  or enum; descend if a map), structs descend per field, scalars/arrays/enums
  replace whole. Hand-written impls or a small derive — whichever reads
  better in this codebase (keepcalm-school minimalism; no new heavy deps).
  Unit tests proving each table's derived depth matches table A above,
  including `[records]` at depth 2 and `[html.head.*]` at depth 3.

- [x] **B2. Port `merge_base` onto it; delete the dispatch.** Collections
  keep their annotation (key on `source`, rules prepend). The A2
  destructure remains as the compile-time completeness check; the per-key
  law now comes from B1's structure, not from a hand-assigned depth.
  Byte-parity across grack.com, field-notes, minimal, raw, theme-preview and
  the fixture suite. *[parity]*

- [x] **B3. `grackle config --effective`.** Print the merged config with,
  per key: provenance (site / base / default) and the law applied (atom
  taken from X vs merged). This is the item DESIGN.md §4d says "should ship
  before 1.0" and the review said is the single highest-leverage tool. TOML
  output with provenance comments is fine for v1. Include at least one test
  asserting provenance for a shadowed registry entry and a merged bag key.

*→ Batch review 2 after B3.* ✓ done — findings in §6; verdict: sound to build
Phase C on. Two follow-up items (land before the final review; sequenced next):

- [x] **R3. Table-capable atoms must trip the depth invariant.** *(Batch
  review 2, finding 1.)* `a_nested_struct_ends_at_one_depth` whitelists
  depth 0, but `LocalizedStr` is an Atom that *deserializes from a table* —
  a future `LocalizedStr`-typed field on `I18nCfg` beside the deeper
  `strings` map would pass every guard while `merge_to_depth(2)` composes a
  LocalizedStr out of two writers, violating table D. Distinguish
  table-capable atoms in `Shape` (a marker on enum atoms that read from
  tables) and tighten the invariant so the case fails the build; or, if
  that fights the design, narrow the test's doc claim honestly.
  Mutation-check with a test-local shape carrying such a field.

- [x] **R4. `engine_defaults()`'s promised guard.** *(Batch review 2,
  finding 2.)* Its doc comment cites `every_defaulted_scalar_is_printed`;
  no such test exists, and deleting `("extends", …)` from the list passes
  the whole suite. Write the test (every top-level `#[serde(default)]`
  scalar on `Config` appears in minimal's `--effective` output) — or
  delete the sentence. The comment as it stands is a false guard claim.

### Phase C — the laws hold at every rung (strictness symmetry)

- [x] **C1. Dissolve `CASCADE_KEYS`.** `theme`/`shell`/`layout`/`toc`
  (load.rs:169) skip `apply_defaults` and get no type checking:
  `defaults = { toc = "true" }` is silently `false`, `theme = 1` silently
  vanishes (load.rs:131-133, :178). Declare the four in base `[schema]` with
  types, route them through the same typed cascade as every other field
  (front matter still nearest), keep the existing vocabulary checks (`shell`
  load.rs:182-188). Follow the flag-family playbook from §4e. Behavior
  identical on well-typed configs. *[parity]*

- [ ] **C2. Row `theme:` validates like the other rungs.** `[site] theme`
  errors at load with knowns (theme.rs:79-97); a view's errors with view
  context (build.rs:386-391); a row's fails at render with **no filename**
  (build.rs:515, :1007, :1331). Validate row theme names against the
  registry before rendering, with the file named. Same for the marker/rule
  rung (may partly fall out of C1).

- [ ] **C3. Promised checks that don't exist.** (a) "Dead rule (matches zero
  rows) → warning" — DESIGN.md §4 line ~249 promises it; no code provides
  it. (b) `trail` is never validated: a typo'd `trail = "montly_archive"`
  silently produces no trail (config.rs:1774 `chain` stops on unknown,
  trails.rs:177 `continue`s), while `tags` — the same shape of reference —
  is checked (config.rs:1416-1425). Validate `trail` names a grouped,
  routed view; mutation-check both.

- [ ] **C4. Silent-name sweep: `[i18n.names]`, `.slots/` stems, `axes`
  identity collision.** (a) `[i18n.names]` keys validate against declared
  locales (config.rs:361-363 — the one localized string outside the net).
  (b) A `.slots/` file whose stem names no known slot warns, naming the
  knowns (slots.rs:167-195 accepts any stem); unknown stems include case
  variants of known slots (`Nav.md` fills nothing — batch review 2,
  finding 7). (c) Exclude engine stream
  slots (`axes`) from the identity-slot set (theme.rs:200 excludes only
  `main`/`site_title`; parts.toml declares `axes` in the shell, so
  `.slots/axes.md` would silently replace the switcher with prose in a
  slot bound as `stream:axis`).

- [ ] **C5. Axis spelling and linking coherence.** (a) The "axis declared but
  never spent" check (views.rs:473) accepts only `{name}` while
  `spends()`/`select_path` (load.rs:218-222, :259-263) accept `{name}` and
  `{axis:name}` — a route written the namespaced way fails falsely. Unify on
  `spends()`. (b) links.rs:309-316 and :421 do bare-form substitution only —
  same unification. (c) Row-link member reconstruction uses `axes[0].template`
  (an arbitrary template) instead of the template `select_path` picked —
  align, or at minimum make the error stop blaming the rule for a selection
  mismatch (links.rs:322-327). (d) A row link with a misspelled axis name
  ships as a literal query string while `view:` errors with knowns
  (links.rs:187-190 vs :404-410) — since only *declared* axis names are ever
  read as selectors, keep unknown keys literal, but warn when the key
  case-matches a declared axis field or axis name.

- [ ] **C6. Profile hardening.** (a) A profile's `where` parses against a
  vocabulary strictly weaker than a view's — the two-shot
  `row_schema()`-then-`route_schema()` with `?` (config.rs:1344-1355)
  cannot mix vocabularies and the comment above it claims otherwise; use
  `row_filter_schema()` (the union views use). (b) `apply_profile` runs
  after `validate()` (config.rs:1096-1099) — re-validate what the profile
  wrote. (c) The `[profiles.*.sets]`/`[profiles.*.routes]` split is
  decorative (:1334 chains both into one lookup) — naming a route under
  `sets` (or vice versa, or the same view in both) becomes an error, since
  the split's entire purpose is legibility. (d) `noindex = true` silently
  clobbers a site's own `robots` expression (:1328-1331) — document the
  override in the profile section of base.toml comments, or preserve a
  site-declared `robots`; decide in-item and record the choice in §6.
  (e) `config --effective --profile nosuch` asserts a projection that
  would never run — check the name against the merged `[profiles]` table
  and say "names no profile (knowns: …)" in the preamble (batch review 2,
  finding 4).

- [ ] **C7. Collection identity errors.** (a) A second `kind = "tree"` or
  `kind = "objects"` collection is silently discarded (load.rs:917-933,
  last-in-BTreeMap-order wins) — the exact bug §4 documents as fixed for
  posts. Load error naming both collections. (b) When an *inherited* view's
  `from` names a collection the site renamed away (`views.rs:347` — the
  error blames `published`, which the user never wrote), say so:
  "`published` is inherited from the base config; its `from = "posts"`
  names no collection on this site — declare `[sets.published]` or keep a
  collection at source `_posts`". `View::inherited` already records what's
  needed.

*→ Batch review 3 after C7.*

### Phase D — vestigial keys and doc rot

- [ ] **D1. Declared-and-ignored config cleanup.** Remove
  `defaults = { layout = "post" }` from grackle.toml (×2) and field-notes
  (×1) — a no-op since "absent means document" (build.rs:739). Remove
  field-notes' dead `template = "atom.xml"` (names a file that doesn't
  exist; redundant beside `shell = "atom"`). Leave `bucket` in place but add
  a load-time **warning** that it is parsed and unimplemented (§6a specced,
  not built) — removal vs. implementation is a §7 question for Matt.
  *[parity]*

- [ ] **D2. Doc-rot batch (code comments and configs only; never
  `manual/OUTLINE.md`).** (a) `Axis` doc example (config.rs:768-777) shows
  `prefix`/`match` keys that `deny_unknown_fields` rejects — fix to
  `values`/`field`. (b) `[links] policy` doc (config.rs:116) says "`loose`
  (default)" over a `#[default] Strict` — fix. (c) themes/DESIGN.md says
  `theme.toml` is "not yet live" at line ~30 and "now real" at line ~64,
  and zero `theme.toml` files exist — reconcile to unbuilt. (d)
  theme-preview/grackle.toml's head-block comment claims "the base's,
  copied" while missing `icon`/`shortcut icon` — restore the two lines or
  fix the comment. (e) theme-preview/index.md calls itself "the vanilla
  member" and, two lines later, "not one of them" — fix. (f) DESIGN.md
  §5a's stated theme cascade omits the marker rung (markers beat rules) —
  add it. (g) theme.rs module doc (lines ~5-8) names a `head` part that
  doesn't exist in parts.toml — fix.

*→ Final review after D2.*

## 6. Review log

*Batch reviews (Fable) append findings here; follow-up items get added to §5
with a `R` prefix (R1, R2, …). Executed items note deviations here.*

**2026-07-26 — A1.** Landed as specced, with one deviation and one finding.

*Deviation:* the `.schema.toml` expected-error fixture declares its bad field
in `[schema]` rather than in a positional `.schema.toml`, because a broken
`.schema.toml` committed **anywhere in this repo** fails grack.com's own load
(see the finding). Same table, same `parse_fields`, so the error path is the
same one; the positional-file entry point (`Schemas::add`, which is what names
the file) is covered by a mutation-checked unit test in `schema.rs` instead.

*Finding, for the queue:* the `.schema.toml` / `.section` walk (`load.rs`
~873-903) is **root-wide and does not consult the tree collection's
`exclude`**. grack.com excludes `grackle/**`, yet `cover` — declared only in
`grackle/examples/field-notes/books/.schema.toml` — type-checks in a grack.com
`where`. So field-notes' and theme-preview's declarations are silently part of
grack.com's site vocabulary, at the same rung as its own. A4 will meet this:
its same-rung collision check would start firing across sites that have nothing
to do with each other. Not touched here.

Also confirmed: `[site] noindex` is now a parse error (it is `#[serde(skip)]`),
which matches what its doc comment already promised; `apply_profile` sets it in
Rust and is unaffected. Both halves are asserted.

**2026-07-26 — A2.** Landed. The merge operates on `toml::Value`, so a literal
destructure inside `merge_base` was not available: the shape chosen is a law
table per struct (`CONFIG_LAWS`, `COLLECTION_LAWS`) that both merges dispatch
through, guarded by two functions whose entire body is a destructure of
`Config` / `Collection`. A new field fails the build there; the test
`the_law_tables_cover_the_config_surface` checks the tables' TOML spellings
against serde's own `deny_unknown_fields` list, so a rename or a
`#[serde(skip)]` cannot leave a law naming nothing. Both directions are
mutation-checked. The prose registry list above `merge_base` (the one missing
`schema`) was deleted rather than corrected — the table is that list now.

*For B1/B2:* the four laws as named there are `Atom`, `Descend(n)`,
`Collections`, `Prepend`. B1's structural mechanism should be able to *derive*
the `Descend(n)` depths — 1, 2 and 3 today — and check them against the table,
which would make the depth column of table A executable rather than asserted.

*Finding, for the queue:* `[[parts]]` is `Law::Atom` — the site's array
replaces the base's whole — but table A describes a vocabulary ladder where
"engine part wins collisions". Nothing in this item changed it, and the base
config declares no `[[parts]]`, so the two never meet today. Worth confirming
the intended law before B2 makes it structural.

**2026-07-26 — A3.** Landed. `axes` and `links` are both `Law::Descend(1)`
now; table A's two **changed** rows are true of the code. Zero fixture churn,
as predicted — `base.toml` declares neither table, so no site's merge reaches
either arm.

*Deviation (small):* `merge_base` and `merge_collection` had the same loop
body twice, differing only in the law table. Extracted as `merge_table(base,
site, laws)`; both now call it. Behaviour-identical, and it is what gives the
tests an honest entry point — with no `[axes]` in `base.toml`, a
`Config::from_toml` test cannot reach the arm at all (a key the base never
wrote is the site's whole under every law), so the tests drive the real
dispatch with a base of their own rather than restating the loop.

*Note on the second axes test:* "redeclaring an axis replaces it entire" is
asserted at the TOML level, on a site axis that declares `values` and not
`field`. That config would not deserialize into an `Axis` — which is the
point: the assertion is that the base's `field` does **not** arrive to
complete it. A typed test could not express the difference between
`Descend(1)` and `Descend(2)` here, because `Axis`'s two fields are both
required.

*For B1:* `[links]`'s bag law is untestable from a real config today — one
key, so nothing can be left behind. The test states the law with a
hypothetical second key and says so. When B1 derives depth from structure
this becomes a statement about `LinksCfg` being a struct under an
engine-chosen name, and the hypothetical goes away.

**2026-07-26 — R1.** Landed. The vocabulary walk and the marker scan now
share one `store::walker_declarations`, and the §4c declared layer is one
compiled value (`store::NotContent`) built once by `load` from the tree
collection and read by all three walks — `walk_tree` included, which used to
compile its own globsets inside `build_tree_and_objects`. Byte-identical
output for grack.com and all four example sites (only the feed's wall-clock
`<updated>` moves), zero fixture re-blessing.

*The markers walk had the same blindness, and it is inert today.* A marker
under an excluded directory can only govern rows in or below that directory,
and those rows are excluded too — so nothing observable changes and there is
no behavioural test to write for that half. The parity claim is the test.
What the shared filter does buy is the reachability invariant: the
declaration walks now reach a **superset** of the tree walk's directories
(the dot/underscore skip is theirs alone), so no marker governing a loaded
row can be missed.

*Deliberate narrowing, for the reviewer:* `exclude` is applied to
**directories only** in the declaration walks. Pruning the embedded subtree
is the whole of the disease, whereas a file-shaped pattern is a statement
about *content* — grack.com's `exclude` lists `*.toml`, which would
otherwise silently unspeak a root-level `.schema.toml` the day one is
written. That is the same class of silent loss in the other direction. Both
readings are byte-identical on today's corpus.

*Deviation:* one fixture comment edited, not re-blessed —
`schema-field-unknown-key/site/grackle.toml` justified A1's deviation by
describing this leak as live. It now points at the new `excluded-schema`
fixture, whose deliberately-broken `.schema.toml` is **committed and inert**
under an excluded directory. That file is itself the demonstration: check out
the parent commit with it in place and grack.com's own build fails on it.

*For the queue (q34's remainder):* DESIGN.md's "three definitions of *not
content*" bullet names `slots.rs` and `serve.rs` as still carrying private
skip lists. This item did not touch them; the `NotContent` value now exists
for them to adopt.

**2026-07-26 — A4.** Landed, with one scoping call the reviewer should weigh.

*The line is nearness, not rung membership.* Rung 2 is "directory ancestry,
deepest first" — it carries an internal order — so an ancestor and a
descendant disagreeing about a type is §5b working as designed, and
`schema.rs`'s own test has said so since it was written (`books/` declares
`author` string, `books/special/` int, a row picks the nearer). Making that an
error would contradict a documented, tested law. Two directories with
**neither inside the other** have no order at all: nothing ranks them, and
that is the case `declared()` was settling alphabetically. So the guard fires
exactly when the two dirs are incomparable. `[collections.*.schema]` needs no
such test — collections are siblings by construction — so any disagreement
there is the error.

*How it composes with q51's base-field guard:* they do not overlap, and the
older one runs first. q51's is in `parse_fields` and refuses a **built-in row
name** at every rung, for an unrelated reason — `Row::field` answers built-ins
first, so the declaration parses, validates, and is then unreadable. A4's is
in `add` / `add_collection`, runs after parsing, and is about two *declared*
names colliding where nothing ranks them. A file redeclaring `month` still
gets q51's message, not A4's.

*Residual, for the queue (small, deliberate).* The ancestor/descendant case
still reaches `declared()` with two types, and the flattening gives the global
name to the **ancestor** (`BTreeMap<PathBuf>` orders a parent before its
child, so this is at least deterministic — the broader claim takes the site
vocabulary). A `where` written against it therefore type-checks against the
ancestor's type while rows under the descendant carry the other. Documented on
`declared()` rather than changed: there is no obviously right answer for a
*global* vocabulary built from *positional* claims, and picking one is a
behaviour change B3 (`--effective`) would make legible first.

*Reachability of the collection case:* real, untested by the corpus — **no
site anywhere in the repo uses `[collections.*.schema]` at all**, so rung 3
has zero live writers today. Guarded and unit-tested regardless, since it is
the rung with no nearness to fall back on.

*Corpus:* nothing tripped. The four `.schema.toml` files in the repo live in
three different sites and share only `cover = { type = "image" }` between
field-notes and theme-preview — different roots, and agreeing anyway. grack.com
and all four examples build. Zero fixture re-blessing; one new fixture
(`schema-same-rung-conflict`), mutation-checked both ways (deleting the check
in `add` makes it build silently; deleting the loop in `add_collection` fails
the unit test).

**2026-07-26 — A5.** Landed. The per-file fold moves out of `scan` into
`Markers::fold`, which carries scan-lifetime provenance (directory → key →
marker filename) and bails on a same-directory disagreement, naming both files
sorted and the key. A4's `conflict()` shape is reused verbatim in spirit — a
two-element sort — because the marker walk is as unsorted as the declaration
walk it shares (`walker_declarations`, R1).

*Same key, same value in one directory is legal — the decision.* A4's line was
"the error is the unrankable disagreement, not the second writer", and that
reading transplants exactly. Nondeterminism is the disease; two markers writing
the same value leave the directory with the same defaults whichever the walk
saw last, so there is nothing to rank and nothing to observe. Erroring anyway
would be a rule about *tidiness* (don't say a thing twice), not about merge
order, and it would fire on a real and harmless shape: a config where `.archive
= { noindex = true, hidden = true }` and `.noindex = { noindex = true }` both
exist, and someone drops both files in one directory. The value equality check
that buys this is one `!=` on `toml::Value` — the cost the item flagged is
nil, and paying it keeps the two same-rung guards saying the same sentence.
It is mutation-checked in the other direction too: forcing the error regardless
of value fails `two_markers_agreeing_on_a_key_is_legal`.

*Scope, stated for the reviewer:* the guard is per directory only. Two markers
at **different** levels claiming one key never reach it — rung 2 carries an
internal order, `defaults_for`'s walk is that order, and
`the_same_key_at_two_levels_is_still_nearest_wins` pins it. Identical to A4's
ancestor/descendant carve-out, minus the residual: markers have no `declared()`
equivalent flattening the tree into a global vocabulary, so there is no second
consumer to disagree with `defaults_for`.

*Corpus:* nothing tripped, and nothing could have — **there is not a single
marker file anywhere in the repo**, tracked or untracked. All five sites
declare `[markers]` (`.draft`/`.hidden`/`.noindex`, disjoint keys in every
config), and zero directories carry one. `markers.found` is 0 for grack.com and
all four examples; the whole feature is configured and unexercised by content
today. Worth knowing before D1's vestigial-key sweep, and worth knowing when
reading `db.stats.markers_ms`. All five sites build; zero fixture re-blessing;
one new fixture (`marker-same-dir-conflict`), which is also the only place in
the repo where a marker file exists at all.

**2026-07-26 — Batch review 1 (Fable), covering A1, A2, A3, R1, A4, A5.**
Verdict: **sound to build Phase B on.** All mutation claims spot-checked held
(A5's `bail!`, A4's collision check, A3's law flips both directions, A2's law
table); no undeclared behavior change found; A4/A5's shared "unordered peers"
discipline rhymes as intended. Findings, condensed — full text in the session
transcript:

1. *should-fix → R2 (filed above):* R1's directory-only pruning misses a
   declaration file at the excluded subtree's **first** level — `embedded/**`
   never matches the directory `embedded` itself. Proven empirically; the
   `excluded-schema` fixture nests too deep to catch it; falsifies
   `NotContent`'s doc comment at that boundary. The judgment call itself
   (file-shaped patterns like `*.toml` must not unspeak declarations) is
   endorsed.
2. *should-fix → A6 (filed above):* `.slots/` same-stem fills in one
   directory resolve by unsorted `read_dir` order — the batch's own disease,
   one directory over. C4(b) covers unknown stems only.
3. *spec amendment (applied):* table B now says **unordered**, not
   "same-rung" — A4's nearness line is the better statement of the law, and
   §5b's tested ancestor/descendant behavior confirms it.
4. *kept visible (§7):* A4's residual — the ancestor takes the global
   `declared()` name while descendant rows carry the other type. Deterministic
   but observable skew; deferred to B3's legibility, parked below so it isn't
   lost.
5. *spec amendment (applied):* `[[parts]]` — A2's `Law::Atom` is correct; the
   "engine part wins collisions" prose was imprecise (same-type re-declaration
   is a no-op, a retype is a load error, and that ladder is `parts.rs`
   machinery, not this merge). A2's flag is resolved.
6. *endorsed:* A5's same-key-same-value-is-legal decision — the error is the
   unrankable disagreement, not the second writer.
7. *cosmetic, no action:* with three conflicting writers, which *pair* an
   error names still depends on walk order (each pair is internally sorted).
8. *test-honesty notes:* A1's `rule-unknown-key` expected-error substring is
   loose (matches any unknown-`theme` error); A2's `serde_keys` parses serde's
   error format (version-fragile but fails loudly). Neither can pass silently.
9. *ledger correction:* A4's §6 corpus note — the non-fixture corpus has
   **three** `.schema.toml` files (field-notes/books, field-notes/recipes,
   theme-preview/shelf), and books/shelf share `author` too (agreeing, so
   harmless).
10. *§7 addition (filed below):* markers are configured in all five sites and
    used by zero directories.

**2026-07-26 — R2.** Landed. `NotContent::keeps_dir` is the directory
question; `walker_declarations` asks it, `walk_tree` still asks `keeps`.

*The idiom: the empty child.* globset has no "matches anything under this
directory" query, so the second question is `rel.join("")` — the same path with
a trailing separator, `embedded/`. Verified against globset directly:
`embedded/**` does not match `embedded` but does match `embedded/`, while
`*.toml` matches `a/b.toml` and matches neither `a/` nor `a`. That asymmetry is
exactly the line R1 drew by hand, so the trailing separator *is* R1's narrowing
rather than an exception carved beside it — no sentinel filename, no pattern
string surgery, and a file-shaped pattern can never prune a directory.
`include` is asked both questions before `exclude`, so its precedence holds at
the same granularity.

*The doc comment is now true, and says which question buys it.* "The walks must
reach the same verdict" was false at this boundary because sharing one value is
not the same as asking it one question. The tree walk can prune loosely — it
post-checks every file it emits, and a file pattern *should* exclude a file
there — while a walk that decides purely by pruning gets no second chance.
`NotContent`'s doc now states that split; `walker_declarations`' "directories
only" paragraph names `keeps_dir`.

*Mutation-checked both directions.* Restoring `keeps` in the walk fails the
extended fixture on `embedded/.schema.toml: TOML parse error` instead of the
expected `unknown field cover`; probing with a sentinel that a file pattern can
match (`rel.join("probe.toml")` instead of `""`) fails
`a_file_shaped_pattern_still_does_not_prune_a_directory`, which is R1's call
guarded as a test rather than a comment.

*Residual, for the queue (hypothetical today).* R1's reachability invariant —
declaration walks reach a superset of the tree walk's directories — now has one
hole in the other direction: `exclude = ["vendor/**"]` with a **file-shaped**
`include` beneath it (`include = ["vendor/keep/x.md"]`) lets the tree walk load
that row while the declaration walk has pruned `vendor` and cannot see a
`.schema.toml` beside it. Closing it needs the include *patterns*' literal
prefixes, not their globset, which is more machinery than this item is worth.
No site in the repo has an `include` that points inside an excluded directory
(grack.com's is `.well-known/**`, `.htaccess`); a subtree-shaped include
(`vendor/**`) is handled, since it matches the empty child.

*Parity:* grack.com and all four examples byte-identical except the feeds'
wall-clock `<updated>`; zero re-blessing; the only fixture change is the new
first-level file and the comment pointing at it.

**2026-07-26 — A6.** Landed. `load_dir` asks the map before it inserts;
`conflict()` is the third sorted two-element message in the family, differing
from A4's and A5's only in what each writer *says* — here the pipeline its
extension picks, not a type or a value.

*The cross-extension decision: A5's exemption does not transplant, and it
cannot.* A5 let two markers agree because the walk's arbitrariness was then
unobservable. Here there is nothing to observe *with*: two files sharing a stem
in one directory can differ only by extension (the filesystem forbids the
same name twice), and extension is the pipeline — `.md` through comrak,
`.html` verbatim with links resolved (§5e, and `Fill::render`'s two arms).
Byte-identical files are still two different fills, so the error fires
regardless of content and there is no equality check to write. The
same-stem/same-extension case the item asked about is not merely legal, it is
**unreachable**; the guard's comment says so, so a later reader does not go
looking for the missing `!=`.

*One behaviour-neutral reorder, worth naming.* The unknown-extension `bail!`
now runs **before** the file is read. Beyond not reading a file it will reject,
this makes the two errors independent: a `.slots/nav.txt` beside `.slots/nav.md`
is always the extension error, never a collision that depends on which of the
two `read_dir` reached first.

*Test shape.* The message is asserted by unit tests in `slots.rs` (temp-dir
trees, the `build.rs` css-pass precedent) rather than by the fixture, because
`conflict()` prints absolute paths and `expected-error` matches one contiguous
substring — no substring can span both filenames. So the fixture asserts the
site-level fact ("this site does not build") and the unit tests assert the
sentence, including that sorting removes walk order from it (`conflict(md,
html) == conflict(html, md)`, asserted directly — `read_dir`'s order is not
ours to permute). Both controls are unit tests too: `nav.md` beside
`nav.fr.md`, and the same stem two levels apart resolving nearest-first.

*Corpus:* three `.slots/` directories in the repo — the site root
(`copyright.md`, `nav.md`), field-notes (those two plus `.fr` twins) and
theme-preview (`copyright.md`). No stem repeats in any of them, and
field-notes is the live proof that the locale convention is the common shape.

*Why this fixture needs none of R1/R2's pruning:* the `.slots/` walk carries
its own hard-coded `SKIP` list, and `grackle` is on it — so grack.com's build
never descends into the workspace at all, and the fixture's deliberate
collision cannot leak into it the way `excluded-schema` could. That skip list
is one of the two private "not content" definitions DESIGN.md still complains
about (R1's §6 note names `slots.rs` and `serve.rs`); adopting `NotContent`
here would *cost* this isolation unless the site's `exclude` is consulted, so
whoever picks that up should read this line first.

*Parity:* grack.com and all four examples byte-identical but for the feeds'
wall-clock `<updated>`; zero re-blessing; one new fixture
(`slot-same-stem-conflict`). Mutation-checked in both directions — with the
`bail!` deleted the fixture builds and its footer carries
`© 1998 the markdown pipeline`, chosen by nothing but `read_dir`.

**2026-07-26 — B1.** Landed. `shape.rs` holds Law 2 (`Shape` — `Atom`,
`Struct`, `Map` — and `Shape::depth`); `config.rs` holds the shape of this
config below the law tables; `the_derived_laws_agree_with_the_hand_tables`
holds the two halves to each other, key by key, across both tables. Every
depth in table A now derives, `[markers]` excepted (below). No behaviour
change — `merge_table` still dispatches through the hand tables.

*The mechanism: a small trait, not a derive.* `Shaped::shape()` per struct,
with each field's shape read off the FIELD'S OWN TYPE through a selector that
is never called — `field("site", |c: &Config| &c.site)`. That is what makes
this a derivation rather than the law table spelled twice: retype a field and
its law changes with nothing to edit. The containers are mechanical (a map
descends per key, a `Vec` is an atom whatever it holds, `Option` is
transparent, `toml::Table` is a map of untyped values), which leaves one
hand-written FIELD LIST per engine-named struct — seven of them — and A2's
`serde_keys` trick checks each against serde's own `deny_unknown_fields` list,
so the description cannot drift out of TOML's name space. A proc-macro derive
would have bought only those seven lists, at the price of a new dependency and
struct definitions read through a macro.

*Namespace or atom is POSITION, not type* — which is the part that pays.
`Shape::depth` applies §1's rule (a struct under an engine-chosen name
descends per field; under a user-chosen one it is a definition, and stops the
descent), so one description of a type merges correctly wherever it is used,
and the depths fall out: `[sets]`/`[axes]`/`[shells]`/`[profiles]` 1 (maps of
structs), `[records]` 2 (map of maps of structs), `[i18n]` 2 (two of
`I18nCfg`'s five fields are maps, and the scalars beside them are unharmed),
`[html.head.*]` 3 (`HtmlCfg` → `HeadCfg` → `BTreeMap<String, String>`).
A3's `[links]` hypothetical is retired as evidence: the law is now a statement
about `LinksCfg` being a struct under an engine-chosen name, true of every key
it will ever have. The test keeps the `reach` stand-in only as a demonstration
of what `merge_to_depth` does, and says so.

*The one disagreement, for the queue (§7 item 10).* `[markers]` is
`BTreeMap<String, BTreeMap<String, toml::Value>>` — a map of maps — so the
structure reads **`Descend(2)`**: the marker filename, then each default it
sets. Table A and `CONFIG_LAWS` say **`Descend(1)`**: the payload table is one
atom. Nothing here decides it; `KNOWN_EXCEPTIONS` carries the derived value
and `the_marker_payload_is_the_one_disagreement` states it alone so it cannot
pass as a typo in a list. **B2 must not port `markers` until this is
answered.** Reachable but inert today: `base.toml` declares the three markers
and every site that redeclares them (grack.com, raw — raw is
`extends = "none"`) restates the payloads verbatim, so both readings are
byte-identical on the corpus. The question is which one a marker payload is —
a definition under a user-chosen name (A5's `.archive = { noindex = true,
hidden = true }` is one thought) or a bag of row defaults, which is how
defaults merge everywhere else in the system.

*Two invariants asserted rather than assumed.* (a) A definition's fields are
deliberately undescribed — `Shape::definition()` is an empty field list, which
is a claim that nothing reads them, not that there are none. That holds only
while no definition sits under an engine-chosen name, so
`a_definition_never_sits_under_an_engine_name` walks both shape trees and says
so. (b) One `Descend(n)` governs a whole table, so a struct takes the DEEPEST
of its fields; `a_nested_struct_ends_at_one_depth` pins that every nested
struct's fields bottom out together. Over-descending a scalar or an array is
harmless (the merge hands back anything that is not a table), but a field that
is both an ATOM and a TOML TABLE — a `LocalizedStr`, a definition — sitting
shallower than a map beside it would be split. None exists; that test is the
tripwire for the next field anyone adds.

*Mutation-checked five ways*, each against the test that owns it: `widgets`
flipped to `Atom` and `html` to `Descend(2)` (agreement test), the markers
exception emptied (agreement + disagreement tests), `HeadCfg` described as a
definition (definition invariant + surface test), and one `HeadCfg` field
given a level its siblings lack (depth invariant).

*Deviation (small):* `serde_keys` now splits serde's message on `expected `
rather than `expected one of `. Same list, second message shape — a one-field
struct (`HtmlCfg`, `LinksCfg`) is told "expected \`head\`". Batch review 1's
note stands: the helper is version-fragile and fails loudly.

*Parity:* zero fixture changes, zero re-blessing, no non-test code path
touched — the only edits outside the new module are the `Law` derives
(`Debug`/`PartialEq`), the additive shape block, and the tests.

**2026-07-26 — B2.** Landed in two commits: the marker newtype, then the port.
`merge_table(base, site, &Shape)` reads each key's law off the description
(`law_of` → `Shape::law`), and `CONFIG_LAWS`, `COLLECTION_LAWS`,
`derived_laws`, both annotation lists and the agreement test are gone. Table A
is now a *description* of the code rather than a second copy of it: there is
one list of keys in `crates/source/src/config.rs`, and the compiler already
checks it against `Config`.

*The annotation is on the field, not beside it.* `Shape::Annotated(law, inner)`
is a fourth variant, and the two hand laws read as
`annotated("collections", |c: &Config| &c.declared_collections,
Law::Collections)` and `annotated("rules", |c: &Collection| &c.rules,
Law::Prepend)` — in the field list, in declaration order, where a reader
meets them. It keeps the field's own shape underneath rather than erasing it,
so the annotation overrides the LAW and not the description and B1's invariant
walks pass through it like any other field. `Law` moved to `shape.rs` beside
the law it spells out; nothing else moved.

*What replaces `KNOWN_EXCEPTIONS`.* The exception list only meant something
against a hand table, so it retired with one — but the thing it guarded did
not. With the law read off the shape, the only way to write one by hand is
`annotated(…)`, so `only_the_annotated_keys_have_a_hand_written_law` counts
those instead and pins them to exactly `collections`/`rules`. A third one now
fails a test that says to file a §6 entry. That test is also what fires if
anyone "fixes" a key by forcing its law, which is mutation-check (c) below.

*q10 landed as its own commit, first, as B1 required.* `MarkerDef` is a
`#[serde(transparent)]` newtype over the payload whose `Shape` is a
definition — no TOML change, no site change — and it emptied the exception
list before the port could read a law off a type the ledger had not settled.
Its behaviour is pinned on the live path (`base.toml` really declares the
three markers, so `Config::from_toml` reaches the arm):
`a_redeclared_marker_replaces_the_payload_whole`. §7 q10 stays open for veto
at the wrap-up; vetoing it now means changing table A's `[markers]` row, not
this code.

*A2's guards are what the item said they were.* Both never-called destructure
functions survive verbatim (`every_config_key_has_a_law`,
`every_collection_key_has_a_law`) — a new field still stops the build — and
the serde-surface test now checks the SHAPE against serde's
`deny_unknown_fields` list per struct, which is the same sentence one table
over: a renamed or skipped field leaves a key no shape claims, and `law_of`
would hand it back whole. B1's `a_definition_never_sits_under_an_engine_name`,
`a_nested_struct_ends_at_one_depth` and `table_as_depths_fall_out_of_the_types`
survive too and now guard the live merge; the depth pin reads through `law_of`
(the merge's own lookup, not a test-side restatement) and gained the
`[markers]` row.

*Signature change, stated:* `merge_table`'s third parameter is a `&Shape`
instead of a law slice. The only call sites are `merge_base`,
`merge_collection` and A3's `merged` test helper, which still drives the
shipping entry point — one line each. A3's and B1's tests are otherwise
untouched.

*Parity:* grack.com, field-notes, minimal, raw and theme-preview built before
and after into separate trees and diffed — every file byte-identical except
each feed's two `<updated>` lines (5 files across the five sites, and nothing
else in any diff). Zero fixture changes, zero re-blessing. This was expected
rather than hoped: B1's agreement test had already proven every derived law
equal to its hand-assigned twin, and the port deleted the twin.

*Mutation-checked three ways, each restored:* (a) `HeadCfg` described as a
definition fails `a_definition_never_sits_under_an_engine_name`,
`the_shape_covers_the_config_surface` and the depth pin (`html` reads
`Descend(2)`); (b) unwrapping `MarkerDef` back to a bare
`BTreeMap<String, toml::Value>` fails the depth pin and
`a_redeclared_marker_replaces_the_payload_whole`, whose message shows the
base's `noindex` composing itself into the site's marker — the `Descend(2)`
disagreement, resurfaced as behaviour rather than as a table entry; (c)
forcing `axes` to atom behaviour (`annotated(… Law::Atom)`) fails A3's
`a_base_declared_axis_survives_a_site_declaring_a_different_one`, the depth
pin, and the new annotation count.

*For the queue (small).* (i) `Config::shape()`/`Collection::shape()` allocate
on every call, and `merge_collection` calls the latter once per paired
collection — a few `Vec`s per merge, once per load, against a saving of a
whole parallel table; noted rather than optimised. (ii) The unknown-key
fallback in `law_of` is still `Law::Atom` (unchanged from the table era) and
is now the one place a key's law does not come from the shape — it is a key
on its way to `deny_unknown_fields`, and B3's `--effective` is where that
would become visible. (iii) `markers.rs`'s test helper `cfg()` was already
dead before this item and now also describes a type `scan` no longer takes;
left alone as out of scope.

**2026-07-26 — B3.** Landed. `merge_table`, `merge_to_depth`,
`merge_collection_list` and `prepend` carry a `path` and a `Trace` and record
each decision as they make it; `crates/source/src/effective.rs` holds the
`Prov`/`Trace` types and the printer, and `Config::effective(path, profile)`
is the entry point. `grackle config --effective` and the `grackle explain`
alias are both in `main.rs`.

*The recorder is a parameter, not a second pass.* The load path merges with
`Trace::off()` — one bool test per key, and the two `for` loops that exist
only to record are inside `if t.on()`. `the_load_path_records_nothing` asserts
it on the real `merge_base` rather than on a stand-in. The alternative the
item allowed (merge twice, once with a recorder) was not taken: two merges is
two chances to differ, and threading the recorder cost four signatures.
`merge_table`'s traced variant was folded back into `merge_table` itself so
there is still exactly ONE merge function; A3's `merged` helper passes
`Trace::off()` and still drives the shipping code.

*What the merge does NOT decide is where the base is most invisible.* The
merge's own loops only visit keys the site wrote — a table the site never
mentioned is passed through untouched and would have no note at all, which is
precisely the case a reader needs. So `note_key`/`note_depth`/`note_table`
walk an unmerged subtree to ATOM granularity and stamp it, descending by the
same law (`law_of`) and the same depth the merge would have used. That is what
lets `[routes.home]` say `# base, whole` on its header and nothing on its six
keys, while `[site]` says something per key: **where the comment sits is Law 2,
made visible.** It is the whole design of the output.

*Provenance has four values, not three.* `site` / `site over base` / `base`,
and `default` for a key neither file wrote (`extends`, `root`, `gitignore`).
The values come from calling the same `default_extends()` / `default_root()` /
`default_true()` functions that `#[serde(default = "…")]` names, so this is a
USE of the defaults rather than a copy of them; a NEW defaulted scalar would
have to be added to `engine_defaults()`, which is the one hand-maintained list
this item adds and the reason it is a three-line function with a comment
saying so. Deserializing to read them was rejected — `Config` is
`Deserialize`-only, and `--effective` deliberately answers on a config the
engine has REJECTED (`bogus_key = 1` prints as `# site` beside the value the
build refuses). That is the item's most useful property and it is worth the
small list.

*Key order.* Top level follows `base.toml` (`ORDER` in `effective.rs`); every
nested table keeps the order the merged `toml::Value` already has, which is
alphabetical — `toml` 0.8 without `preserve_order` sorts, so base.toml's
authored order inside a table is not recoverable and inventing one would be a
third opinion. One exception: within a table, sub-tables print before arrays
of them, so `[collections.schema]` never lands after `[[collections.rules]]`.
That ordering is legal TOML either way (a header path is absolute) but a
printer that leans on it is one edit from being wrong.

*Presentation, stated because it is the only non-derived thing here.* A
table-valued atom prints inline (`draft = { type = "bool" }`) when it fits in
62 columns and as a `[block]` when it does not, which happens to reproduce how
`base.toml` writes each of them. Comments align at column 46.

*No golden file, and the reasoning.* The fixture harness is site → rendered
tree; a CLI text has nowhere to live in it. `base_config.rs` was the right
home, but a byte golden of minimal's effective config would be a SECOND copy
of the base config, machine-generated, needing `UPDATE_EXPECT` on every
`base.toml` edit — while `examples/raw`, the first copy, needs a human edit
and an argument. fixtures.rs's own warning applies exactly ("a blessing tool
that is trusted blindly is a test suite that asserts the code does what the
code does"). What landed instead asserts what a golden would have been
consulted for and cannot churn: **minimal's effective config is entirely
`# base`/`# default`, raw's is entirely `# site`/`# default`**, and all five
sites' output parses back as TOML. The pair is stronger than a golden at the
thing that matters — it fails if the merge starts crediting the site for the
base's values, and it does not notice when the base merely changes.

*Round-trip as the value check.* `printing_the_merged_config_loses_nothing`
parses the printed text back and asserts it EQUALS the merged `toml::Value`
plus the defaults. A comment cannot be wrong about a value it does not carry;
what could go wrong is a definition flattened, an inline table mis-quoted, a
key printed under the wrong header — and the round-trip catches all of those.
It caught one immediately: header path segments were joined unquoted, so a
long marker payload printed `[markers..draft]`. Found by mutation (deleting
the base-recording loop turned every inherited marker into a block), fixed,
and now `a_quoted_key_stays_quoted_in_a_table_header` reaches it without a
mutation.

*Mutation-checked five ways, each restored:* dropping `merge_table`'s
base-recording loop (`an_untouched_table_is_all_base`); `merge_to_depth`'s
depth-0 verdict recording `Site` instead of `SiteOverBase`
(`a_shadowed_registry_entry_reads_as_one_atom_from_the_site`); swapping which
end of a prepended rules list is the site's
(`prepended_rules_carry_provenance_per_rule`); deleting `gitignore` from
`engine_defaults` (`an_untouched_table_is_all_base`); and never marking a
paired collection paired, which lets the Base sweep overwrite the merge's own
per-key notes (`prepended_rules_carry_provenance_per_rule`).

*Bare `grackle config` prints the effective config* — `--effective` is the
documented spelling and accepted, but there is nothing else the subcommand
could mean today and a usage message would be a worse default. Noted so a
later flag (`--json`, `--path`) knows what it is changing.

*The `explain` alias was trivial and is in.* One `Cmd` variant, one match arm
forwarding to `run_query(Query::Explain{..})`; both TODO-1.0.md boxes are
ticked and DESIGN.md §4d's two "not built yet" sentences are corrected, since
this commit is what made them false.

*Parity:* grack.com and all four examples built before and after into separate
trees and diffed — every file byte-identical except each feed's wall-clock
`<updated>`. Zero fixture changes, zero re-blessing, no new clippy warnings
(compared before/after as a multiset).

*For the queue (small).* (i) `--effective` shows the config BEFORE
`apply_profile`, because a profile is a projection applied in Rust after
deserialization. The preamble says so when `--profile` is passed; showing the
projected result would need `Config` to be `Serialize`, and that is a
different tool (`config --projected`?). (ii) B2's note (ii) — `law_of`'s
unknown-key fallback to `Law::Atom` — is now visible in the output: an unknown
key prints with a provenance like any other, on its way to
`deny_unknown_fields`. (iii) Two collections that key the same
(`collection_key` collision) would collide in the trace; the merge would have
paired them first, so it is unreachable today, but it is the one place a path
is not unique by construction.

**2026-07-26 — Batch review 2 (Fable), covering R2, A6, B1, ba4369c (q10),
B2, B3.** Verdict: **sound to build Phase C on.** Five mutation checks
re-executed and held exactly as recorded; the merge, the derivation and the
provenance recorder verified to be one code path (`Trace::off()` on the load
path, asserted); no law silently changed in the port beyond the intended
`MarkerDef` alignment; byte-parity claims checked out. Findings, condensed:

1. *should-fix → R3 (filed above):* the depth-invariant tripwire whitelists
   depth 0, so a table-capable atom (`LocalizedStr`) added beside a deeper
   sibling would be split by `merge_to_depth` with every guard passing —
   latent, no such field exists today.
2. *should-fix → R4 (filed above):* `engine_defaults()`'s doc comment cites
   a test that does not exist; deleting an entry passes the suite. The
   repo's own "guard without a mutation check" disease, in a comment.
3. *should-fix → §4 rule (applied) + §7 question (filed):* rustfmt 1.9.0
   wants to reformat 13 untouched files; repo-wide `cargo fmt` in any agent
   commit would drag unrelated hunks into a pathspec commit. Rule added;
   the toolchain pin is Matt's call.
4. *note → C6(e) (applied):* `--effective --profile nosuch` asserts a
   projection that would never run.
5. *note → §7 (filed):* nested struct-level defaults (`links.policy`,
   `i18n.default`) are invisible in `--effective` — an omission, not a lie.
6. *endorsed:* R2's empty-child idiom — incidental library behavior, but
   pinned by direct globset tests, which is the right treatment; the
   include-inside-exclude residual is correctly hypothetical.
7. *endorsed (+ C4 note applied):* A6's cross-extension-only argument holds
   on case-insensitive and normalizing filesystems — variants are different
   byte stems, which lands them in C4(b)'s unknown-stem territory instead.
8. *endorsed:* q10's disposition — reasoning and implementation; the
   counterargument conflates the row ladder with the config ladder. Not
   fait accompli: revert is small and corpus-inert; §7 annotated.
9. *scope note:* below the top level the merge collapses subtrees to a
   uniform depth rather than recursing over `Shape`; B1's two invariants
   are the load-bearing proof of that equivalence post-B2 (not leftovers),
   with finding 1 the one soft plank.

**2026-07-27 — R3 + R4.** Landed together in one commit — they share
`config.rs`, and splitting a file across two pathspec commits needs index
games this ledger's own rule is there to avoid.

*R3: the variant, and why it is a fourth one.* `Shape::TableAtom` sits beside
`Shape::Atom` with the same depth (0) and the same law (`Law::Atom`) — a
`LocalizedStr` merges exactly as `Kind` does, and §3 table D is unchanged.
Growing `Atom` a flag was the alternative; a separate variant reads better
because the three structural variants are named for what a type IS, and "an
atom spelled as a table" is a kind of thing rather than a property of one.
The module doc now says the law reads three questions off a type and this
variant answers a fourth that only a DESCENT asks.

*The invariant became a function.* `an_atom_a_deeper_sibling_would_split`
takes the walked shapes and returns the sentence, so
`a_nested_struct_ends_at_one_depth` asserts `None` over the real config and
`a_localized_string_beside_a_map_would_be_split` fires it at a `[i18n]` the
config does not have — which is the only way to mutation-check a tripwire
whose whole point is that nothing trips it. That second test also asserts
what the invariant BUYS, since a shape alone does not say: it runs
`merge_to_depth(base, site, 2, …)` on the hypothetical and shows the base's
`en` and the site's `fr` coming back as one localized string, written by two
files and by no author. The whitelist is now "depth 0 **and not** table-
spelled"; a scalar or an array at depth 0 is still descended past, which is
what made the old rule right in the first place.

*The merge-refusal decision: NO, and the reasoning.* The item asked whether
`merge_to_depth` should refuse to descend into a table-capable atom. It
cannot, cheaply: the merge carries a single `n` and no shape, so refusing
means threading a narrowed `Shape` through `merge_by`/`merge_to_depth` and a
per-level lookup — at which point the depth scalar is redundant and the
honest version is the merge RECURSING over `Shape`, which is batch review 2
finding 9's scope note answered rather than a guard bolted on. It would also
have to be mirrored in `note_depth`/`note_table`, or `--effective` would
claim a granularity the merge did not use — and B3's whole design is that
those two are one code path. The half-version (an `Option<&Shape>` consulted
only to stop) buys nothing the invariant does not already buy and leaves the
recorder disagreeing. So: enforcement stays in the description, where it is
TOTAL — the walk visits every struct in both shape trees, so there is no
field it can miss — and `Shape::is_table_atom` is `#[cfg(test)]` to say that
out loud rather than leave a method the merge looks like it might call. *For
the queue, if anyone wants the structural version: it is one item — "the
merge recurses over `Shape`" — and it retires `Descend(n)`'s number, not just
this hole.*

*R4: mechanical, and where it lives.* The defaulted set is read off `Config`'s
own text (`include_str!` of `config.rs` from `tests/base_config.rs`), keying
on `#[serde(default = "…")]` — the attribute that names a function returning
a value — as against plain `#[serde(default)]`, which is an empty container
with nothing to print. Extraction beat enumeration: the list would have been
three names and a comment asking to be updated, which is the disease R4 was
filed for, one level up. It yields exactly `extends`, `root`, `gitignore`
today, asserts loudly if a defaulted field is ever `rename`d (the extraction
reads Rust names), and fails if the struct moves.

*The assertion is PRESENCE, not `# default`.* A key `base.toml` writes needs
no entry in `engine_defaults()` — the value prints as `# base` and nothing is
lost — so requiring the `default` provenance would fail the day the base
grew a `gitignore = true` line, for no fault. Presence in the empty site's
output is exactly the promise the doc comment makes.

*It lives in `tests/base_config.rs`*, beside the two `--effective` tests it
completes, rather than in `config.rs` where `engine_defaults()` is: it is a
claim about `examples/minimal`, and that section already carries the argument
for why no golden file exists. The doc comment now names the file as well as
the test, since the name alone was what went stale.

*Mutation-checked four ways*, each restored: the depth-0 whitelist put back
(fails the hypothetical test), `LocalizedStr` flipped to `Shape::Atom`
(same), the hypothetical field added to the REAL `I18nCfg` shape (fails
`a_nested_struct_ends_at_one_depth` and, as a real field addition would,
`the_shape_covers_the_config_surface`), and `("extends", …)` deleted from
`engine_defaults` (fails `every_defaulted_scalar_is_printed` and nothing
else — it was the sole guard, which is the finding restated as a test run).

*Parity:* no production behaviour touched — `TableAtom` has `Atom`'s depth and
`Atom`'s law, and the only other edits are tests and comments. grack.com,
field-notes and theme-preview print byte-identical effective configs across
the change; zero fixture changes, zero re-blessing; clippy's warning multiset
identical before and after. Formatted by hand (§4): `rustfmt` on `config.rs`
still wants two hunks at lines 1962 and 2424 that this work never touched,
which is finding 3 seen from the inside.

**2026-07-27 — C1.** Landed. `CASCADE_KEYS` is gone and `apply_defaults` has no
`reserved` parameter: the four are `schema::CASCADE`, declared in `base.toml`'s
`[schema]`, and every key a marker or a rule sets now takes one path. Table B's
"every key here is schema-typed" is true of the code.

*`Cascaded` survives, and its name finally fits.* It is a typed READ of the
row's resolved fields — `worn("theme")` off `fields.values` — plus the one
vocabulary the engine closes (`shell`). The cascade is two calls above it,
`schema::cascade_front` then `apply_defaults`, in that order, which is the same
ladder every declared key climbs; keeping the struct meant two call sites
unchanged and the posts/tree "one spelling" property intact.

*The seeding step is the part the item did not predict, and it is load-bearing.*
The four arrive on named `FrontMatter` fields rather than in `extra` — serde had
already typed them, which is exactly why front matter never had this disease —
so `validate` never sees them and `apply_defaults`' nearness test ("does the row
already carry this key") was blind to them. `cascade_front` seeds them, which
makes that test true and, in the same move, carries §4e's "every row is
governed" to the four: a row wearing one no schema declares is a load error
naming the knowns. **That is the one behaviour change beyond typing**, and it is
what made `theme-preview` grow two lines.

*The values STAY in `fields` rather than being lifted out.* They also land on
the row's named fields, so `layout`/`toc` read identically through `Row::field`
and `theme`/`shell` — previously unfilterable — now answer from `fields` at both
layers (a route copies `p.fields`). Stripping them would have left four names in
`declared()` that type-check and answer `Null`, which §4e names as the worse
failure. Visible consequence: `grackle export`'s JSON now shows these keys under
a row's `fields`. No built artifact contains them (the search payload lists six
named keys; `fill_from_fields` is keyed by PART name and no kind in `parts.toml`
— nor any site's `[[parts]]`, of which there are none — declares one of the
four).

*Why the base may declare `layout` and `toc` at all.* Both are `reserved` names
(`row_schema()` has them), and q51's guard refuses those because a declaration
would be silently overruled. For these four it is not overruled — the value goes
INTO the row's named field — so `parse_fields` takes a cascade name, but only at
the type the engine reads it at. Retyping (`toc = { type = "string" }`) is a load
error: it would type a rule's value one way and have `cascade` read it the
other, which is this item's silence one rung out. Restating the engine's line is
legal, which is what `examples/raw` does. `row_schema()` was left alone — the
alternative (drop `layout`/`toc` from it and let `declared()` supply them) would
have narrowed the filter vocabulary of every `extends = "none"` site for no
gain.

*Corpus: nothing ill-typed, and little to type.* The only cascade-key defaults
in the repo are grack.com's two `defaults = { layout = "post" }`, field-notes'
one, and field-notes' `defaults = { theme = "recipes" }` — all correct, all on
base-inheriting sites. (D1 plans to delete the three `layout = "post"` as
no-ops.) Front matter is where the four actually live: theme-preview's ten
`layout: page` and one `toc: true`, all under `guide/`, on the one
`extends = "none"` site that has content — hence its two new `[schema]` lines.
`theme`/`shell` are deliberately left undeclared there: nothing sets either on a
row (the theme axis sets its member field at render), and a site declaring what
it does not use is how a vocabulary stops meaning anything. `examples/raw` takes
all four, being the base printed out. **Zero fixture configs needed governance
fixes** — every fixture site with front matter inherits the base.

*Parity:* all five sites built before and after into separate trees and diffed —
byte-identical but for each feed's wall-clock `<updated>`. Zero re-blessing; one
new fixture (`cascade-default-mistyped`); clippy's warning multiset identical.

*Mutation-checked three ways, each restored:* re-exempting the cascade keys in
`apply_defaults` fails six unit tests AND the fixture, which then builds with
its `notes/outline.md` carrying no outline and nothing said — the silence, live;
dropping the `ty != engine` test accepts `theme = { type = "int" }`; dropping
`cascade_front`'s governance check accepts an undeclared front-matter `layout`.

*For the queue (small).* (i) C2 inherits a cleaner surface than it expected: a
row's `theme` is now a typed declared field, so validating its VALUE against the
theme registry has one place to sit for the front-matter and default rungs
alike. (ii) `schema::CASCADE` is a second statement of what `row_schema()` says
about `layout`/`toc` (types only) — they cannot drift silently, since the guard
compares them, but §7's vocabulary pass may want to notice. (iii) DESIGN.md
§4e's `CASCADE_KEYS` sentence and table row were corrected in the same commit;
they were made false by it.

## 7. Serious questions (parked for the wrap-up conversation)

Not work items. Each needs Matt's call; agents must not attempt them.

1. **`bucket` / §6a bare-name resolution** — build it or delete it. The tour
   (§0) and base.toml comments imply it works; it is specced, unbuilt, and
   unexercised (all 194 corpus refs are paths).
2. **`variant` validation policy** — "silent variant degradation is the
   design" for row requests across themes, but a view's `variant` naming a
   fragment *no loaded theme provides* is probably a typo. Warning? Error?
   Where's the line?
3. **The `extensions` array knife** — adding `ico` wholesale-replaces the
   base's six. Arrays stay atoms by law; if the pain is real the answer is a
   data-model change, not a softer merge.
4. **Per-post `<style>` layering** — today unlayered (beats everything);
   §6c's `@layer post` would invert that. Behavior change on existing posts;
   needs a decision before building §6c.
5. **A view's `theme` when embedded** — `{% view %}` renders through the
   host page's theme; a view-declared theme silently doesn't apply. Correct?
6. **The vocabulary pass** — `shell` ×4, `kind` ×3, `match` ×3 (two path
   bases), `from`/`over`, `layout` ×2, `[[parts]]` vs `parts.toml` `[[kind]]`
   spelling. Every rename touches documented surface; decide before 1.0.
7. **Profile `noindex` vs site `robots`** — C6(d) makes a provisional call;
   confirm it.
8. **The ancestor takes the global `declared()` name** *(A4 residual, batch
   review 1 finding 4)* — an ancestor and descendant `.schema.toml` may
   legally disagree on a field's type (nearest wins per row), but the global
   filter vocabulary flattens to the ancestor's type, so a `where` can
   type-check against one type while some rows carry the other.
   Deterministic, documented on `declared()`, deferred to B3's legibility —
   is that the end state, or should a cross-type ancestor/descendant pair be
   an error too?
9. **Markers: configured ×5, used ×0** *(A5 + batch review 1 finding 10)* —
   every site declares `[markers]`, no directory carries a marker file.
   Keep as documented convention, trim from the base, or leave as-is?
   *(Batch review 2 datum: `--effective` now surfaces the three inherited
   markers to every site, which slightly strengthens "keep".)*
10. **Is a marker payload a definition or a bag?** *(B1)* — table A says the
    payload table is the atom (`Descend(1)`); the type is a map of maps, so
    Law 2 derives `Descend(2)`. Inert today (every redeclaration restates the
    base's payload verbatim), and the only key where structure and the hand
    table disagree. A site that wrote `".noindex" = { hidden = true }` over
    the base's `{ noindex = true }` would get one key under the table's law
    and both under the type's. Which is the law — and if it is the table's,
    should the payload become a named struct so the types say so?

    **Provisional disposition (B2 proceeds on it; veto at the wrap-up):**
    the payload is a **definition**. DESIGN.md §4d already lists `[markers]`
    among the registries that "shadow by name, whole entry", and the
    registry rule's argument applies with full force — a marker's *meaning*
    should never be composed of two files ("what does `.archive` mean"
    answers from one place). The type under-describes the semantics, so B2
    newtypes the payload (a `MarkerDef` wrapper deriving `Atom`), which
    empties `KNOWN_EXCEPTIONS` and keeps "derivable from types" honest —
    the same move Law 2 already makes for `LocalizedStr` (enum = atom).
    *Batch review 2 endorsed both reasoning and implementation, and
    confirmed this is NOT fait accompli: the revert is small (unwrap the
    newtype, flip one test, one table-A row) and corpus-inert — treat as
    genuinely open with a shipped default.*
11. **Should `--effective` show struct-level defaults?** *(Batch review 2,
    finding 5.)* Top-level scalars nobody wrote print as `# default`, but
    nested defaults (`links.policy = strict`, `i18n.default = "en"`) are
    invisible when neither base nor site writes the table. An omission, not
    a lie — does `--effective` grow struct-level defaults, or is that a
    future `config --projected`'s job along with profiles?
12. **Pin the toolchain?** *(Batch review 2, finding 3.)* rustfmt 1.9.0
    wants to reformat 13 files the merge work never touched. A
    `rust-toolchain.toml` pin would stop format drift across agents and
    humans, but changes your own build — your call. (§4 now carries the
    interim "no repo-wide cargo fmt" rule.)
