# Graveyard

Compressed records of prose deleted from the design docs, so it can be swept
back into the manual rather than lost. One line per removal; measured numbers
survive as numbers, because they are often the only remaining record.

**Scope.** Three files, compressed 2026-07-25:

| file | before | after | |
|---|---|---|---|
| `DESIGN.md` | 5132 | 2333 | −55% |
| `themes/DESIGN.md` | 591 | 287 | −51% |
| `themes/README.md` | 227 | 63 | −72% |

Every `DESIGN.md` entry below cites the OUTLINE chapter that already covers it,
or the criterion that licensed the cut — so a bogus justification is visible
rather than buried. Section and subsection headings were preserved throughout:
where a whole subsection deserved to go, it is marked `PROPOSED WHOLE-SECTION
CUT` and left in place for a human to decide. There are four of those.

> **The failure mode this pass has, found once and probably not unique.**
> These documents record a claim in one section and its later correction in
> another — the correction usually living in exactly the dated ledger prose the
> criteria call "build narration". Deleting the narration can therefore leave a
> superseded claim standing *alone and looking authoritative*, having removed
> the only thing that contradicted it.
>
> Confirmed instance: `themes/DESIGN.md` §7 said "typography is opt-in
> (`@import "type";`)". Its correction — the `_type.scss` split into an
> always-on ladder and opt-in skins — lived in the Landed ledger, which this
> pass removed. The doc came out of the compression *more* wrong than it went
> in. Fixed, and the entry above is struck through so it is not swept back.
>
> Anything in this file that reads like a rule rather than a record should be
> checked against the code before it is reinstated. See `TODO-1.0.md`.

---

## themes/README.md

### Design attribution

- `miroir` is drawn from ojeda-e.com (fixed dark rail, right-aligned nav,
  centred title over a hairline rule) and Zola's `daisy` (saturated brand bar,
  raised card feed with icon meta rows, accent CTA). Daisy advertises 37 colour
  schemes; here that is one palette plus three subtheme tokens — the same
  feature expressed as the thing the gallery argues for.

### Base theme architecture *(the rules live in DESIGN.md §5e — check before reinstating)*

- **The base is structure, never decoration** — a rule belongs there if a theme
  would have to re-derive it, not if a theme would have to undo it. A `content:`
  separator, an ellipsis, a pill are decoration and live in `vanilla/`.
- **Ship a shell, own the frame** — the base's page geometry keys on
  `[data-frame]`, stamped by its own `shell.html`, so a theme writing a shell
  inherits none of it. Correct for `atlas`'s full-bleed sticky bar and
  `miroir`'s fixed rail, which a centred `--measure` column would break.
- The base is `crates/grackle/assets/base/` — 14 fragments, 4 stylesheets,
  `include_str!`'d as `parts.toml` is. Gallery went 109 files → 34.

### Cascade and configuration

- Per-row theme cascade: front matter → rule default → `[site] theme` → the
  `default` directory → the base theme. A misspelled theme is a load error.
- Subtheme tokens ride after a colon (`theme: "ledger:dark"` → `data-subtheme`
  on `<html>`, CSS subselects `[data-subtheme~="dark"]`). They compose:
  `marginalia:dark:wide`. Every gallery theme uses one to force light or dark
  against `prefers-color-scheme`.

### Token contract teaching

- File roles: `_tokens.scss` holds every literal (**the file you edit**),
  `theme.scss` holds geometry and no literals, `*.html` only the fragments the
  theme rearranges. **The smallest theme is one file** — a directory holding
  only `_tokens.scss` is a complete theme.
- **Reset + type ladder are always on**; safe to impose because they read only
  tokens, so `--size`/`--scale` moves the whole hierarchy without restating a
  rule. Under a theme with its own type sheet the ladder measured *inert*.
- **Skins are opt-in** (`@import "skin";`) — blockquote rule, code panel, table
  borders, callout — because they are not inert: under grack.com's theme they
  move a paragraph 19px and a listing page 61px. `vanilla` imports neither and
  is still a whole page.
- Cascade layers `@layer base, theme`: a theme rule beats a base rule
  regardless of selector specificity.
- **Breakpoints cannot be tokens** — a media query's condition resolves before
  custom properties do, so they are Sass variables (`$collapse` in marginalia,
  `$drop-aside`/`$drop-sidebar` in atlas, `$unpin-rail` in miroir) declared at
  the foot of `_tokens.scss`. `ledger` and `terminal` have no breakpoint at all.

### Partiality

- Themes are partial: `vanilla` ships no fragments; `ledger`/`marginalia`/
  `terminal` ship shell + document + summary + tokens; `atlas`/`miroir` add
  `listing--cards`, `listing--gallery`, `summary--card`.
- A variant a theme lacks degrades silently to the plain kind — row-declared
  variants are requests, not demands. The base ships `summary--figure` but no
  card/gallery variants, so four of six gallery themes fall back in public.

### Authoring gotchas

- **Rule 2 deletes an empty part's element, not a wrapper the fragment
  invented.** `atlas` gets optional sidebars free because the rails *are*
  parts; `terminal` and `miroir` group two parts in a `.doc-meta` bar and pay
  `:not(:has(*)) { display: none }`. The base pays the same tax for its
  `<footer>`.
- **A flat fragment plus CSS Grid means one row per child.** `marginalia`'s
  margin column started as `grid-template-columns` and was wrong: four margin
  items grow four empty rows opposite them and the prose starts *below* its own
  marginalia. Floats out of a padding inset express "beside"; grid expresses
  "table".
- **A placeholder link is a conditional** — `<a>` with no `href` is how the
  engine says "current page" or "nowhere to go", so inert crumb tails, the
  current page number and index-less tree nodes are all `a:not([href])`.
- **`aria-current` and `data-relation` come from the engine** — style them,
  don't reinvent them. The language switcher is the `translations` relation
  repositioned by CSS: it keeps its place in reading order and in the
  accessibility tree, and only its pixels move.
- **Flags are attributes, not content** — `data-truncated`, `data-tree` on the
  fragment root. `ledger`'s read-more fade is a rule on a fact.

---

## themes/DESIGN.md

### Landed ledger and second review pass *(build narration)*

- The base went further than the doc proposed: compiled into the binary rather
  than emitted beneath every theme, fragments as well as CSS.
- Vanilla is zero-fragment — the 14 fusion fragments moved to the base, where
  there is one sensible way to write each. `ledger` 19 files → 5; gallery
  109 → 34.
- `body > [data-kind="shell"]` replaced by `[data-frame]`.
- `callout` relocated to `_type.scss` (opt-in), so the always-on reset carries
  no site vocabulary. Consequence: **vanilla renders a callout as plain text**,
  which is readable, so opt-in is defensible rather than a dodge.
- The `_type.scss` ladder/skins split was decided by measurement, not taste —
  see the 19px/61px figures above.
