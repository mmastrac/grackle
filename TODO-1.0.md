# grackle 1.0 — the list

Checkbox form. Anything moved out of `DESIGN.md`, `themes/DESIGN.md` or
`themes/README.md` during the compression pass landed here rather than being
lost; `GRAVEYARD.md` holds the compressed prose those files gave up.

**What this file is not**: a mirror of `DESIGN.md` §11. An earlier draft copied
all thirty open questions in, and that is exactly how two settled ones (q32,
q46) came back as open and two invented q numbers appeared. §11 is the authority
for open *design* questions; this file is the release checklist. Where a §11
question has 1.0 exposure it gets one line here and a pointer, never a copy.

Ordering is rough priority within each group, not across groups. Items marked
*(from doc prose)* were harvested from documentation, not read out of the code —
check them before acting; three such items turned out to be already built.

---

## Defects

- [ ] **Audit the compressed docs for claims whose correction was deleted** —
      the one failure mode of the compression pass. These documents state a
      claim in one section and correct it in another, and the correction usually
      lives in the dated ledger prose the criteria classify as build narration.
      Removing the narration can leave the superseded claim standing alone,
      looking authoritative, with the only thing that contradicted it gone — the
      doc comes out *more* wrong than it went in. Found twice: `themes/DESIGN.md`
      §7 said typography was opt-in while its correction (the `_type.scss` split)
      sat in the cut ledger; §4e's "a marker cannot set a declared field" claim
      survived one pass past the code that fixed it. Both now fixed. The check is
      mechanical: for each surviving rule, confirm it against the code, not the
      document. `git show dc96d5d^:grackle/DESIGN.md` and its `themes/` sibling
      hold the pre-compression text.

- [ ] **`from = "*"` on a site-declared route** — a routed row is routed whatever
      its flags say, so every star route must restate `!draft && !hidden`.
      `View::inherited` already records whose route a view is, so the validator
      can refuse a site-declared star route whose `where` omits the flags while
      the base's own passes. (§5c)

## Tooling — the pipeline's dark stages

The pipeline is `file → row → query → doc model → parts → slots → CSS → URL`.
Stages 1–2 have `query explain`; 4–6 have nothing, and the config merge added a
layer with no inspector at all.

- [ ] **`grackle config --effective`** — print the merged config with provenance
      per key (base vs site file). This is what makes `extends` inheritance
      rather than magic; `examples/raw` is the stopgap. (§4d, named there as
      "should ship before 1.0")
- [ ] **`grackle explain <url> --parts`** — the part map, which producer filled
      each part, which fragment placed it, and **which parts nothing placed**.
      The last is a partial answer to q50's forgotten-vs-deliberate hole that
      needs no settlement first.
- [ ] **Top-level `grackle explain`** — `DESIGN.md` §0 and the manual (ch. 2)
      both teach `grackle explain <url>` as *the* debugging tool; the command is
      `grackle query explain`. Alias it.

## The theme ladder and distribution

Rung 0's config half landed (`[site] theme`). Everything below is
`themes/DESIGN.md` §3–§5, specced and unbuilt.

- [x] **`.style.scss` — built, at the root.** Rung 1 is real: a site-owned sheet
      compiled into `@layer overlay` above every theme's CSS, appended to every
      theme's stylesheet so an override survives a theme *switch*. Unscoped, so
      it may declare `:root` properties — which is the recolour case. `serve`
      watches it. (§5b, themes/DESIGN.md §2)
- [ ] **Positional `.style.scss`** — §5b's other half: a file per subtree,
      scoped to `[data-scope~="dir"]`. Needs every rendered row to emit its scope
      chain, which nothing does yet, and carries the `:root`-in-a-scoped-block
      constraint below. Separable from the root file, which is the rung the
      ladder actually promised. (§5b)
- [ ] **`theme.toml` and `extends` chains** — rung 3. `theme.rs` never reads
      `theme.toml`, so theme-level inheritance is entirely unbuilt while the
      config-level `extends` it was modelled on has shipped. Fragment union
      (child wins), CSS concatenation, cycle/unknown-parent errors naming the
      chain. (themes/DESIGN.md §3)
- [ ] **Nested `@layer` down the chain** — `@layer theme.root, theme.mid,
      theme.leaf` so a child always outranks its parent by layer and
      `revert-layer` walks one step at a time. Plain concatenation recreates the
      specificity war `@layer` was introduced to settle. Build it this way from
      the first commit: the failure it prevents is silent and shows up only in
      someone else's theme.
- [ ] **`theme derive <name>`** — the load-bearing distribution command. Because
      inheritance is file shadowing, "the files you edited" already are a valid
      derived theme, so this is nearly `mv` plus two lines of TOML. It converts
      the classic SSG failure mode (hacked vendor theme, updates now scary) into
      rung 3 mechanically, which is what makes rung 4 safe to allow.
