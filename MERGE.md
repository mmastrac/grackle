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
| `[[parts]]` | — | the array | same (arrays are atoms); vocabulary ladder: site `[[parts]]` under engine `parts.toml`, engine part wins collisions |
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
(6). Atom = the field definition. **New: two same-rung declarations of one
name with conflicting types are a collision error**, not alphabetical order.

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
- Check the box here and note deviations in §6 when an item lands. Do not
  reorder or rescope other items.

## 5. The work plan

### Phase A — make the current law enforceable

- [ ] **A1. `deny_unknown_fields` sweep.** Add to `Rule` (config.rs:726 —
  the worst: `[[collections.rules]] theme = "x"` is silently ignored),
  `Site` (config.rs:604), `I18nCfg` (config.rs:353), `LinksCfg`
  (config.rs:535), `ShellDef` (config.rs:343). Make `.schema.toml` field
  tables reject keys other than `type` with an error naming the file and the
  knowns (schema.rs:148-152 currently reads only `["type"]` and drops the
  rest). Fix any configs/fixtures that break; add `expected-error` fixtures
  for at least the `Rule` and `.schema.toml` cases. Note: `Site.noindex` is
  `#[serde(skip)]` — confirm profiles can still set it after the change.

- [ ] **A2. Exhaustive destructure in `merge_base`/`merge_collection`.**
  In `crates/source/src/config.rs` (~200-226, ~278-301), destructure the
  full `Config` (and `Collection`) so adding a field without deciding its
  merge law is a **compile error**, not a silent fallthrough to
  wholesale-replace. Annotate each key with its law (registry / bag / atom /
  annotation) in a comment. Fix the doc comment at config.rs:189-194 which
  omits `schema` from the registry list the code includes. Behavior-neutral
  except as prepared by A3. *[parity]*

- [ ] **A3. `[axes]` becomes a registry; `[links]` merges per-key.** Axes:
  merge by axis name, whole-definition atom (today it falls through to
  wholesale replace — the bug that motivated Law 2's derivation). Links:
  per-key bag (invisible today; `policy` is its only key). Mutation-checked
  tests for both: a base-declared axis must survive a site declaring a
  different axis, and deleting the registry arm must fail the test.

- [ ] **A4. Same-rung schema collisions are errors.** `Schemas::declared()`
  (schema.rs:207-220) flattens every subtree schema with `or_insert`, so two
  `.schema.toml` files declaring one name with **different types** resolve by
  alphabetical directory order for the global filter vocabulary — silently,
  and possibly differently from the per-row nearest-wins `resolve()`. Make
  conflicting-type redeclaration at the same rung a load error naming both
  files and both types; same-type redeclaration stays legal. Mutation-check.

- [ ] **A5. Marker determinism.** Two different marker files in one directory
  setting the same key resolve by walk order (markers.rs:54-57
  `slot.insert`). Same-directory conflict on a key = load error naming both
  marker files; different keys stay legal. (Nearest-wins across *levels* is
  the law and stays.) Mutation-check.

*→ Batch review 1 after A5.*

### Phase B — derive the merge from structure

- [ ] **B1. A structural `Merge` mechanism.** Introduce the machinery that
  expresses Law 2 in types: maps descend per key (value = atom if a struct
  or enum; descend if a map), structs descend per field, scalars/arrays/enums
  replace whole. Hand-written impls or a small derive — whichever reads
  better in this codebase (keepcalm-school minimalism; no new heavy deps).
  Unit tests proving each table's derived depth matches table A above,
  including `[records]` at depth 2 and `[html.head.*]` at depth 3.

- [ ] **B2. Port `merge_base` onto it; delete the dispatch.** Collections
  keep their annotation (key on `source`, rules prepend). The A2
  destructure remains as the compile-time completeness check; the per-key
  law now comes from B1's structure, not from a hand-assigned depth.
  Byte-parity across grack.com, field-notes, minimal, raw, theme-preview and
  the fixture suite. *[parity]*

- [ ] **B3. `grackle config --effective`.** Print the merged config with,
  per key: provenance (site / base / default) and the law applied (atom
  taken from X vs merged). This is the item DESIGN.md §4d says "should ship
  before 1.0" and the review said is the single highest-leverage tool. TOML
  output with provenance comments is fine for v1. Include at least one test
  asserting provenance for a shadowed registry entry and a merged bag key.

*→ Batch review 2 after B3.*

### Phase C — the laws hold at every rung (strictness symmetry)

- [ ] **C1. Dissolve `CASCADE_KEYS`.** `theme`/`shell`/`layout`/`toc`
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
  knowns (slots.rs:167-195 accepts any stem). (c) Exclude engine stream
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