- A `_tokens.scss` nothing imports now warns; a stylesheet that fails to
  compile fails `build` (serve prints and carries on), because publishing a
  site whose stylesheet silently failed is the worst outcome — it looks
  deployable.
- A dev build serves `assets/base/` from the source tree and watches it, so
  editing the floor reloads like editing a theme. Released binaries never touch
  disk.
- Correction on the record: an earlier ledger cited "+320px on a long post" as
  the base typography's cost under grack.com. It was a measurement artifact
  (snapshots taken before fonts and images settled); measured symmetrically the
  base is inert on grack.com.

### grack.com dropped its Meyer reset *(migration story)*

- grack.com carried the Meyer reset (a hard 90-element blanket) and its 860
  lines of chrome were written against that blank slate. Dropping it naively
  grew the site 16% and over-indented content lists.
- It came out as **six targeted rules** grack now owns
  (`themes/default/_reset.scss`), found by diffing every element of five page
  types against the frozen Meyer build: `body { line-height: 1 }`; headings and
  unspaced block elements zeroed; `ul, ol` un-bulleted and un-indented; three
  overrides for base floor rules on engine vocabulary (site-title link weight,
  4px neighbour-link padding, 32px summary margin).
- Verified pixel-identical across five page types. The stylesheet got 770 bytes
  *smaller*.
- **The lesson worth keeping: a light base reset is not a drop-in for a hard
  one.** A theme moving onto the base owns the specific blank-slate assumptions
  its chrome was written against — a short, legible list, not a third-party
  blanket.

### The vanilla experiment: superseded findings

- Finding 1 ("zero fragments is falsified") **inverted**: the 14 fragments were
  trivial *and identical in every theme*, which made them engine material. The
  finding stands, its conclusion reversed.
- Finding 3 (the null-theme shell needs `body > [data-kind="shell"]`)
  **superseded**: the base ships a shell, so the canonical-shell case no longer
  arises. It answered a question the finding did not ask — not "how does a
  shell-less site get the measure" but "how does a theme *with* a shell avoid
  inheriting one it must undo".
- Finding 4 (`_base.scss` carries the `callout` impurity) **resolved by
  relocation** into the opt-in typography partial.
- Tooling note kept for anyone writing a build step: the themes' SCSS uses
  inline `//` comments, valid in Sass and invalid in CSS — a naive concat
  silently eats the *following* declaration. Concatenate compiled CSS, never
  raw SCSS.
- Verified in-browser at the time: both OS colour schemes with zero authored
  hues; placeholder-link rendering; rule-2 deletion. **Forced-colors mode is
  still claimed from spec and untested** (now a TODO).
- One bug the hand-built demo could not have caught, found at mobile width:
  `pre { overflow-x: auto }` had landed in the opt-in typography partial, so a
  long code line under vanilla scrolled the whole page sideways. It is in the
  reset now, beside `img { max-width }` — a wide code block breaks the *page*,
  which is the same class of bug as an oversized image and not a matter of
  taste.

### §7 floor: as-built corrections to the proposal

- The floor is a **theme, not a layer the engine emits** — so the guarantee
  strengthens from "reasonable with every theme" to "reasonable with **no**
  theme". `Theme::null()` is the base.
- **Two layers, not three**: base and canonical turned out to be one artifact.
  The sheet declares the full `reset, base, theme, overlay, post` order anyway,
  so this statement is the authority on the order rather than leaving a future
  implementer to discover that an undeclared layer sorts last by accident.
- The review test became **re-derive/undo**, which is sharper and falsifiable.
- ~~Typography is opt-in, because a second heading ladder underneath a theme
  that has one is invisible and wrong.~~ **Stale when written** — this was the
  pre-split claim, and `_type.scss` was later split into an always-on ladder
  and opt-in skins. Do not sweep it into the manual; ch. 14 already has the
  correct version. See the note below.

### §10 implementation order *(completed checklist)*

- Step 4 done: vanilla runs through the real engine, fragments pass `binder.rs`
  validation, grass compiles its sheet, the portability falsifier landed as
  four tests. Step 2 partial: a tokens-only child compiles (`css_pass` falls
  back to `_tokens.scss` when there is no `theme.scss`), and the dev server
  invalidates through the `themes` symlink.
- Four tests hold the base, each mutation-checked when written: the base drops
  only what an EXEMPT list declares (5 entries, each with a reason), every
  theme keeps a row's name, no theme uses a token nothing defines, no
  `theme.scss` names a colour. Plus four on the CSS pipeline: the tokens-only
  theme compiles, a theme with a sheet is not given typography, the full
  cascade order is declared, a wide code block never scrolls the page.
- Edge 2 is linted rather than deferred to `theme check`: a test asserts every
  theme's shell places every slot the base's does. Verified by deleting atlas's
  copyright slot and watching it fail.

---

# DESIGN.md

Removed in the 2026-07-25 compression pass (5132 → 2333 lines). Every line
cites the OUTLINE chapter that already covers it, or the removal criterion.
Criteria: 1 covered by the manual · 2 changed/historical · 3 settled discussion
· 4 build narration · 5 completed item · 6 internal duplicate.

<!-- bucket 1 -->

## §0

- Tour introduction paragraph: "Every step below is built and measured, with three deliberate gaps..." — criterion 4: build narration with specification notes
- Authoring interface detail: "That is the whole authoring interface..." — OUTLINE ch. 2, ch. 3
- Load-time checking paragraph: "Everything is checked at load time..." — OUTLINE ch. 4
- Query loop example: "Nobody writes `{% for post in site.posts %}...`" — OUTLINE ch. 6
- View composition paragraph: "The new post enters `published`..." — OUTLINE ch. 6
- Filter type-checking sentence: "Filters are parsed and type-checked..." — OUTLINE ch. 6
- Doc model detailed section: blocks, notes, rewrites, facts with three addressing modes — OUTLINE ch. 26
- Layout kind explanation: "A `document` emits a part map..." — OUTLINE ch. 12
- Theme placement paragraph: "A fragment is straight-line HTML..." — OUTLINE ch. 14
- Theme slot HTML example — OUTLINE ch. 14
- CSS geometry paragraph: "Modern CSS is the declared baseline..." — OUTLINE ch. 15
- CSS sidenote grid example: "this theme wants Tufte sidenotes..." — OUTLINE ch. 15
- Sidenote consequence sentence: "The footnote just became a sidenote..." — OUTLINE ch. 15
- Build/serve/query block with timing detail — OUTLINE ch. 2, ch. 7
- Day two table entries: photo gallery, per-post CSS, dark mode, Rust box, new look, margin footnotes — OUTLINE ch. 12, 13, 14, 15, 27
- Closing design rule: "want an `if` → you're missing a fact..." — OUTLINE ch. 25

## §1

- None removed (kept as foundational core idea)

## §2

- Salsa consideration section: "Considered: `salsa` for automatic..." — criterion 4: build decision ledger and infrastructure note

## §3