- [ ] **`theme add <url>[@ref]`** — shallow-fetch to cache, copy in, write lock;
      follow `extends = { git = … }` recursively; refuse on `contract` mismatch
      naming both versions.
- [ ] **`theme update [name]`** — replace wholesale if every local hash matches
      the lock; else refuse, list the edited files, point at `derive`.
- [ ] **`theme check [name]`** — validate fragments + CSS against the engine
      schemas standalone; lint token names against the contract on the
      **resolved chain**, never the leaf alone. Catches the three back-tested
      edges: split `kind`/`kind--variant` pairs, identity slots a child shell
      dropped, vars no ancestor defines.
- [ ] **`theme list` / `theme new` / `theme try`**, and **`themes/.lock.toml`** —
      chain and lock status; rung-3 scaffold; cache-only install loaded last;
      provenance and per-file hashes at install.
- [ ] **`?theme=name[:tokens]` dev override** — render any page through any
      loaded theme, gated to the dev profile. Both the experimentation loop and
      the standing test of guarantee 2.
- [ ] **Child themes invalidate on ancestor edits** — the chain is known at load;
      invalidate by every chain member's key.
- [ ] **`head.html` theme fragment** — appended after the computed facts, for
      themes wanting their own head content (fonts). *(from doc prose)* (§5g)
- [ ] **Per-theme head-fact selection** — the engine renders all head facts. §4e
      moved the head into config (`[html.head.*]`), so the *shape* of this
      changed: it is now "can a theme override a config table", not "which facts
      does `theme.toml` list". Re-decide before building. (§0, §5a, §4e)
- [ ] **Forced-colors mode** — claimed from spec for vanilla and the gallery,
      never tested.

## Policy

