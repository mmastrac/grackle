# grackle 1.0 — the list

Checkbox form. Anything moved out of `DESIGN.md`, `themes/DESIGN.md` or
`themes/README.md` during the compression pass lands here rather than being
lost; `GRAVEYARD.md` holds the compressed prose those files gave up.

Ordering is rough priority within each group, not across groups.

## Defects

- [ ] **Audit the compressed docs for claims whose correction was deleted** —
      the one failure mode this compression pass has. These documents state a
      claim in one section and correct it in another, and the correction
      usually lives in the dated ledger prose the criteria classify as build
      narration. Removing the narration can leave the superseded claim standing
      alone, looking authoritative, with the only thing that contradicted it
      gone — the doc comes out *more* wrong than it went in.
      Found once already: `themes/DESIGN.md` §7 said typography was opt-in,
      while its correction (the `_type.scss` split into an always-on ladder and
      opt-in skins) sat in the Landed ledger that was cut. Fixed 2026-07-25.
      The check is mechanical: for each surviving rule in `DESIGN.md` and
      `themes/DESIGN.md`, confirm it against the code, not against the
      document. `git show dc96d5d^:grackle/themes/DESIGN.md` and its `DESIGN.md`
      sibling have the pre-compression text to diff against.

- [x] **The base theme's shell hardcodes grack.com's favicons** — fixed. The
      icon is `site.icon`, filled from a `favicon.*` at the site root and
      declared in `base.toml`'s `[html.head.link]`; empty emits nothing. The fix
      reached further than the head — the Atom feed's `<icon>`/`<logo>` carried
      the same hardcoded path, so `examples/minimal` was shipping a link to a
      file it does not have. (§4d)
- [ ] **A titleless view renders its own config key as a heading** —
      `trails.rs` falls back to `r.key` / the view name. The base's routes dodge
      it with `@home`/`@blog` refs; the general fallback is still wrong. (§4d)
- [ ] **`from = "*"` on a site-declared route** — a routed row is routed
      whatever its flags say, so every star route must restate
      `!draft && !hidden`. `View::inherited` already records whose route a view
      is, so the validator can refuse a site-declared star route whose `where`
      omits the flags while the base's own passes. (§5c)

## Tooling — the pipeline's dark stages

The pipeline is `file → row → query → doc model → parts → slots → CSS → URL`.
Stages 1–2 have `query explain`; 4–6 have nothing, and the config merge added a
new layer with no inspector at all.

- [ ] **`grackle config --effective`** — print the merged config with
      provenance per key (base vs site file). This is what makes `extends`
      inheritance rather than magic; `examples/raw` is the stopgap. (§4d, named
      there as "should ship before 1.0")
- [ ] **`grackle explain <url> --parts`** — the part map, which producer filled
      each part, which fragment placed it, and **which parts nothing placed**.
      The last is a partial answer to q50's forgotten-vs-deliberate hole that
      needs no settlement first.
- [ ] **Top-level `grackle explain`** — `DESIGN.md` §0 and the manual (ch. 2)
      both teach `grackle explain <url>` as *the* debugging tool; the command is
      `grackle query explain`. Alias it.

## The theme ladder's missing bottom rungs

- [x] **`[site] theme = "name[:tokens]"`** — built. Full spec, so `"ledger:dark"`
      sets the site's subtheme tokens too; absent still means the `default`
      directory then the base theme, and an unknown name is a load error naming
      the knowns. (themes/DESIGN.md §8)
- [ ] **Theme distribution** — the rest of rung 0: `theme add/update/list/new/
      derive/check/try` and `themes/.lock.toml`. Install is still `cp -r`, and
      there is no update path. `derive` is the load-bearing one — it is what
      makes editing a vendored theme safe. (themes/DESIGN.md §4)
- [ ] **Decide `.style.scss` — build it or cut it** — rung 1 (recolour without
      touching a theme) is promised in themes/DESIGN.md §2, the manual ch. 13
      and DESIGN §5b, and is unbuilt. The gallery README meanwhile teaches a
      tokens-only theme directory as the place to start, which contradicts the
      ladder table. Ship one answer to "how do I change the accent colour".
      (§5b, themes/DESIGN.md §2)