- Objects merge history: "Objects went last and cost nothing, because q51 had already written..." — criterion 3: concluded design rationale for merged table architecture
- Objects table history: "What the objects table had been doing, and what took each over..." section — criterion 3: implementation narrative of three-table consolidation

## §4

- DB DEFAULT analogy sentence: "The DB analogy is a `DEFAULT` clause..." — criterion 6: duplicates concept in "rules supply column values"
- Detailed worked examples in resolution sections — OUTLINE ch. 4, ch. 5

## §4a

- "**What this deleted.**" section: Jekyll-era behaviours `search_docs` and `post_trail` — criterion 2: historical change narrative (port effects)
- "**Punted, deliberately.**" section: baseurl, exclude, defaults, analytics, _config-fast — criterion 3: design decisions deferred from v1 spec

## §4b

- "**That exemption has a cost worth knowing about.**" section with walkdir prune details and timing: ~80ms vs ~6ms — criterion 4: implementation performance measurement
- Optimization note: "folding it into the existing walks is an available optimisation..." — criterion 4: infrastructure notes

## §4c

- Verification paragraph: "Verified: all 10 build-artifact excludes..." — criterion 4: measured audit of configuration
- Marker scan performance table with `gitignore = false` measurements (205.4ms scan, 232.4ms total) — criterion 4: performance measurement ledger

---

## OUTLINE duplication noticed

None noticed.

<!-- bucket 2 -->

PROPOSED WHOLE-SECTION CUT: `### What it cost` — criterion 4 (measurement story about test counts/mutations), kept heading with one sentence.

PROPOSED WHOLE-SECTION CUT: `#### What the fold found` — criterion 4 (byte oracle measurement story), kept heading with q51 sentence.

## §4d

- Opening setup: examples/minimal was 27 lines with 20 boilerplate; "should fall as defaults land" — criterion 5: completed item now stated as rule
- Base theme argument intro (lines 9-15) establishing the background — criterion 2: historical context, not current design
- Empty file boast "27 lines to 0" — criterion 5: completed milestone, conclusion captured in rule
- Date marker "(built 2026-07-25)" on heading — criterion 4: dated build ledger
- Content of "What it cost" section (lines 180-188): grack.com and field-notes byte-identical URL set, eight merge tests, three falsifiers, mutation checks — criterion 4: build measurement story; heading/summary kept

## §4e

- Date markers "(Matt, 2026-07-25)" and "(2026-07-25)" on headings — criterion 4: dated ledger
- Matt discussion quote: "any time we see `draft` or `hidden` explicitly in engine code, that's a big smell" (lines 216-217) — criterion 3: discussion quote whose conclusion is now the stated rule
- Follow-up: "He is right, and the first answer this document gave..." explanation (lines 218-221) — criterion 3: discussion narrative leading to the rule
- "The audit, so the size is on the record" (line 222 intro) — criterion 4: procedural note about audit purpose, not the design
- Matt's second quote on schema governance (lines 248-249): "I think we should consider all sites governed..." — criterion 3: discussion establishing a rule now stated plainly
- "It cost exactly one line across every site" explanation in Every row is governed — criterion 4: cost measurement
- Why the flag move was one step: "Splitting... looked reasonable and was not" (lines 296-302) full explanation — criterion 3: decision narrative whose conclusion ("it went as one") is the rule
- Second defects paragraph intro and narrative flow (lines 310-312) — criterion 3: discussion setup now condensed to rules
- Matt's shape attribution on head config — criterion 3: author quote, conclusion stated in mechanism
- Content of "What the fold found" section (lines 444-460): grack.com/field-notes/theme-preview byte oracle, head tag SET comparison, 559+41+97 pages, three head differences, hardcoded description/og_type findings — criterion 4: measurement story; heading with q51 sentence kept
- Head tier measurement "~85 B" stale note (line 442) — criterion 4: measured metric superseded by new design

---

### None noticed.

<!-- bucket 3 -->

## §5

- Route example configs with layout directives and feed shell — OUTLINE ch. 6
- ⚠️ sitemap filter audit (noindex, paginated pages testing) — criterion 4: build narration verifying rule
- Verification data: 573 URLs = reference's 556 + 17 postdated rows — criterion 4: measured build ledger
- `dir` is distinct from `ext == ""` explanation and four-extensionless-binary count — criterion 4: build narration
- `over = "*"` view count (1544→1559) symptom story about iteration order — criterion 4: build narration
- Filter language operators table (year=="2022" errors, tags=="rust" errors, etc.) — OUTLINE ch. 32b covers this
- Corpus verification counts: tags 44 matches, description 7 matches, "rust" in tags 5 — criterion 4: build narration

## §5a

- Six Jekyll layouts and six includes diagnosis (three concepts, three queries with disagreeing filters) — criterion 2: historical narration on port from Jekyll
- Schema layer explanation (Jekyll has none, conflates other three) — OUTLINE ch. 12 explains
- `<head>` computed facts story with og:type/BlogPosting example — OUTLINE ch. 12 and 32a
- Per-row themes root explanation (built 2026-07-25 cascade) — OUTLINE ch. 13 covers per-row
- Theme is chosen per row vs site-wide explanation — OUTLINE ch. 13
- Layout kinds follow from row type (page vs post distinction collapses) — OUTLINE ch. 12
- Schema drives rendering: render directive / layout hint distinction — OUTLINE ch. 12 and implicitly ch. 21
- Class as contract vs implementation detail ({% image right foo.png %}) — OUTLINE ch. 15
- Chrome parity tradeoff story (bodies byte-identical, chrome by eye) — criterion 4: build narration about cost and verification

## §5b

- No nodes with .style.scss yet on grack.com code/writing sections — criterion 4: measured status ledger
- Mindstorms 17 pages with inline `<style>` as use case — criterion 4: motivating build audit

## §5c

- `/` example roles: intro/left/right with explanations — OUTLINE ch. 18 (landings mode B)
- Grid layout `.blocks-50` example — OUTLINE ch. 18 mode B landing
- Five-opinions problem as historical bug (blog_index vs tag_index vs monthly_archive filters disagreed) — criterion 2: historical port narration
- Transcribing faithfully also transcribed bug `monthly_archive` written `!draft` without `!hidden` — criterion 4: build narration/verification
- Corpus proof: 0 drafts, 0 hidden posts, flags are pure potential energy — criterion 4: measured status
- Members explanation (old re-derived matches in Rust, new members field, layout dispatch on kind not view name) — criterion 4: build narration about old impl
- Reference build byte-identical consequence — criterion 4: build verification
- `/blog` as `over = "/blog"` does not work (66 routes ambiguity, dependency graph inversion) — OUTLINE ch. 6
- `{{ 'X' | prepend: site.baseurl }}` and `{{ page.title | escape }}` verbatim rendering — criterion 4: historical Liquid compatibility narration
- Jekyll build consequence (Unknown tag 'view', publish.sh exits, reference build regeneration impossible) — criterion 2: historical consequence of Jekyll cut-over
- `over` keyword renaming from `under` — criterion 2: historical vocabulary churn
- `group_keys` three hardcoded specs were same operation story — criterion 4: build narration of generalization
- Byte-identical through general path proof — criterion 4: build verification
- `month_name` stopgap special-case until §5f formatters — OUTLINE ch. 28 and ch. 35
- Trail *content* changes and hardcoded strings retirement in crumb producers — criterion 4: build narration about previous impl