- [ ] **Base-theme breaking-change policy** — the base *config* has one ("base
      changes that mint URLs are breaking"). The base theme has the same
      exposure and no policy, and the favicon fix was the forcing case.
      (§4d honest edges)

## Composition

- [x] **Grouping composes over every base** — done. `build_object_view` was a
      second materializer that never grew `group_by` or `paginate`; the two
      merged into one `build_view` taking a `Base { schema, membership, parsed }`,
      so objects group, paginate and subdivide like anything else
      (`group_by = "ext"` puts the jpegs at one route and the pngs at another).
      `paginate` under `group_by` was silently ignored and now paginates inside
      each partition. Byte-identical across all five sites. (§5c)
- [x] **`from` scopes to what it names** — done. Membership is one clause for
      every base (*the row's collection is one of these*), and §4's "several
      collections, one table" is now written out: `from = ["posts", "drafts"]`.
      A union may name only collections and they must share a kind; both
      violations are load errors. grack.com needed the one-line union, and the
      regression that proved it was invisible in the default projection —
      only `--profile drafts` showed 561 pages become 560. (§5c)
- [x] **A view may declare its own theme** — done. A listing took the theme its
      members agreed on and a claimed landing took its claimed row's, so the look
      was a property of the content and one query could not wear two. `[routes.x]
      theme = "ledger:dark"` makes it a property of the route; view beats member
      unanimity beats `[site] theme`, and an unknown name is a load error. (§5a)
- [x] **The axis prerequisite** — done, and inert. `Route.row` states the
      row→route relation; post rendering is route-driven like the Page arm (it
      was the fourth split in the render path, after q51's row type, `passes/`'s
      view walk and §5c's materializer); per-row maps key on the row rather than
      its URL. Enforced by a new §4 constraint — **one row, one route, N only
      along an axis** — the dual of the route-collision check, and unstatable
      before `Route.row` existed. Byte-identical on five sites, both profiles.
- [x] **Axes** — built. `[axes.<name>]` with `values`/`field`/`url`/`match`:
      one row, N routes, keyed by a value, the sole exception to §4's one-row-one-route
      constraint. The first value is canonical and keeps the row's own URL (the
      shape the default locale has); alternates are templated; every member
      canonicalizes to the document; `*` views see canonical members only.
      `field = "theme"` and `field = "shell"` are both wired — the second is
      q44's md twin, which is the multiplicativeness the idea was claiming. (q53)
- [x] **Link to an axis member** — done: `page.md?theme=ledger`. A link resolves
      to a row and a row answers with its canonical URL, so a member had no
      spelling; the merged gallery's first build failed on exactly that. Same
      strictness as every other link — undeclared value and not-on-that-axis are
      both load errors, and a non-axis `?k=v` is still a literal suffix. (q53)
- [ ] **Axis edges, named in q53** — two left, neither blocking: **two axes over
      one row collide** rather than composing (`/fr/ledger/…` is unbuilt, though
      each would now spend its own segment), and **axis members emit no
      `rel="alternate"`** because `Head.alternates` is hreflang-shaped (same item
      as variable-length head entries). The third — a `field` no render path
      consults — is closed: a member stamps `data-axis-<name>` on the root, so an
      axis declared for a purely presentational difference works with no engine
      wiring. (q53)
- [x] **A view may be materialized across an axis** — done (q53 step 1).
      `[routes.x] axis = "theme"` with a `{theme}` segment in the path; an
      unspent axis is a load error. A landing on an axis claims one row (the
      constraint is per-view), which retired per-group `content` before it was
      built. `view:name?axis=value` links a member.
- [x] **Axis URL shape in the collection rule** (q53 step 2) — done. A route
      template spends the axis with a `{theme}` segment and the rule that spends
      it opts its rows in, so `[axes.*] url` and `[axes.*] match` both retired
      into the rule that was already deciding both halves. Canonical-bare went
      with them: every member wears its segment, and canonical is purely the
      declaration of which one `rel="canonical"` names. (q53)
- [x] **Unify the materializer into a product over dimensions** (q53 step 3) —
      done. Grouped, paginated and single were three branches with three locale
      loops and a duplicated pagination loop; they are one partition/slice/emit
      over axis × locale × group × page. Byte-identical everywhere. (q53)

- [ ] **Per-group `content`** — the blocker the composition work actually hit.
      `theme-preview`'s route families each claim a per-theme landing row
      (`content = "vanilla/notes/index.md"`), so grouping can mint the six URLs
      but cannot give each its own words. Wants `{key}` interpolation in
      `content`, using the same `template::render` that `path` and `title`
      already use — and §4d's distinction kept intact: an explicit templated
      `content` stays a promise (missing file = error), `default_content` stays
      the offer. (§5c, §5h)
- [ ] **The same N views over N subtrees** — `theme-preview/` is 341 lines and
      33 `[sets]`/`[routes]` tables for six structurally identical subtrees. Two
      different problems wear one shape: notes and shelf are a real **partition**
      (six disjoint row sets — `group_by` fits exactly, and now works), while the
      six wall routes are **one** row set at six URLs, which no grouping
      primitive can express because nothing is being partitioned. Worth revisiting
      only if `?theme=` (above) doesn't retire the harness's shape first. (§4d, §5c)

## Unbuilt, and carrying no q number

Everything here is specced somewhere and owned by nobody. *(all from doc prose)*

- [ ] **Authored `.rewrite.toml` rules** — the full rule table with selectors and
      wrapping. Stage A shipped the narrow HTML-source-link rewrite; the general
      form waits for a second consumer. (§6d)
- [ ] **Variable-length head entries** — `rel="alternate" hreflang` × n cannot
      live in a name→string map, and `sizes`/`type` on link entries need the same
      shape. Subsumes the "link table form" item: the favicon half of that
      motivation is gone (`site.icon` restored it). (§4e)
- [ ] **Expression-form derivers** — v1's `toc` uses a hardcoded h2–h3 window;
      the §5f form would be `toc = outline(content, {"max_depth": 3})`. Same
      shape as the `hero`/`lede` derivers §11 parks under q23. (§6e)
- [ ] **Parenthesised expressions in rank** — `(a + b) > c` is valid CEL but
      unsupported; the error suggests lifting it into a rank term. **Not q13**
      (that is embedding model pinning) — this carries no number. (§6g)
- [ ] **Localized group keys** — enum records extend to group *keys*, not just
      value domains. **Not q40** (that is structured record fields); §6f calls
      this "q40-adjacent" and an earlier harvest dropped the qualifier. (§6f)
- [ ] **The rest of i18n's locale-free surface** — `month_name` (computed at
      route build), `pretty_date` (hardcoded), the search overlay's client-side
      strings, and `site.title` (not a `LocalizedStr`). (§6f)
- [ ] **Embedded views follow their embedding page's locale** — specced,
      pending. (§6f, §5h)
- [ ] **Orphaned translation warning** — `index.fr.md` with no French rows should
      warn. (§5h)
- [ ] **Mode-B prose is structurally excluded from the search index** — a
      landing's content route never reaches the searchable set. (§5h)
- [ ] **Explicit `parent =`** — for the edge where URL nesting lies about parent
      structure. Unneeded so far; named so the absence is deliberate. (§5h)
- [ ] **Scoped SCSS cannot declare `:root` custom properties** — they would be
      scoped to the selector and silently not apply. Needs a documented
      constraint or a load-time error, because the failure is invisible. Rides
      with the `.style.scss` decision. (§5b)

## §11 questions with 1.0 exposure

One line each; `DESIGN.md` §11 carries the design. Everything else in §11 is a
design question without a release consequence and is not listed here.

- [ ] **q47 — listing views render no language switcher.** A French reader who
      lands on `/fr/blog/` has no way back. The most user-visible of these.
- [ ] **q26 — body-image dimensions.** Post bodies still ship without them;
      layout shift site-wide until the §6d rewrite stage reaches `{% image %}`.
- [ ] **q28 — redirects for restructured URL trees.** No mechanism, and the
      migration story is a headline 1.0 feature.
- [ ] **q50 / q45 — the forgotten-hole warning.** A variant fragment missing a
      hole drops that part silently, and a deliberate omission is byte-identical
      to a forgotten one. `explain --parts` above is the partial answer that
      needs no settlement.
- [ ] **q34 — three "not content" lists.** `slots.rs` and `serve.rs` carry
      private skip lists that can drift from `exclude`. Silent when it happens.
- [ ] **q14 — `<style>` auto-scoping default.** A decision, not a build; cheap to
      settle and it blocks per-post CSS.

## Known gaps: document, don't fix

Real, understood, and cheaper to write down than to close. Each needs a line in
the manual (ch. 35 or the relevant chapter), not a commit. *(from doc prose)*

- [ ] **Same-day neighbours** — `earlier`/`later` compare day-granular with
      strict `<`, so two posts on one day are neither's neighbour. Zero pairs in
      the corpus today. (§6g)
- [ ] **Cross-kind relation pools** — a pool spanning kinds may compare only
      fields every candidate carries; the rest is a load error where checkable.
      (§6g)
- [ ] **A script shell's source is a content file** — it will be routed and
      published unless excluded. The example site shipped `shells/llms.py` this
      way. (§5g)
- [ ] **Silent variant degradation is the design** — a row-requested variant the
      theme lacks falls back quietly; a fragment's own `data-fragment=` override
      naming a missing fragment IS a load error. The contrast is the teachable
      part. (§5e)
- [ ] **Bare-name resolution is unexercised** — all 194 site invocations use
      paths, so §6a's bubble-and-bucket path has no production coverage, and §0's
      `burrs.jpg` tour example is specced rather than observed. (§6a)
- [ ] **Embedding text includes title and tags** — retitling re-embeds, so
      "Related changed" after a refactor does not indicate semantic drift. (§6b)
- [ ] **Footnote duplicate `id` collision** — theoretical: one post has
      footnotes, and it only bites if two such posts ever list together. (§6d)
- [ ] **The streaming rewriter cannot use `:first-of-type` or `:has()`** — verify
      the selector subset before relying on either. (§6d)
- [ ] **Raw-HTML bodies cannot distinguish engine-derived from authored URLs** —
      one asymmetry in the rewrite stage, scoped to unavoidable cases. (§6d)

## Surveys and audits worth re-running

Methods worth repeating. Several produced numbers now quoted in the docs that
nobody has re-measured since the base config, the flag move and the head fold.

- **Cross-site render parity** — grack.com, field-notes, minimal, raw,
  theme-preview, byte-identical under the base config. The measurement §4d rests
  on; drift since is unmeasured.
- **URL-set parity** (`grackle urls --against`) — the half that protects 20 years
  of inbound links, and the gate for any redirect strategy.
- **Minimal-site line count** — the number `examples/minimal` exists to carry.
  27 → **0** under the base config. Re-run whenever defaults change; a rise wants
  a reason.
- **The seams audit** (§9b) — do the crate boundaries still hold after the base
  config, the flag move and the head fold? (Three separately-harvested seams
  audits collapse to this one.)
- **The 36-site backtest** (§7b) — re-run against the current model to catch
  structural gaps the last survey could not see.
- **The markdown gap** (§8a) — with the 97-post blind spot unresolved, the 90%
  figure is over an unrepresentative 230.
- **The archetype audit** (§5e) — walk document/sidenotes, gallery, masonry,
  full-bleed, timeline, dense index against the current part schema.
- **The portability falsifier** — every row through every gallery theme and no
  theme, asserting each row keeps its name. In place; re-run per release.
- **Namespace collisions** — across `[collections]`, `[sets]`, `[routes]`,
  `[records]` after the base-config merge.
- **Inspector feature audit** — four lenses (tree, rows, views, diagnose);
  verify completeness per release.
- **Fixture audits** — mutation-checking coverage (only `minimal-blog` and
  `undeclared-field` are known-checked), and the fixture-to-function boundary
  (~17 tests once hand-built rows).
- **Body-image dimension census** — 442/468 (94.4%) at the time of measuring;
  track as content lands. Ties to q26.
- **Marker scan cost** — ~6ms with `.gitignore` pruning vs ~205ms without.
- **Object name collisions** — currently two (`screenshot5.png`/`screenshot6.png`).
- **Locale-parallel partition validation** — no locale should materialize an
  empty route set; untested against a real multi-language corpus.
- **Template-per-element rewrite cost** — profile before rolling injected
  templates out over 327 posts × many elements.
