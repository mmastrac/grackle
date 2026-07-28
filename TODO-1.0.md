# grackle 1.0 — the list

The release checklist. MERGE.md and IO.md's ledgers are closed; their
remaining Matt-only calls live here. `DESIGN.md` §11 is the authority for
open *design* questions — where a question has 1.0 exposure it gets one
line here and a pointer, never a copy. `GRAVEYARD.md` holds the compressed
prose the design docs gave up.

Ordering is rough priority within each group, not across groups. Items
marked *(from doc prose)* were harvested from documentation, not read out
of the code — check them before acting.

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

- [ ] **A site-declared fold over every output** *(absent `from` under a fold
      shell — IO.md I3)* — a routed row is routed whatever its flags say, so
      every such route must restate `!draft && !hidden`. `View::inherited`
      already records whose route a view is, so the validator can refuse a
      site-declared one whose `where` omits the flags while the base's own
      passes. (§5c)

## Tooling

- [ ] **`grackle explain <url> --parts`** — the part map, which producer filled
      each part, which fragment placed it, and **which parts nothing placed**.
      The last is a partial answer to q50's forgotten-vs-deliberate hole that
      needs no settlement first.

## The theme ladder and distribution

Rung 0 (`[site] theme`) and rung 1 (root `.style.scss`) are built.
Everything below is `themes/DESIGN.md` §3–§5, specced and unbuilt.

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
- [ ] **Subtheme token validation** — `theme: ledger:drak` stamps
      `data-subtheme="drak"` silently; tokens name nothing the engine knows.
      Needs `theme.toml` to declare them. *(MERGE.md §7 q13)*

## Policy

- [ ] **Base-theme breaking-change policy** — the base *config* has one ("base
      changes that mint URLs are breaking"). The base theme has the same
      exposure and no policy, and the favicon fix was the forcing case.
      (§4d honest edges)

## Unbuilt, and carrying no q number

Everything here is specced somewhere and owned by nobody. *(all from doc prose)*

- [ ] **Authored `.rewrite.toml` rules** — the full rule table with selectors and
      wrapping. Stage A shipped the narrow HTML-source-link rewrite; the general
      form waits for a second consumer. (§6d)
- [ ] **Variable-length head entries** — the `rel="alternate"` half is **built**
      (2026-07): `Head.alternates` is now a list of `Alternate { href, hreflang?,
      media_type? }`, which repeats `rel` and carries a second attribute, so
      hreflang × n and `type` both fit. What remains is the same shape for the
      DECLARED head (`[html.head.link]` with `sizes`/`type` per entry) — a
      name→string map still can't express those. Subsumes the "link table form"
      item: the favicon half of that motivation is gone (`site.icon` restored
      it). (§4e)
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
      settle and it blocks per-post CSS. *(MERGE.md §7 q4 is the layering half.)*

## IO leftovers (Matt's calls)

Unowned or deliberately not taken when IO closed. Priority call before build.

- [ ] **`robots_txt` emission** — fold shell over output facts; exact emission
      spec still open. *(IO.md §9 q3)*
- [ ] **`kind` / search config migration** — needs scope-membership expressibility
      on the output pool first, then Matt's migration decision. *(IO.md §3 / I13)*
- [ ] **Scope-membership expressibility on the output pool** — the column (or
      shell respelling of search) that unlocks deleting `kind`. *(IO.md I13)*
- [ ] **Sitemap's honest respelling** — still filters via the old shape. *(IO.md §3)*
- [ ] **Rendition-address extension** — parameterized image outputs beyond the
      citation-site demand I12 shipped. *(IO.md I12)*
- [ ] **Claimed-row rendition scan** — behaviour change under a byte gate.
      *(IO.md I13)*
- [ ] **Eager srcset** — *(IO.md I12)*
- [ ] **Description-page shape** — second output whose content is not the bytes;
      no item owns it. *(IO.md I8→I13, DESIGN.md)*

## MERGE leftovers (Matt's calls)

Not work until decided. Full text stays in MERGE.md §7.

- [ ] **Variant validation policy** — silent degradation for row requests across
      themes, but a view `variant` naming a fragment no loaded theme provides is
      probably a typo. Warning? Error? *(MERGE.md §7 q2)*
- [ ] **Vocabulary pass remainder** — `shell`/`tier`/`frame`, `kind`, row
      `layout`, `[[parts]]` spelling, `template`, tree `source`. Every rename
      touches documented surface. *(MERGE.md §7 q6)*
- [ ] **`--effective` struct-level defaults** — nested defaults invisible when
      neither base nor site writes the table. Grow `--effective`, or a future
      `config --projected`? *(MERGE.md §7 q11)*

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
- [ ] **Bare-name resolution is parked** — all 194 site invocations use paths,
      so §6a's bubble-and-bucket branch never had production coverage; it is
      specced-and-parked and the `bucket` key is deleted (MERGE.md F1). §0's
      tour now writes the path form. Reintroduction trigger: page bundles
      (§5b). (§6a)
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