<!-- bucket 4 -->
## §5d

- Detailed classification of ~60 liquid constructs (17 queries, 22 schema facts, 12 argument passing, 8 display iteration) — OUTLINE ch. 12
- Filter language discipline and untyped runtime-resolved discussion — criterion 3: discussion with Matt resolved
- Existence proof story: `/` page as hardest case, nine-line counter loop → `where` + `limit` — criterion 4: build narration/existence proof
- Section 9a reference to liquid as dependency risk — criterion 4: build narration from earlier discussion
- `{% post_url %}` retirement details and 51-usage count — criterion 2: port history, Jekyll retirement
- Custom widgets: concrete motivation story with kramdown `markdown="1"` attribute bug — criterion 4: build narration motivating the feature
- Widget box collapse explanation and pre-widget hand-normalisation workaround — criterion 4: build narration

## §5e

- Status paragraph: verification of body byte-identity and dark mode CSS experiment/backing out — criterion 4: build verification ledger
- Identity slots implementation detail and year of usage — criterion 4: build ledger
- Implementation verification: canonical rendering closure, PartType::Url addition, dark mode landing and removal — criterion 4: build ledger
- Diagram of model pipeline (`db row → doc model → layout kind → theme`) — criterion 1: OUTLINE ch. 14 covers model
- Detailed part-map table with source column (`crumbs: fragment from tree ancestors *or* date trail`, `neighbors: stream from adjacency index`, `truncated: fact from build-time cut`) — criterion 1: OUTLINE ch. 14 covers the part schema
- Theme directory structure example with full file listing — criterion 1: OUTLINE ch. 13-14 covers this
- Fragment binder implementation details: parser strictness, wellformed nesting requirement, raw-text handling, doctype verbatim — criterion 4: build/implementation detail
- Checks at load list (unknown slot, fact-as-content, content-slot-on-void, scalar-with-data-fragment, attr-hole-naming-non-text, stream-missing-child-fragment) — criterion 4: implementation details
- After-load rendering infallibility statement — criterion 4: build property
- CSS geometry example detail: grid-template-areas for "post vs page" distinction using `[data-tree]` selector — criterion 1: OUTLINE ch. 15 covers selector contract
- Body.multipost discussion and context-class approach — criterion 1: OUTLINE ch. 15 covers `:has()` replacing upward stamps
- Historical BEM justifications (specificity wars, no scoping, decoupled) and "already a decent contract" qualifier — criterion 3: design discussion resolved
- Theme CSS checkability explanation with filter-language discipline reference — OUTLINE ch. 15 mentions checkability but DESIGN elaborates on the mechanism; criterion 1: principle covered in OUTLINE
- Worked example: footnotes → sidenotes in ~4 lines of grid CSS — criterion 4: how-to / worked example
- Placeholder link idiom explanation (`a:not([href])`) and full list of engine vocabulary — criterion 1: OUTLINE ch. 15 covers these
- Flat fragment plus grid layout trap (margin column example) — criterion 4: how-to gotcha
- Dark mode as theme concern and subtheme token discussion — criterion 1: OUTLINE ch. 15 covers dark mode as theme concern
- Syntax highlighting engine/theme obligation and four token classes (`.k`, `.s`, `.c1`, `.n`) — criterion 1: OUTLINE ch. 15 covers syntax highlighting
- Anchor tags with and without href semantic discussion — criterion 1: OUTLINE ch. 15 covers the principle via `:has()` section
- Modern CSS baseline feature-retirement table and detailed explanations (specificity-by-convention, BEM flattening, context-classes, upward-stamps, layout-shift) — criterion 1: OUTLINE ch. 15 lines 460-472 covers all baseline features and their retirements
- Inline BEM history (three historical justifications with "role names in contract, structure in selector tree" principle) — criterion 3: BEM analysis resolved in design
- Validation and auto-scoping as kept compile-step jobs with old-browser flattening dropped — criterion 4: build detail
- Archetype audit examples: document margin/sidenotes, album gallery, Pinterest masonry, magazine full-bleed, timeline, dense index surfaced — criterion 5: completed audit, only conclusion matters
- Four specific gaps with closures: hero part with cover field and first-image-block fallback; per-view fragment variants (q24); dimension facts on images (q26 with 442/468 body-image stat); per-block facts (q25 still open) — criterion 5: completed audit details, criterion 4: measurement stories
- Image dimensions implementation detail: build.rs measurement, Thumb.dims projection, tags::Ctx structural limitation, theme CSS pairing with max-width constraint — criterion 4: build narration
- Base theme preamble: null theme completion problem, five-theme gallery analysis, byte-identical fragments count (11/17), _base.scss identity — criterion 4: build motivation story
- Base theme implementation: include_str! embedding, Fragments::load merge-then-validate, theme inheritance via merge-before-call — criterion 4: implementation detail
- Gallery file reduction stat (109 files → 32, three-themes four-files) — criterion 4: measurement ledger
- Four-test verification: base drops only declared, every theme keeps row names, no-token-undefined, no-colour-in-theme.scss, mutation-checked — criterion 4: test verification ledger
- Honest weakness statement: new theme is data but part vocabulary is Rust (ch. 33) — criterion 5: completed design acknowledgment

<!-- bucket 5 -->

PROPOSED WHOLE-SECTION CUT: The disease this cured (§5h) — historical narrative explaining the problem that landings solved (four separate implementations with symptoms); the design solution is stated in "The rule" section. Kept as heading + one-sentence summary per coordinator guidance.

## §5g

- Detailed build narrative: "Built. The engine owns `root_shell`..." through "The migration was accounted byte-for-byte..."; three collapsed skeletons, `light` dissolution, subtheme movement — OUTLINE ch. 19
- Search shell build story: "Fixed 2026-07-19" with `strip_tags` bug narrative and search-core fix (1.2 KB lost) — criterion 4: dated build ledger
- Script shells narrative history, experimental bench setup (lines 179-202 minus the core concept) — criterion 4: build narration; OUTLINE ch. 19
- Row shells detailed explanation: lengthy paragraphs about objects vs `none` distinction, `light` vs `theme:` distinction, the gotcha with malformed rows, the escape-hatch framing — criterion 4: explanatory narrative
- Row tiers: detailed explanation of the "Two bits, one incoherent corner" framing and the 2×2 collapse mechanics — criterion 4: build explanation

## §5h