## Policy

- [ ] **Base-theme breaking-change policy** — the base *config* has one ("base
      changes that mint URLs are breaking"). The base theme has the same
      exposure and no policy, and the favicon fix above is the forcing case.
      (§4d honest edges)

## Composition

- [ ] **The same N views over N subtrees** — `theme-preview/` is 341 lines and
      33 `[sets]`/`[routes]` tables for six structurally identical subtrees.
      §4d diagnosed why: `from` a posts collection does not scope to it, it
      ranges over the whole posts table. Decide whether that is a wart to fix or
      a missing primitive; today the config answer is copy-paste, which is the
      disease `[sets.published]` exists to cure. (§4d, §5c)

## The manual

- [x] **Resync the manual to §4d/§4e** — largely done. Ch. 3 is now "your first
      site: nothing but content" and teaches the empty config; `extends`,
      `default_content`, `[html.head.*]` and `[schema]` all appear.
- [ ] **Two resync gaps left** — `[collections.<n>.schema]` (the per-collection
      schema axis) appears nowhere in `OUTLINE.md`, and ch. 20 still teaches
      `[markers] ".draft" = …` as something you write, without noting the base
      config now ships `.draft`/`.hidden`/`.noindex`.
- [ ] **Adopt §4d's framing in ch. 4** — the "one law" box is weaker than §4d's
      three-shapes table (keys merge, registries shadow by name, settings bags
      merge per key). A reader who meets "one law" and then hits fragment
      shadowing concludes the manual lied.
- [ ] **Reference generation** — `OUTLINE.md`'s own open question 8, and the
      churn since makes it sharper: 32a/32b/32c/32d must be generated from the
      config structs, `parts.toml` and the error enums, or they rot in a week.
      A `grackle docs` subcommand.
- [ ] **Retired-spelling grep gate** — `[views]`→`[sets]`/`[routes]`,
      `over`→`from`, `filter`→`where`, and now `layout:` on rows. The old
      spellings read as plausible. (OUTLINE open question 9)

## Theme distribution and dev loop

From `themes/DESIGN.md` §4–§5, all specced and unbuilt.

- [ ] **`theme add <url>[@ref]`** — shallow-fetch to cache, copy in, write
      lock; follow `extends = { git = … }` recursively; refuse on `contract`
      mismatch naming both versions.
- [ ] **`theme update [name]`** — replace wholesale if every local hash matches
      the lock; else refuse, list the edited files, point at `derive`.
- [ ] **`theme derive <name>`** — the load-bearing one. Because inheritance is
      file shadowing, "the files you edited" already are a valid derived theme,
      so this is nearly `mv` plus two lines of TOML. It converts the classic
      SSG failure mode (hacked vendor theme, updates now scary) into rung 3
      mechanically, which is what makes rung 4 safe to allow.
- [ ] **`theme check [name]`** — validate fragments + CSS against the engine
      schemas standalone; lint token names against the contract on the
      **resolved chain**, never the leaf alone. Catches the three back-tested
      edges: split `kind`/`kind--variant` pairs, identity slots a child shell
      dropped, vars no ancestor defines.
- [ ] **`theme list` / `theme new` / `theme try`** — chain and lock status;
      rung-3 scaffold; cache-only install loaded last.
- [ ] **`themes/.lock.toml`** — provenance and per-file hashes at install.
- [ ] **Nested `@layer` down the chain** — `@layer theme.root, theme.mid,
      theme.leaf` so a child always outranks its parent by layer and
      `revert-layer` walks one step at a time. Plain concatenation recreates
      the specificity war `@layer` was introduced to settle. Worth building
      this way from the first commit: the failure it prevents is silent and
      shows up only in someone else's theme.
- [ ] **`?theme=name[:tokens]` dev override** — render any page through any
      loaded theme, gated to the dev profile. Both the experimentation loop and
      the standing test of guarantee 2.
- [ ] **Child themes invalidate on ancestor edits** — the chain is known at
      load; invalidate by every chain member's key.
- [ ] **Forced-colors mode** — claimed from spec for vanilla and the gallery,
      never tested.

## From the docs, unverified against code

Harvested from the compression agents. These come from doc prose that is known
to contain stale claims, so **check each against the code before acting** — two
items I carried from the same source (`[site] theme`, the favicon leak) turned
out to be already built.

- [ ] **Notes stream / footnotes as sidenotes** — §6d stage B. (q18)
- [ ] **Per-block facts** — a block-level directive surviving as a `data-`
      attribute so a theme can span it; needs an authoring syntax decision,
      since IALs are kramdown, not CommonMark. (q25)
- [ ] **Body-image dimensions** — `{% image %}` output gains width/height when
      the §6d rewrite stage reaches it. (q26)
- [ ] **Authored `.rewrite.toml` rules** — selector-driven HTML transforms per
      subtree. (§6d)
- [ ] **Per-post `<style>`** — and its auto-scoping default. (§6c, q14)
- [ ] **`md` shell** — specced; `/llms.txt` currently ships via a script shell.
- [ ] **Atom/sitemap as true part-map consumers** — currently bespoke
      serializers. (q44)
- [ ] **Per-theme head-fact selection** — the engine renders all head facts
      today. (§0, §5a)
- [ ] **Variable-length head entries** — `rel="alternate" hreflang` × n cannot
      live in a name→string map; `sizes`/`type` on link entries need the same.
      (§4e)
- [ ] **Listing views render no language switcher** — a French reader landing
      on `/fr/blog/` has no way back. (q47)
- [ ] **`type:` as row data** — held until something other than the renderer
      consumes it. (q48)
- [ ] **Set-scoped computed fields** — aggregates over a view's members. (q39)
- [ ] **Structured record fields** — list-of-records plus JSON-LD emission.
      (q40)
- [ ] **Computed fields onto §5f** — `truncate = {…}` is still the stopgap
      struct shape now that the expression language exists. (q31)
- [ ] **Redirects for restructured URL trees** — no mechanism. (q28)
- [ ] **Metadata for rows that can't carry front matter** — derive, then a
      `.p01.png.toml` sidecar. (q49)
- [ ] **Transplanting an imported page** — and the blocked "deliberate omission
      vs forgotten hole" underneath it. (q50, q45)
- [ ] **Serve v2** — incremental invalidation; v1 rebuilds the world. (q1)
- [ ] **Scoped SCSS cannot declare `:root` custom properties** — silently
      doesn't apply. Needs a documented constraint or a load-time error. (§5b)

## Surveys and audits worth re-running

Methods worth repeating; several of these produced numbers now quoted in the
docs that nobody has re-measured since the base config, the flag move and the
head fold landed.

- **Cross-site render parity** — grack.com, field-notes, minimal, raw,
  theme-preview, byte-identical under the base config. The measurement §4d
  rests on; drift since is unmeasured.
- **URL-set parity** (`grackle urls --against`) — the half that protects 20
  years of inbound links, and the gate for any redirect strategy.
- **The 36-site backtest** (§7b) — re-run against the current model to catch
  structural gaps the last survey couldn't see.
- **The seams audit** (§9b) — do the crate boundaries still hold after the base
  config, the flag move and the head fold?
- **The markdown gap** (§8a) — with the 97-post blind spot still unresolved,
  the 90% figure is over an unrepresentative 230.
- **The archetype audit** (§5e) — walk document/sidenotes, gallery, masonry,
  full-bleed, timeline, dense index against the current part schema.
- **The portability falsifier** — every row through every gallery theme and no
  theme, asserting each row keeps its name. In place; re-run per release.
- **Namespace collisions** — across `[collections]`, `[sets]`, `[routes]`,
  `[records]` after the base-config merge.

---

## Harvested from the DESIGN.md compression pass

Raw per-bucket output from the nine compression agents, appended verbatim so
nothing is lost in a merge. Expect overlap with the sections above and with
each other. Same caveat as "from the docs, unverified against code": these are
derived from doc prose, which in this repo lags the code by hours.

### bucket 1

## Core unbuilt features

- [ ] **Doc model notes stream (stage B)** — where footnotes render is not yet decided. (q6d / §0)
- [ ] **Transplanting (extract + render)** — extract content from imported pages and render through theme. Currently only `shell: none` (byte-exact) or rewrite by hand. (q50 / §24)
- [ ] **Object metadata extraction** — derive metadata from artifact (e.g. `<title>` from HTML) or declare in `.p01.png.toml` sidecars. Currently front matter or nothing. (q49 / §24)
- [ ] **Per-post CSS with `<style>` blocks** — SCSS compiled, cached, hoisted, auto-scoped; `style_scope: false` to opt out. (§27)

## Open schema and edges

- [ ] **List-of-records type** — no list-of-records schema type, no JSON-LD emission. (q40 / §21)
- [ ] **Truncate struct stopgap** — truncate struct is provisional; expression language exists but computed fields haven't migrated to it. (q31 / §7)
- [ ] **Layout: dissolving into shell:** — layout: still carries Jekyll word `page`/`post`; scheduled to dissolve into `shell:`. (q33(f) / §12)
- [ ] **Atom/sitemap as part consumers** — atom/sitemap becoming true part-map consumers; `json` shell unbuilt. (q44 / §19)
- [ ] **Body dimensions on posts** — post bodies still ship without dimensions; only summaries and object images carry them. (q26 / §10, §26)
- [ ] **Language switcher on listings** — listing views render no language switcher; `translations` is a row relation, not available on view routes. (q47 / §30)
- [ ] **Variant fragment hole-dropping** — when a variant fragment omits a hole, the part drops with no warning; omission is indistinguishable from oversight. (q45 / §16)
- [ ] **Redirects for URL restructuring** — URL rewrites/redirects for migrations where routes change substantially are unsolved. (q28 / §24)
- [ ] **Full rewrite rule table** — rewrite stage is narrow (HTML source links only); full `[rule]` table form with selectors and wrapping still specced. (§26)
- [ ] **Profile-scoped `.style.scss` overlays** — `.style.scss` overlay cascade is still specced (separate from layer placement). (§13)
- [ ] **Theme-level `extends` inheritance** — config-level `extends` is built; theme-level `extends` (theme.toml chains) still specced. (§14)

## Known gotchas

- [ ] **Same-day neighbours** — `earlier`/`later` use day-granular comparison with strict `<`, so two posts on the same day are neither's neighbour. (zero pairs in corpus today). (§28)
- [ ] **Cross-kind relation pools** — can only compare fields every candidate carries; parenthesised expressions unsupported (q13 model upgrade pending). (§28)
- [ ] **Script shell publishing** — script shell source is a file, will be routed and published unless excluded (needs `shells/**` in exclude). (§19)
- [ ] **Silent variant degradation** — row-requested variant missing from theme falls back silently; fragment's own `data-fragment=` override IS a load error. (§16)

---

## Surveys/audits worth re-running

- Marker scan performance with `.gitignore = false` vs `true` (currently ~6ms vs 205ms difference).
- Object collision rate: current measurement was `screenshot5.png` / `screenshot6.png` non-unique.
- Posts by language variant: current locale-pivot indexing untested with real multi-language corpus.

---

## OUTLINE duplication noticed

None noticed.

### bucket 2

- [ ] **`grackle config --effective`** — printing the merged config with provenance per key before 1.0. (§4d, honest edge)
- [ ] **A titleless view still renders its config key as heading** — the general fallback for `trails.rs` remains wrong when a route has no `@ref` localization. (§4d, known issue)
- [ ] **Link table form for head attributes** — allow `[html.head.link]` entries to be tables with `href`, `sizes`, `type` etc., restoring favicon/apple-touch-icon support. (§4e, honest edge)
- [ ] **theme.toml inheritance chains** — `extends` at theme level, still specced. (§4e, q33 leftover)

## Surveys/audits worth re-running

None identified in this bucket.

## OUTLINE duplication noticed

None noticed.

### bucket 3

## Open questions

- [ ] **q23 — hero field for groups** — Gallery group metadata. (§5 audit)
- [ ] **q28 — URL-parity on restructured trees** — Redirects for repos that move (mindstorms example, migration story). (§5 audit, ch. 24)
- [ ] **q30 — pagination × subdivision** — Year paginating while months subdivide off year root; `/blog/2022/page/2/` namespace conflict with child routes. Deliberately punted. (§5c)
- [ ] **q32 — URLs derived values** — Settled 2026-07; producers take URLs never construct. (§5c)
- [ ] **q46 — breadcrumb content** — Collection's own `crumb`/`index` fields last non-derived names in trail; q46 proposes dissolving into §5h landings. (§5c)

## Unbuilt design requiring specifications

- [ ] **`.style.scss` overlays** — Scoped CSS per-subtree. Mechanism specified (data-scope wrapping, SCSS nesting, @layer order), constraints documented, but build not complete. (§5b)
- [ ] **Scoped SCSS `:root` constraint** — Custom properties in scoped blocks are silently scoped and don't apply. Must be load-time error or documented constraint. (§5b)

## Surveys/audits worth re-running

- Mindstorms gallery restructure: inventory 17 range HTML pages; measure URL-parity issue (q28 landing consequence).

## OUTLINE duplication noticed

None noticed.

### bucket 4

None identified — §5d and §5e contain no open questions marked q## or unbuilt features marked ★, and no explicitly flagged honest edges requiring action items for 1.0.

## Surveys/audits worth re-running

- Archetype test audit over layout patterns (margin documents, galleries, masonry, full-bleed, timelines, dense indices) to detect new gaps that resolve to schema fields or parts.
- Body image dimension census: recount images carrying width/height in post content to track coverage as new content lands (measured 442/468 = 94.4% at time of cutting).

## OUTLINE duplication noticed

None noticed.

### bucket 5

## Open questions and unbuilt features

- [ ] **`head.html` theme fragment** — optional `head.html` theme fragment appended after computed facts, for themes wanting to add head content (fonts). (§5g)
- [ ] **`md` shell** — markdown serialization of part maps; forcing consumer is `/llms.txt`; needs decision on row-only vs any kind, and how widgets serialize (expanded HTML or unexpanded source). (§5g, q44)
- [ ] **atom/sitemap as true part-map consumers** — a feed entry IS a document-parts subset; `json` shell pending (though script shell covers it now). (§5g, q44)
- [ ] **Explicit `parent =` for URL nesting edge** — when URL nesting lies about parent structure; unneeded so far. (§5h)
- [ ] **Orphaned translation warning** — index.fr.md with no French rows should warn. (§5h)
- [ ] **Mode-B prose in search index** — landing content routes structurally excluded from search; may be missed. (§5h)
- [ ] **Variant fragment hole warning** — a fragment lacking a hole drops the part silently; needs load-time warning for schema parts no fragment places. (§5h)
- [ ] **Home and manual section-tree lifting** — home is queryless landing (q37 board hangs here); manual waits for section tree as landing's listing. (§5h)

## Surveys/audits worth re-running

- None noticed.

## OUTLINE duplication noticed

- None noticed.

### bucket 6

- [ ] **Name bubbling and bucket scoping** — specced design for bare-name resolution; all 194 site invocations use paths, so the feature is unexercised. (§6a)
- [ ] **Authored `.rewrite.toml` rule table** — selectors language specced but deferred, waiting for second consumer beyond link resolution. (§6d stage A)
- [ ] **Per-post `<style>` compilation** — entire feature specced (comrak extraction, scss compilation, auto-scoping, load-time validation). (§6c)
- [ ] **Notes as a second stream and sidenotes** — the footnote model (blocks + notes streams, three addressing modes) is designed but unbuilt. The grid-column layout change required for sidenotes is blocked on this. (§6d stage B, q18)
- [ ] **Computed fields on §5f expression language** — `truncate = {…}` struct form is stopgap; fields should become expressions once config grows functions. (q31 / §6d)
- [ ] **Per-block facts** — factoring facts into individual blocks rather than whole-doc granularity (mentioned in OUTLINE ch. 35). (§6d risk)
- [ ] **Template-per-element cost measurement** — rewrite rules that can inject templates need cost profiling before rollout at scale. (§6d risk 3, 327 posts × many elements)

## Honest edges / clarifications needed

- Bare-name resolution (§6a) never reached in production; spec and code diverge. The aspirational example in §0 (burrs.jpg resolving to sibling) is specced but untested.
- Embedding text includes title and tags, so retitling re-embeds; until body-text re-embedding lands, "Related changed" after refactoring doesn't indicate semantic drift. (§6b)
- Rewrite stage carries one asymmetry (raw-HTML bodies can't distinguish engine-derived vs authored URLs) scoped to unavoidable cases. (§6d stage B)
- Footnote duplicate-`id` collision is theoretical (only `life-before-main` has footnotes; only happens if two footnote posts ever list together). (§6d)
- Streaming rewriter can't use `:first-of-type` or `:has()` — verify CSS selector subset before relying on these. (§6d)

## Surveys/audits worth re-running

- If post retitling becomes common, re-embed on the updated corpus to verify "Related changed" causality. (§6b)

## OUTLINE duplication noticed

None noticed.

### bucket 7

## §6e — Hierarchy

- [ ] **q27: index-less directory rendering** — Unlinked labels for directories without an index page; auto-index view would share `outline_entry` fragment. (§6e / q27)
- [ ] **q35: marker payload vs schema** — If `.section` wants options (depth, ordering), grow markers' payload or use `.schema.toml`-style per-directory config? (§6e / q35)
- [ ] **q23: depth deriver** — Replace v1's hardcoded h2–h3 window with `toc = outline(content, {"max_depth": 3})` expression form via §5f. (§6e / q23)

## §6f — i18n

- [ ] **q40: localized group keys** — Enum records extend to group *keys*, not just value domains. (§6f / q40)
- [ ] **q47: listing view language switcher** — Translations axis is a row relation; listings don't get the switcher, so French reader on `/fr/blog/` has no way back. Workaround: `.slots/` locale link or mode-B landing. (§6f / q47)
- [ ] **Embedded view locale following** — Embedded views currently follow their embedding page's locale; pending build. (§6f / §5h)
- [ ] **Prefix selector corpus exercise** — Built and tested but not yet exercised by a real corpus. (§6f / §6b markers)
- [ ] **Search overlay localization** — `site.title` not yet a `LocalizedStr`, search overlay strings client-side in `/search.js`. (§6f)
- [ ] **`pretty_date` and `month_name` locale-awareness** — Currently computed at route build (month_name) or hardcoded (pretty_date). (§6f)

## §6g — Relations

- [ ] **q44: atom/sitemap as true part-map consumers** — Currently `atom`/`sitemap` shell types; should become part-map clients yielding `{json}` shell option. (§6g / q44)
- [ ] **q13: parenthesised expressions in rank** — `(a + b) > c` is valid CEL but unsupported; error suggests lifting into rank term. Model upgrade or `reindex` strategy needed. (§6g / q13)
- [ ] **Honest edge: same-day post neighbours** — Two posts on the same day are neither's `earlier` nor `later` (day-granular ordinal with strict `<`). Zero cases in corpus; document rather than fix. (§6g / determinism)
- [ ] **Honest edge: cross-kind field type-checking** — Pool spanning kinds may compare only fields every candidate carries; rest is load error where checkable, test-hidden where not. (§6g / honest edges)

## Surveys/audits worth re-running

- Embedding refresh: retitling a post re-embeds it automatically; verify no regressions as corpus grows.
- Locale-parallel views partition validation: ensure no locale produces empty materialized routes when it should.

## OUTLINE duplication noticed

- None noticed.

### bucket 8

## Unfinished work and open edges

- [ ] **q21: 97-post blind spot** — `diff --against _site-prod` cannot see certain categories of changes; still open. Understand scope and document symptom. (§8a)
- [ ] **q22: Reference build regeneration** — `_site-prod` can no longer be regenerated because `{% view %}` is not Liquid. Impacts ability to refresh measurement baseline. Document workaround and plan remediation if needed. (§8a)
- [ ] **Fixture mutation-checking** — Two fixtures in harness (`minimal-blog`, `undeclared-field`) are mutation-checked; audit whether all critical fixtures need this. (§7d)

## Surveys/audits worth re-running

- **Minimal site measurement** — `examples/minimal/` measured at 27 non-blank, non-comment lines (2026-07). Re-run as defaults land. Target: should fall; a rise wants a reason. (§7a)
- **Backtest: 36 real sites against the model** — Comprehensive survey (2026-07) found gap clusters; track progress on q38–q43 closure. Reruns should measure which gaps have become non-issues. (§7b)
- **Markdown gap parity check** — 90.0% usable (92.2% with smartypants). Method: 230 posts, liquid-free, untouched since reference build, normalized for invisible differences. Rerun when major comrak upgrades land. (§8a)
- **Inspector feature audit** — Inspector carries four lenses (tree, rows, views, diagnose); verify completeness per release. (§7c)
- **Fixture-to-function boundary audit** — Original audit found ~17 tests hand-building rows; re-audit after major refactors to keep the boundary clean. (§7d)

## OUTLINE duplication noticed

None noticed.

### bucket 9

## Open questions

- [ ] **q1: Dependency tracking** — Hand-rolled typed invalidation vs `salsa` framework. (q1)
- [ ] **q2: Row version** — Content hash vs mtime+size vs mtime-then-hash pre-check. (q2)
- [ ] **q4: Highlighting fidelity** — Token spans: coarse Rouge-class vs syntect classes. (q4)
- [ ] **q6: Drafts in serve** — Replicate `_drafts` preview from day one or post-phase-3. (q6)
- [ ] **q11: Iframe policy** — Inject sandbox/loading attributes, or passthrough? (q11)
- [ ] **q13: Embedding model pinning** — Silent re-embed on upgrade or explicit `grackle reindex`? (q13)
- [ ] **q14: Style auto-scoping** — Default-on with opt-out or default-off with opt-in? (q14)
- [ ] **q21: Tighten diff's liquid skip** — 30% of corpus unmeasured; 97 of 327 posts falsely excluded. (q21)
- [ ] **q22: Refresh _site-prod reference** — Script refresh or move behind auto-stash flag? (q22)
- [ ] **q23: Hero part** — First-image fallback and mindstorms group hero. (q23)
- [ ] **q25: Per-block facts** — Block-level directives as `data-` attributes; decide authoring syntax. (q25)
- [ ] **q26: Image dimensions in post bodies** — Dimension facts at §6d rewrite stage to kill layout shift. (q26)
- [ ] **q28: Mindstorms restructure** — 17 URLs with no `noindex`; fix accidental indexability before restructure. (q28)
- [ ] **q30: Pagination × subdivision** — Namespace collision or pattern-space overlap check with declaration? (q30)
- [ ] **q33: View-name policy** — Blog_index fallback, template reclaim, sitemap filter eval. (q33)
- [ ] **q34: Three "not content" lists** — Unify private skip lists with §4c layers; keep `_cache/` separate. (q34)
- [ ] **q37: Board kind** — Query over queries; composition of views as content. (q37)
- [ ] **q38: Transclusion** — Render row X inline by reference; backlinks built, needs real consumer. (q38)
- [ ] **q39: Set-scoped computed fields** — Aggregates (count, sum) over a view's members. (q39)
- [ ] **q40: Structured record fields** — List-of-records type plus schema.org/JSON-LD emission. (q40)
- [ ] **q42: Client-side faceted filtering** — Combinable facets via client-side view with typed facet index. (q42)
- [ ] **q43: Media beyond image** — Audio/video schema types, podcast RSS, srcset, externally-hosted. (q43)
- [ ] **q47: Listing language switcher** — Plain listing views don't get translations axis; lost locale link. (q47)
- [ ] **q48: Type as row data** — Declare *what it is*, config maps to presentation; need real consumer. (q48)
- [ ] **q49: Row metadata without front matter** — Derive from artifact, or sidecar `.p01.png.toml`; precedence open. (q49)
- [ ] **q50: Transplant imported page** — Extract meat + apply chrome via `shell:`; omitted part detection. (q50)
- [ ] **q51: Route-token supply** — Unify path tokens (tree) and inline `match` (posts) for cross-tree routing. (q51)
- [ ] **q53: Axes vs relations** — Member type (row/projection/route), declaration site, md twin scope. (q53)

## Surveys/audits worth re-running

- Seams audit post-landings/records/links/i18n: track `build.rs` gravity well and intro/prose family eviction.
- Seams audit post-crate split: revisit boundary declarations as workspace grows.
- Seams audit post-merge passes: monitor kind branches and positional assumptions in new features.

## OUTLINE duplication noticed

None noticed.