- "The disease this cured" (entire section): four historical implementations (view roots, index pages, home, `collection.index`), their symptoms, and diagnosis — criterion 2: describes something that changed (the four implementations and their evolution)
- "The collection stops naming itself" (entire section): historical account of `collection.crumb`/`index` dissolution (lines 434-445), the dissolution narrative (lines 447-456 mostly narrative), and byte-for-byte accounting (lines 458-466, including the one-line visible change and "page 1" suppression detail) — criterion 2: describes changes made; criterion 4: dated change ledger. Kept: the key technical finding that `params` empty, page ≤ 1 tests materialization (q46 correction).

<!-- bucket 6 -->

## Proposed whole-section cuts

- PROPOSED WHOLE-SECTION CUT: `### What this changes elsewhere` (§6d) — compressed to one summary line; cross-references to §9a/§6c already implicit in context.

## §6b

- Embeddings: detailed description of three departures from spec (stale-while-revalidate implementation, title/tags in cached text, year-distance knobs) — criterion 4: build ledger, implementation detail
- Embeddings: mechanics as specced (content-addressed caching details, L2-normalization, brute-force ranking, model download location, warm build 1.5s total) — criterion 4: implementation detail and build ledger
- Embeddings: what replaced Jekyll's LSI, comparison narrative — criterion 2: historical port narrative
- TF-IDF search: measured facts (7,125 terms, 29,793 postings, **195 KB index built in 22ms** per build, **First-click payload ≈ 288 KB**) — criterion 4: measurement story
- TF-IDF search: detail about postings capped at 40/term, scores quantised u16, title/tag hits boosted 5×, stopworded, years searchable — criterion 4: build narration

## §6c

- Opening narration about "three posts contain `<style>` blocks" and SCSS example showing nested rules — criterion 4: build narration ("three existing posts")
- Comparison to Jekyll: "Jekyll passes `<style>` through raw, so today these only render because **native CSS nesting** happens to work" — criterion 2: historical Jekyll comparison
- "Expected diff: exactly 3 posts" ledger entry with list of three specific posts — criterion 4: build ledger

## §6d

- Status line "Status: blocks built (stage A, 2026-07); notes and rewrites remain" — criterion 4: build ledger/dated entry
- Introductory measurement narrative from "Blocks, and the 93%" section (CSS truncation hiding 93% of page) — criterion 4: measurement narration. *(Heading restored with the 93% figure and byte counts carried into it.)*
- Extensive detail about measurement methodology, corpus test caveats, and CSS rule deletion — criterion 4: build narration. *(The one-sentence summary "The single mismatch is footnotes" now concludes the Blocks section.)*

<!-- bucket 7 -->

## §6e

- Implementation details about `outline.rs` and "against the example site" testing — criterion 4: build ledger
- Measurement: "7 posts use `##` headings, `code/`/`writing/` hold 36 index pages up to five levels deep, with 23 index-less directories" — criterion 4: measured ledger
- Historical note: "The refactor deleted `Block.tag` as unused; this is its replacement" — criterion 4: build history

## §6f

- Verification claim: "verified against the oracle" (i18n off byte-identical) — criterion 4: build verification ledger
- Retired spelling commentary: "`[tags.id]` spelling is a load error naming the new form" details — criterion 5: completed implementation detail
- Verification: "Main site: no overrides, all bare strings → builtins → byte-identical, verified" — criterion 4: build ledger

## §6g

- Author direction and dated narrative: "Matt (2026-07-20): *Each collection should define...* Where this comes from: relations are hardcoded — five groups in `parts.rs`, unconditional, ranging over whatever table the code reached for; that was already wrong once (adjacency crossing two dated collections, measured and fixed in q51)" — criterion 4: dated build narration, author dialogue, measurement ledger
- Implementation detail: "the unused-key load error polices the change" (§6f context about unused key detection) — criterion 5: completed mechanism commentary
- Measurement: "Zero such pairs in the corpus today; worth documenting rather than discovering" (ties on same day never happen) — criterion 4: measured build ledger
- Implementation context removed from "Two slices" section: eliminated detailed references to "Slice 1: ..., Slice 2: ..." framing in favor of concise statement of what happened

## Proposed whole-section cuts

None. All three sections carry load-bearing design arguments, rules, and honest edges that survive the manual projection in ch. 22 (hierarchy), ch. 30 (i18n), ch. 28 (relations).

<!-- bucket 8 -->

## §7

- "Both `build` and `serve` are one render path: ... (Verified: refactoring build ...)" — criterion 4: build verification ledger
- "Templates parse once to ASTs per run" — criterion 4: implementation detail
- Full description of RCU cell implementation details including keepcalm and SharedMut mechanics — criterion 4: implementation narrative
- "The snapshot lives in a `keepcalm` RCU cell..." full section — criterion 4: detailed build story
- "Those are the §2 upgrades, not yet built" — OUTLINE ch. 2 covers this; redundant
- Full description of `grackle query` including "REPL/CLI over the live DB" examples — criterion 4: tool narration
- "Doubles as the migration validator..." description — criterion 4: secondary purpose narration
- Full `grackle urls` description including derived assets q12 waiver story — criterion 4: waiver narrative about thumbnail scheme
- "The reference is any directory..." sentence about rsynced trees — criterion 4: dated usage pattern
- "Bodies only — chrome was never in that measurement (§5a)" — criterion 4: prior decision narrative
- "and the URL set is `urls`, above" — criterion 2: changes made; prior name was different

## §7a

- "grackle has been developed against exactly one corpus, and §9b shows the cost: `"blog"` hardcodes, view-name policy, and a phase-1 gate survive" — criterion 4: historical development narrative
- "The design already knows this argument — a boundary with a single implementation is untestable, which is why `light` exists (§5a) and why the null theme runs as a falsifier (§5e)" — criterion 3: settled design conclusion
- "A second site is the same move one level up: the falsifier for site-independence" — criterion 4: design narration (principle already stated in heading)
- "self-contained (own `grackle.toml`, own theme, own `.slots/`, own `_cache/`), invisible to the main corpus (the `grackle/**` exclude already covers it), built and served like any site:" — criterion 4: implementation detail
- Code block showing CLI invocation — criterion 4: build narration
- "It is deliberately a **kitchen sink**: each section exists to force a parked feature, in parallel rather than in sequence." — criterion 4: design principle narration (preserved in compressed form)
- Table mapping sections to features with § references and status — criterion 4: feature survey results. **Carried metrics into graveyard:**
  - Photos section forces object views, dimension facts, CSS-columns masonry
  - Manual section forces section trees, .section markers
  - Long posts force page outlines
  - Recipes section forces .schema.toml validation, per-row themes
  - Books section forces tree views, card/card_list kinds, hero, cross-table embedding
  - Second theme section forces partial themes, shell + CSS on canonical
- "Two rules keep it honest:" section statement — preserved in compressed form
- Full first rule expanded explanation — criterion 4: design narrative
- "which is the whole point is that its needs contradict the main site's assumptions. Day one already produced two contradictions on schedule: the posts collection must be *named* `blog` (the phase-1 gate in `views.rs`, §9b's accepted asymmetry, now with a corpus that objects), and a site without a theme directory should be the null theme by §5e's own words ("needs no directory at all") — the example sidesteps it by shipping a real minimal theme, but the gap is now demonstrable." — criterion 4: discovery narrative with historical context
- "2. **It has no byte oracle, on purpose.** The main site is verified against Jekyll; the example is verified by the engine's own invariants (load-time constraints, the null-theme completeness falsifier, route collision checks) — which is exactly the discipline a *new* grackle site would live under, tested for the first time." — criterion 2: historical comparison with Jekyll (now superseded); criterion 4: verification strategy narrative

## §7b

- "Method: 12 parallel survey agents, each auditing 3 sites against a compact model card — personal/systems/dev blogs, longform, linkblogs, food sites, portfolios, docs, digital gardens, unusual-static, magazines/podcasts." — criterion 4: survey methodology detail
- "35/36 fetched (rachelbythebay blocks this egress; judged from known structure). **90 reported misses: 14 structural, 33 moderate, 43 minor.**" — criterion 4: survey quantification; **carried metrics: 90 total misses (14 structural, 33 moderate, 43 minor)**
- "Raw reports are in the session archive; what follows is the synthesis." — criterion 4: archive reference
- Full "The headline: the core model holds" section with blog examples and "false" miss explanations — criterion 4: verification narrative with discovery story
- "Two reported misses were **false** — full-body listings (jvns TIL, seths.blog) are exactly §6d's "no summary field in the chain = rows ship whole", and prev/next navigation is the earlier/later relations axes — which says the *model card under-communicated*, not that the model missed." — criterion 4: false positive explanation narrative
- "(One true triviality fell out: matklad's per-post "fix typo" GitHub link wants the row's repo-relative source path as a document fact — storage is literally git, the fact is free.)" — criterion 4: discovery story
- "The survey's job was to generate questions, not to track them." — criterion 4: purpose statement
- "**§11 owns their status; this table owns the evidence** — which real sites drove each gap, which is the one thing that never goes stale." — criterion 4: design intent narration
- "(An earlier version of this section restated each design and its state, and had gone stale on two of them within the month: exactly the shadow-copy disease §9b names.)" — criterion 2: historical versioning narrative
- "Two of these resolve without becoming questions. The *interactive-widget* half of ciechanowski (stateful WebGL islands as the site's identity) stays honestly out: raw HTML passthrough plus per-row assets carries the delivery, and the engine never models the widget." — criterion 4: design scope narrative
- "And **external/live data** — trending ranks, HN counts, live solar charge — is not expressible from a git tree; the honest answer is an ETL that *writes* git-tracked data before the build, after which `order_by` works on it normally. Kottke's "vintage post today" is the benign case: a date-seeded deterministic pick is fine for a daily build. The model's answer is "commit the data"." — criterion 4: design decision narrative
- "The single biggest real-world cluster — **memberships, paywalls, comments, ratings** (waitbutwhy's store/forum, craigmod's and atp.fm's memberships, 404media's gated bodies, every recipe site's reviews) —" part of sizing — **carried metrics: memberships/paywalls/comments/ratings is largest cluster**; criterion 4: survey result narrative

## §7c

- "An audit of all 313 tests asked which of them were testing *a site* while pretending to test a function." — criterion 4: audit narration (belongs in §7d, moved there)
- "Its own `index.html`, `debug.css`, `debug.js` and a `site.json`. Serve-only by construction — a build emits none of it, and the prefix is a **closed namespace**, so a miss inside it 404s rather than falling through to a site page that would otherwise shadow the tool." — criterion 4: implementation detail
- "**The payload is deliberately not `grackle export`.** The export is the database as the database sees it; this is the database as someone diagnosing it needs to see it, and the two differ in exactly two ways. It carries what the export skips — route `members` and the row flags are `#[serde(skip)]` there, and they are precisely what answers *what picks this up* and *why is this missing*. And it resolves members to **URLs rather than indices**: an index only means something beside the table it indexes, so emitting URLs lets the client join everything to everything without knowing which table a view ranges over. The payload rides in the serve snapshot, rebuilt with the site, so it can never describe a database the served pages didn't come from." — criterion 4: design rationale narrative
- "**Four lenses, and the cardinality picks the form.** Measured on the main corpus: 838 of 1575 routes are objects, posts are 1:1 with theirs, and **7 views produce 183 routes**." — criterion 4: measurement ledger; **carried metrics: 838/1575 object routes, 7 views → 183 routes**
- "So trees and tables for the big homogeneous sets, and no node-graph anywhere — 1575 routes as a force-directed hairball would teach nothing, and 3 tables whose relationships are all derived make a poor ER diagram." — criterion 4: design decision narrative
- Full tree, rows, views descriptions with deep details — OUTLINE ch. 31 covers this
- "The centrepiece is the **provenance strip**: source → route → the views that picked it up. A generic database viewer structurally cannot show it, because here the row and the URL are not the same object — a claimed row has no route (§5h), a translated row has two (§6f), and a view route has 66 members and no row at all." — criterion 4: design explanation narration
- "Between the two trees is a **gutter** that draws the current selection's correspondence: an arrowhead into each side and a line joining them, one per pair." — criterion 4: UI feature detail
- "Two states make it useful rather than decorative — a target scrolled out of its pane turns its head **up or down** (the arrow stops meaning "over there" and starts meaning "scroll"), and a target inside a *collapsed* branch has no element at all, so the connector points at the nearest rendered ancestor and goes dashed: it names the folder to open instead of pointing at nothing." — criterion 4: interaction design detail
- "Two things it taught immediately. **A node can be both a route and a parent** — `/blog/` is `blog_index`'s own route *and* the ancestor of every archive beneath it, and the first cut conflated "has children" with "is a folder", which made every landing impossible to select. The twisty owns expansion, the label owns selection. And **route order is lexical** (`sort_by(url)`, for determinism), which is right for the sitemap and wrong for reading: `/blog/page/10/` sorts before `/blog/page/2/`. The client owns display order with a numeric-aware comparator; the engine keeps its determinism." — criterion 4: discovery and iteration narrative

## §7d

- "An audit of all 313 tests asked which of them were testing *a site* while pretending to test a function. Roughly seventeen were, and the tell was uniform: **they hand-built what the loader produces.** `views.rs` wired `object_ix` itself; `posts_order_tests` fabricated `Row`s with the `locale` field a comment warned was load-bearing; `trails.rs` faked a route's pagination stamp (`root.key = Some("page 1")`); `outline.rs` wrote seven `Row{…}` literals filling fifteen irrelevant fields each. None of them can catch a loader bug, because none of them run the loader." — criterion 4: audit narration
- "`crates/grackle/tests/fixtures/<name>/` is `site/` (a real `grackle.toml` plus content) and either `out/` (the expected rendered tree, in git) or `expected-error` (a substring the load must fail with). One `#[test]` walks them all and collects every problem before panicking, so one broken fixture cannot hide the rest. `UPDATE_EXPECT=1` re-blesses." — criterion 4: infrastructure detail (framework setup)
- "**The line, so this does not eat the suite**: if the subject is a *site*, it belongs in a fixture; if the subject is a *function*, it does not. All thirteen `binder.rs` tests isolate one hole-algebra rule against a one-line fragment and are clearer that way. `parts.rs` asserts on typed `PartMap` values, which HTML diffing would make *less* precise. Everything in `crates/db`, `crates/model` and `crates/search-core` tests a pure expression language and pure data structures. The audit found no candidates there at all, which is the boundary holding." — criterion 4: audit finding boundary rule (principle preserved, implementation detail removed)
- "Three things the build settled:" — criterion 4: build narration
- Full section on "crates/grackle grew a `lib.rs`" and its rationale — criterion 4: implementation history
- "Exactly one value needs normalizing: the feed's own `<updated>`, which is wall-clock. Per-entry `<updated>` and the sitemap's `<lastmod>` are derived from a row's `date` and are stable, so blanking them would hide a real regression — the harness rewrites the first `<updated>` only." — criterion 4: test normalization methodology
- "A fixture's `site/` must stay the bytes its author wrote. Rendering creates `_cache/` beside the content; the harness removes it, because a suite that dirties `git status` on every run trains people to ignore it." — criterion 4: cleanup strategy narrative
- "**Nine tests converted, six fixtures.** `views.rs`'s object views and post ordering, `trails.rs`'s crumb climb and declared trail, `outline.rs`'s section tree — every one of them replaced by a directory whose contents are what a user would write. Two of the conversions merged several tests into one fixture, which is the shape telling the truth: three `order_by` variants are three routes over one corpus, not three sites." — criterion 4: conversion ledger; **carried metrics: 9 tests converted, 6 fixtures**
- "One thing the fixtures found immediately. `crumb-trails` renders a post from a collection that declares NO `trail` and it still gets a year crumb, because every `kind = "posts"` collection feeds one table (§4) and the archive claims it. The unit test it replaced could not have seen that: it asserted on `post_trail` for one hand-built row. The fixture pins the behaviour without endorsing it." — criterion 5: discovery finding already preserved (compressed narration)
- "**Not everything the audit proposed was right.** It suggested moving the `css_pass`/`embed` scratch directories to `CARGO_TARGET_TMPDIR`; Cargo defines that only for integration tests, so a unit test cannot have it. The existing `who`-suffixed system temp dir is the correct answer for tests that run in parallel threads, and it stays — with a comment saying why, so the next person does not re-propose it." — criterion 4: decision narrative (can be deleted as post-hoc rationale)
- "Two more fixtures ship with the harness itself, each mutation-checked: `minimal-blog` (an empty-ish config, a page, two posts, one of them `noindex:`) and `undeclared-field` (the §4e governance rule, holding the line that let a dead `hide_sidebar:` survive grack.com's whole port). Breaking `og:type` in the base config names the file, the line, and both values; changing an error message names the expected substring and what was actually raised." — criterion 4: test suite addition narrative

## §8

- "| Area | Why | Plan |" table headers preserved, Plan column entries compressed — criterion 4: table plan narration details:
  - Code highlighting: Long narrative about Rouge→comrak pipeline choices — criterion 4: build history
  - kramdown edge syntax: Full story about markdown="1" finding with post-specific examples — criterion 4: discovery narrative
  - Related posts: Full explanation of LSI issues and embeddings replacement — criterion 4: prior approach narrative
  - Feed body HTML: Full explanation of regex port and byte verification — criterion 4: implementation narrative
  
- Paragraph "Heading anchors: kept, deliberately" date stamp and detailed explanation of discovery — criterion 4: narration of when/how discovered (preserved principle, compressed story)

## §8a

- "The kramdown→comrak gap was the one risk that could sink the port." — criterion 4: project risk narrative
- "**Method.** Posts that are both liquid-free *and* untouched since the reference build (a naive comparison would have measured content drift and blamed comrak). comrak configured to kramdown's defaults: `auto_ids`, smartypants, tables, strikethrough, footnotes, description lists, raw HTML passthrough. Normalisation folds only invisible differences: whitespace, entity spellings, self-closing style. Of 230 posts: 20 identical, 187 equivalent, 23 differ." — criterion 4: detailed measurement methodology; **carried metrics: 230 posts (20 identical, 187 equivalent, 23 differ)**
- "The residue is `10 inline/prose · 5 list · 4 link · 3 table · 1 code block`, and spot-checking says every one is **parse**-stage:" — criterion 4: categorization narration
- Extended examples of three parse-stage differences — criterion 4: evidence narrative (preserved findings, compressed detail)
- "Zero heading, zero footnote, zero image diffs — the four node types we have opinions about are not where we lose." — criterion 4: negative finding narration
- "The 90/92% ceiling is a *parser* ceiling, which is what decides the renderer question (§9a): if we ever chase it, we fork comrak's parser, not its formatter." — criterion 2: links to architectural decision in §9a (cross-reference preserved)
- Full "The reference build lied by 17 points" section header and narration — criterion 4: measurement history story
- Full story about jekyll commit that turned rouge on, the 72.6%/90.0% discrepancy, "our output never changed. Only the yardstick did" — criterion 4: major discovery narrative explaining how measurement can be invalidated; **carried metrics: original 90.7% → 72.6% with rouge on → back to 90.0%**
- "**The rules this buys:**" → Four numbered rules — criterion 5: rules preserved, discovery examples removed
- "### Retiring the body oracle *(Matt's call, 2026-07-21)*" — criterion 4: header with decision already preserved, narrative trimmed
- "The body diff is **no longer a cutover gate**. `grackle urls` gates the URL set; everything else is verified by eye. Three things drove it, and they compound:" — criterion 4: decision preserved, rationale trimmed
- "1. **The reference is a wasting asset.** 48 of 327 posts have been edited since it was built, and the edits are deliberate migration work — `{% post_url %}` rewritten file-relative, raw URLs converted for strict links, callouts rewritten as widgets. §8a's method filters to posts "untouched since the reference build", so the comparable set shrinks every time the corpus moves toward grackle. Two posts now carry `{% callout %}`, which Jekyll cannot render at all, so the reference cannot be fully regenerated even with the `git stash` dance (q22)." — criterion 4: reference state narration; **carried metrics: 48/327 posts edited**
- "2. **The harness hides real differences.** `diff::normalize` calls `strip_comrak_anchors`, so the 90% figure is computed with comrak's 226 injected heading anchors removed. They ship; the reference has none; the measurement structurally cannot see them. That normalizer was right for the question it was written to answer (do the slug algorithms agree?) and wrong as a parity gate." — criterion 4: measurement design narrative
- "3. **The remaining gap is a parser ceiling**, ~92%, and §9a already decided we will not fork comrak's parser to chase it." — criterion 2: decision reference (preserved)
- "What survives: `diff` stays as an *investigative* tool, and the 97-post blind spot below stays worth knowing when reading any number it prints. What ends is treating its matrix as the thing that says "safe to publish"." — criterion 4: outcome narration; reference to §8a's next section
- Full "### The 97-post blind spot *(open — q21)*" section and explanation — criterion 4: open issue narrative that links to other questions; context preserved via q21 reference
- "Related and still true: **`_site-prod` can no longer be regenerated** (§5c) — `{% view %}` is not Liquid, so Jekyll fails the whole build and refreshing the reference needs `git stash push index.html` first (q22). Losing the ability to refresh the reference is exactly the capability that caught the 17-point lie." — criterion 4: explanation of interdependency in measurement infrastructure
- "### Two SCSS findings worth keeping" — criterion 5: heading and findings both preserved, no removal

---

# Summary of metrics carried to graveyard

**Survey/audit counts:**
- 7b: 35/36 sites fetched; 90 total misses (14 structural, 33 moderate, 43 minor) — criterion 4: survey statistics (archived for re-running baseline)
- 7c: 838 of 1575 routes are objects; 7 views produce 183 routes — criterion 4: measurement ledger (corebase metrics)
- 7d: 9 tests converted to 6 fixtures — criterion 4: conversion ledger (infrastructure metrics)
- 8a: 230 posts total (20 identical, 187 equivalent, 23 differ); 48 of 327 corpus posts edited; original 90.7% → 72.6% with rouge on → back to 90.0% — criterion 4: measurement statistics (method re-runnable)

<!-- bucket 9 -->

## §9a

- Per-crate table with versions and health notes — rotted after axum→hyper replacement — OUTLINE ch. 2
- Detailed explanation: why not write own AST→HTML renderer; fidelity argument, control argument, tripwire — criterion 4: build narration
- Code-block adapter analysis (CodefenceRendererAdapter, SyntaxHighlighterAdapter measurements) — criterion 4: build narration
- Detailed notes on inline code needs and Rouge quote escaping — criterion 4: build narration
- Explanation of how §6d's blocks change lol_html scope — criterion 4: build narration

## §9b

- Round 2 audit narration: "Verdict: boundaries held under load"; detailed story of landings/records/links landing in one owner each, example config shrinking — criterion 4: audit narration result (kept the lessons: "The landing pass re-shapes rows", "build.rs is the gravity well", "Semantic drift in main config")
- Round 3 audit narration: "The engine became a workspace"; story of crate split revealing boundaries — criterion 4: audit narration result (kept the four lessons)
- "Since, and what is left" introductory sentence "Three merges landed, each removing a distinction that was never real" — criterion 3: settled conclusion, kept remainder

## §10

- Phase 0: FsStore + posts table + `query` — ✅ 327 rows, URL set matches Jekyll sitemap exactly; loads ~3.5ms warm — criterion 5: completed item
- Phase 1: route mapping, `export`, `routes` — ✅ ~1579 routes, every 556 Jekyll sitemap URL routed, 0 missing — criterion 5: completed item
- Phase 2a: markdown-gap spike + `diff` — ✅ 90.0% against reference, 92.2% with smartypants — criterion 5: completed item
- Phase 2b: render pipeline end to end — ✅ 327 posts + listings + 40 pages + 1025 assets + 260 thumbnails + feed + sitemap in ~0.4s — criterion 5: completed item
- Phase 3: feed, sitemap, scss, thumbnails, passthrough — ✅ entry sets byte-identical; 25.3 MB sources → 9.0 MB shipped; linklint retired 2026-07-21 — criterion 5: completed item
- Phase 4: `serve` — 🟡 v1 — raw hyper, resident render map, no output dir; rebuilds ~0.3s; deferred: incremental invalidation, SSE — criterion 5: completed item
- Phase 6: §5e presentation synthesis — ✅ complete — part maps, binder, real theme directory, canonical fallback — criterion 5: completed item
- Phase 8: §6b embeddings + search — ✅ LSI and Swiftype retired — criterion 5: completed item

## §11

- q1 explanation: "Rides with serve v2 (the phase-4 deferral)" — criterion 4: narrative context removed, question kept
- q2 explanation: full sentence context — criterion 4: narrative
- q4 explanation: detailed story of 4 of 6 posts liquid-skipped — criterion 4: build measurement narrative
- q6 full explanation — criterion 4: narrative removed, decision kept
- q11 full explanation "Do iframes need any sandbox/loading attributes injected by the same pass, or is passthrough correct?" — criterion 4: narrative
- q13 full explanation with cache key mechanics — criterion 4: narrative (kept core question)
- q14 full explanation — criterion 4: narrative (kept core decision point)
- q21 full explanation with numbers "97 of 327 posts are excluded, many falsely (`{{ github.event.issue.number }}` in code samples is GitHub Actions, not Liquid). 30% of the corpus is unmeasured and the 90% is over an unrepresentative 230." — criterion 4: build measurement narrative
- q22 detailed story "Jekyll fails the whole build; refreshing needs `git stash push index.html` first. Losing the ability to refresh the reference is exactly the capability that caught the 17-point lie." — criterion 4: build ledger narrative
- q23 full explanation — criterion 4: narrative (kept core remainder)
- q25 full explanation with IAL context — criterion 4: narrative (kept core question)
- q26 full explanation — criterion 4: narrative (kept core work)
- q28 full cautionary context — criterion 4: narrative
- q30 full detailed explanation about namespace collision — criterion 4: narrative (kept core decision axes)
- q33 opening preamble "The serialization half settled as shells (§5g). Settled since: (a)..." — criterion 3: settled context, kept remainder
- q34 full explanation with example — criterion 4: narrative (kept core work)
- q37 full explanation with decision table structure — criterion 4: narrative (kept core pending items)
- q38 full explanation — criterion 4: narrative (kept core work)
- q39 full explanation — criterion 4: narrative (kept core work)
- q40 full explanation — criterion 4: narrative (kept core work)
- q42 full explanation with pattern-space concepts — criterion 4: narrative (kept core work)
- q43 full explanation — criterion 4: narrative (kept core work)
- q47 full explanation including "French reader landing on `/fr/blog/`" story — criterion 4: narrative (kept core problem)
- q48 full explanation with test criterion "The test it must pass: a type is real only if something other than the renderer consumes it" — criterion 4: narrative (kept core rule)
- q49 opening "measured 2026-07-19" and detailed precedence explanation — criterion 4: narrative (kept core work), some measurement detail removed
- q50 opening "Matt's case" and detailed explanation of nested HTML problem — criterion 4: narrative (kept core operations and open questions)
- q51 opening "the table merge is built; this is the remainder" and "Matt's shape" preambles — criterion 3: settled context (kept remainder)
- q53 opening "Matt, 2026-07-20; the locale half built 2026-07-25" date context — criterion 4: dated ledger (kept the mechanical definitions and open questions)
