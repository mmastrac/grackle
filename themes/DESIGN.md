# Themes: distribution, inheritance, and the floor

## Landed ledger *(2026-07-24)*

§7's floor shipped, and it went further than this document proposed: rather
than the engine *emitting* vanilla's sheet beneath every theme, **the base
theme is compiled into the binary** (`crates/grackle/assets/base/`, embedded
with `include_str!` as `parts.toml` is) and every theme inherits it —
fragments as well as CSS. A site can forget to copy a directory and cannot
forget the binary. Consequences, section by section:

- **§6 finding 1 inverted, correctly.** The 14 fusion fragments moved into the
  base, where there is one sensible way to write each. Vanilla is now genuinely
  zero-fragment — one `theme.scss` of pure residue. The gallery shrank with it:
  ledger went from 19 files to 5, the whole gallery from 109 to 34.
- **§6 finding 3 superseded.** `body > [data-kind="shell"]` is gone; the base's
  page geometry keys on `[data-frame]`, an attribute its own `shell.html`
  stamps. **Ship a shell, own the frame** — which also fixes a hazard this
  document missed: a theme with a sticky full-bleed header would have inherited
  a measure it had to undo (atlas and miroir both would have).
- **§6 finding 4 resolved by relocation.** `callout` lives in `_type.scss`, the
  opt-in typography partial, not the always-on reset. The honest consequence:
  **vanilla renders a callout as plain text**, which is readable, so opt-in is
  the defensible answer rather than a dodge.
- **§7's review test sharpened.** "Would terminal and marginalia both be happy
  inheriting this?" became **a rule belongs in the base if a theme would have to
  re-derive it; it does not if a theme would have to undo it.** That line is
  backed by measurement, not taste: the base briefly carried vanilla's comma
  separators and truncation ellipsis, and both turned up inside ledger's pills
  and on top of its fade.
- **§7's two preconditions are both discharged** — finding 4 by `_type.scss`,
  the austerity test by `[data-frame]` plus the re-derive/undo rule.
- **§10 step 4 is done**: vanilla runs through the real engine, its fragments
  pass `binder.rs` validation, grass compiles its sheet, and the portability
  falsifier landed as four tests (below).
- **§10 step 2 partially**: a tokens-only child compiles (`css_pass` falls back
  to `_tokens.scss` when there is no `theme.scss`), and the dev server
  invalidates through the `themes` symlink.
- **§3's substrate is live.** `theme.rs`'s `from_sources` is the union merge
  this document specified for `extends`, with the base as a hardcoded single
  parent — `extends` generalizes the parent list rather than inventing the
  mechanism. All three back-tested edges now have live instances against it:
  mixed-lineage variants (the base ships `summary--figure`), identity-slot
  derivation from the merged shell (so even a themeless site gets site nav),
  and the token lint.
- **Still pending**: rung 0 (`theme add`, `[site] theme`), rung 4 (lockfile),
  derive-from-a-theme. A rung *below* 0 appeared instead: **no theme at all is
  now reasonable** — `examples/minimal` has no `themes/` directory and renders
  semantic HTML with a stylesheet.

Four tests hold it, each mutation-checked when written: the base drops only
what an EXEMPT list declares (5 entries, each with a reason), every theme keeps
a row's name, no theme uses a token nothing defines, and no `theme.scss` names
a colour. Plus four on the CSS pipeline: the tokens-only theme compiles, a
theme with a sheet is not given typography, the full cascade order is declared,
and a wide code block never scrolls the page.

### Second review pass *(same day)*

- **The growth cliff is mostly gone, and the fix came from asking whether the
  base could just always apply.** It can — for half of itself, and the half is
  determined by measurement rather than argument. Splitting `_type.scss` into a
  **ladder** (heading sizes, weight, block rhythm) and **skins** (blockquote
  rule, code panel, table borders, callout), then applying each under
  grack.com's theme:

  | half | under a theme with its own type sheet |
  |---|---|
  | ladder | **inert** — the theme's reset wins every conflict; the ladder only fills gaps |
  | skins | moves a paragraph 19px, a listing page 61px |

  So the ladder is now unconditional and the skins stay opt-in
  (`@import "skin";`). Writing your first `theme.scss` no longer costs you the
  heading hierarchy — only the code panel and the blockquote rule, whose
  absence is legible rather than alarming. What makes the ladder safe to impose
  is that it reads **only tokens**: a theme retunes the entire hierarchy
  through `--size`/`--scale` without restating a rule, which is a stronger
  sense of "overridable" than the cascade alone provides. This is the
  structure/decoration line one level deeper than the base/vanilla split, and
  the third time that line has done real work.
- **Edge 2 is linted now, not deferred to `theme check`.** Five gallery themes
  ship their own shell, so "a child shell drops `copyright` and a site file
  goes dark" is a live hazard; a test asserts every theme's shell places every
  slot the base's does. Verified by deleting atlas's copyright slot and
  watching it fail.
- **A `_tokens.scss` nothing imports now warns.** The dead-file trap had a
  second arm — tokens sitting beside a `theme.scss` that never imports them.
- **A stylesheet that fails to compile now fails the build.** `serve` still
  prints and carries on; `build` refuses and writes nothing, so the last good
  output survives. The binder already treats a malformed fragment as a build
  error, and the CSS half of the same theme was the lenient one — publishing a
  site whose stylesheet silently failed is the worst available outcome, because
  it looks deployable.
- **Iterating on the base got its loop back.** A dev build serves
  `assets/base/` from the source tree it was compiled from and watches it, so
  editing the floor reloads like editing a theme. Released binaries never touch
  disk (`option_env!` is `None`, and the directory check fails anyway).
- **One correction to the first ledger**: it cited "+320px on a long post" as
  the measured cost of the base's typography under grack.com. That number was a
  measurement artifact — snapshots taken before fonts and images settled.
  Measured symmetrically the base is *inert* on grack.com. The `_type.scss`
  split stands on the argument (a second, invisible type scale under a theme
  that has one), not on that number, and the figure is gone from the code.

### grack.com dropped its Meyer reset *(same day)*

The one deferred item — the "risky retheme with no golden" — is done. grack.com
carried the Meyer reset (a hard 90-element blanket: `margin/padding/border: 0`,
`font: inherit`, `line-height: 1`, `list-style: none`), and its 860 lines of
chrome were written against that blank slate. The engine base is a *light*
reset, so the two were not interchangeable — dropping Meyer naively grew the
site 16% and over-indented content lists.

It came out as **six targeted rules** grack now owns (`themes/default/_reset.scss`),
each a measured dependency rather than a guessed one, found by diffing *every
element* of five page types against the frozen Meyer build:

- `body { line-height: 1 }` — the chrome is single-line and was tuned to it;
- headings and the block elements grack doesn't space itself, zeroed, so the
  engine ladder (which grack now sits under) fills gaps but never doubles up;
- `ul, ol` un-bulleted and un-indented — grack marks its list `<li>`s by hand;
- three overrides for the base's own *floor* rules on engine vocabulary that
  Meyer's element-blanket used to wipe: the site-title link's weight, the 4px
  neighbour-link padding, the 32px summary margin.

Verified pixel-identical across home, a post, the blog listing, a table post
and a blockquote post — every page-height and every block type matching the
Meyer build exactly (the retheme's diff harness lives in the session log, not
the tree). The lesson worth keeping: **a light base reset is not a drop-in for
a hard one.** A theme moving onto the base owns the specific blank-slate
assumptions its chrome was written against — a short, legible list, not a
third-party blanket. grack.com is the proof it is short: six rules, and the
stylesheet got 770 bytes *smaller*.

---

**Status: design, with one artifact built.** This file is the plan for making
themes installable, derivable, and safe to update — plus the `vanilla` theme
(built, in this directory) that anchors the "every site renders reasonably
with every theme" guarantee. It is self-contained: everything it assumes from
`../DESIGN.md` (§5a, §5b, §5e) is restated in §0. Sections are ordered so the
implementation plan in §9 can point at them.

The pain being designed away: in most SSGs a theme is a submodule, a fork, or
a hand-copied directory, and the moment you touch one file you can never
update again. The design goal is a **ladder of customization** (§2) where
every rung is strictly more effort and strictly more power, no rung requires
understanding the rung above it, and — the part other SSGs miss — there is a
mechanical path back down (`theme derive`, §4).

## 0. What already exists (the platform this builds on)

Verified against the code, not the spec:

- **A theme is a directory of data** (§5e): binder fragments + SCSS, no code.
  Loaded per directory under `themes/` (`theme.rs::Themes::load_all`), chosen
  per row (`theme:` front matter, cascadable via rule defaults), site default
  is the directory named `default`, and a site with no themes gets the null
  theme — no directory needed.
- **Themes are partial by construction**: any kind a theme declines to
  arrange falls through to `parts::canonical()` — generic semantic markup in
  schema order (`parts.toml`), `data-kind` roots, `data-slot` holes, flags as
  `data-<fact>` attributes. This is already an inheritance mechanism with one
  hardcoded parent.
- **Variant misses already degrade** (`binder.rs::render_with`): a row asking
  for `listing--cards` under a theme without that fragment falls back to
  `listing`, then to canonical. Row-declared variants are requests, not
  demands. (The load-time "missing child fragment" error applies only to a
  fragment's own `data-fragment=` override — intra-theme, correctly strict.)
- **Once rendering drops into `canonical()`, it is canonical all the way
  down** — canonical never consults fragments for child kinds. Per-kind
  fallback happens only while a themed fragment is doing the rendering.
  Consequence: micro-fragments (crumb, tag) are only reached through a themed
  ancestor, which shapes what "minimal theme" means (§6, finding 1).
- **Subthemes** (`theme.rs::split_spec`): `theme: "ledger:dark:wide"` renders
  through `ledger` with `data-subtheme="dark wide"` on `<html>`; theme CSS
  subselects `[data-subtheme~="…"]`. Orthogonal to everything below.
- **The token contract** (README.md): the shared `--bg`/`--size`/`--space`…
  vocabulary; `_base.scss` and `_search.scss` byte-identical across all
  gallery themes, written entirely in `var(--…)`.
- **Identity slots**: shell slots the engine doesn't fill resolve from
  `.slots/` files up the source tree — the site's words never live in theme
  files. Derived from the theme's *shell fragment* slots, which matters in §6
  finding 3.
- **Style overlays** (§5b): site-owned `.style.scss` files by tree position,
  in a cascade layer above theme CSS.
- **`theme.toml` is specced but absent** — `theme.rs` never reads it. This
  design is the moment it becomes real (§3).

## 1. The two guarantees

1. **Any theme on any site**: a theme cannot name site-specific things
   (unknown slots are load errors; identity slots come from the tree), and
   rule 2 (empty part deletes its element) makes absent data silent.
2. **Any site under any theme**: variants degrade (§0), unarranged kinds fall
   to a floor that looks intentional (§7), and knob customization (§2 rung 1)
   survives theme switches because token names are a cross-theme contract.

Both directions get a checker: `grackle theme check` (§4) validates a theme's
fragments and CSS against the engine schemas standalone, and lints token
usage against the contract vocabulary.

## 2. The ladder of customization

| rung | you want | you do | theme files touched |
|---|---|---|---|
| 0 | a look | `grackle theme add <url>`, set site default (§8) | none |
| 1 | different colors/fonts/spacing | site-owned root `.style.scss`: `:root { --accent: … }` — the overlay layer sits above theme CSS | none |
| 2 | a variant the theme ships | `theme: ledger:dark` subtheme token | none |
| 3 | different arrangement | derived theme: `extends` + shadowed fragments and/or `_tokens.scss` (§3) | none (parent pristine) |
| 4 | full control | edit the installed theme; the lockfile knows (§4) | all |

The independence rule: rung *n* must not require understanding rung *n+1*.
Rung 1 is deliberately **not** a theme mechanism at all — it needs no new
machinery, and because the token vocabulary is a contract, a rung-1 override
survives not just theme updates but theme *switches*. Documentation should
state that as a guarantee; `theme check`'s token lint is what keeps it true.

## 3. Inheritance: `extends`

`theme.toml` (now real) gains:

```toml
name        = "mytheme"          # optional; directory name is identity
extends     = "ledger"           # a theme directory name
# extends = { name = "ledger", git = "https://…" }   # installable parent
contract    = 1                  # part-schema major version (§4)
```

**Fragments: shadow, by file name.** Effective fragment set = union down the
chain, child wins. Implemented at load, not render: `Themes::load_all`
resolves chains (cycles and unknown parents are load errors naming the
chain), then builds each theme's `Fragments` as the merged map. Everything
downstream — render fallback, identity-slot derivation, variant resolution —
already works on a `Fragments` value and needs no change. The shell fragment
resolves the same way: nearest in chain; identity slots derive from the
merged result.

**CSS: concatenation, tokens cascade by the platform's own rule.** No Sass
load-path tricks — Sass resolves imports file-relative, so partial shadowing
across directories would lie. Instead:

```
css(theme) = css(parent)                                  # empty for a root
           + compile(theme.scss)                          # if present
           | compile(_tokens.scss as :root block)         # else, if present
```

Custom properties are last-wins, so a child `_tokens.scss` listing only the
two vars it changes is *complete*, stays correct when the parent adds tokens,
and never copies anything. A child with real rules ships a small `theme.scss`
that `@import "tokens";`-s its own file and states its rules; the parent's
CSS is already above it in the output. Output files are per-concrete-theme
(`/css/<name>.css`, `default` keeps `/css/main.css`), unchanged.

The minimal derived theme is therefore two files:

```
themes/mine/theme.toml       extends = "ledger"
themes/mine/_tokens.scss     :root { --accent: #b5651d; --measure: 65ch; }
```

Depth is unbounded, practically 1–2. Subtheme tokens compose unchanged (CSS
from any chain member may select `[data-subtheme~="…"]`).

> **Concatenate into NESTED LAYERS, not into one.** Plain concatenation puts
> parent and child rules in the same cascade level, which recreates exactly
> the specificity war `@layer base, theme` was introduced to settle one level
> up: a parent's `.card .title` beats a child's `.title` no matter that the
> child is later and more specific about wanting to win. The landed base/theme
> split proves the shape works; extend it down the chain —
>
> ```css
> @layer reset, base, theme, overlay, post;
> @layer theme.root, theme.mid, theme.leaf;   /* chain order, root first */
> ```
>
> — so a child always outranks its parent by layer, and `revert-layer` walks
> the chain one step at a time. Sub-layers are ordered by their declaration,
> so the emitter states the chain once and each member's CSS goes in
> `@layer theme.<member>`. Worth building this way from the first commit: the
> failure it prevents is silent and only shows up in someone else's theme.

### Back-tested edges

Three consequences of the union model, found by walking inheritance
scenarios against the design. None changes the model — the union makes each
*legible* rather than wrong — but all three are the difference between
inheritance that works on day one and inheritance that survives a parent
update two years later. Each gets a test in §10 step 1 and a lint in
`theme check`:

1. **Mixed-lineage variants.** The union is per-file, so a child that
   shadows `listing.html` but not `listing--cards.html` renders cards
   through the parent's variant on the child's base styling. Correct —
   shadowing is by name, variants are names — but the least obvious merge
   consequence: a child restyling a kind heavily should shadow its variants
   too. `theme check` warns when a chain splits a `kind`/`kind--variant`
   pair across themes.
2. **Identity-slot drift.** Identity slots derive from the merged shell's
   slot set. A child shell that *drops* a slot the parent had (`copyright`,
   say) silently disconnects the site's `.slots/copyright.md` — a file the
   site author owns goes dark with no signal. Correct behavior, wrong
   silence: `theme check` (and `theme list`'s chain view) names identity
   slots the chain lost relative to a parent.
3. **Token deletions, not additions.** Additions are the case the cascade
   solves. The asymmetry is a parent update *removing* a token: a child's
   two-line `_tokens.scss` still overrides fine, but a child rule reading
   the vanished var fails silently at computed-value time — CSS has no
   unknown-var error. This is why the token lint runs against the *resolved
   chain*, never the leaf theme alone.

The two-file derived theme (rung 3, tokens-only) has no edges left and is
considered de-risked; fragment shadowing is de-risked modulo edge 1.

## 4. Distribution: the `theme` subcommand

**Vendor, never submodule.** The site is a database that lives in git;
installed themes are files in `themes/<name>/`, committed like everything
else. Provenance lives in `themes/.lock.toml` — installer metadata is the
site's, not the theme author's (a fork must not carry a stale origin):

```toml
[themes.paperback]
git       = "https://github.com/…/paperback"
ref       = "v2"                        # requested (branch/tag); absent = default branch
commit    = "8c1f2…"                    # resolved at install
[themes.paperback.files]                # content hashes at install time
"theme.scss"   = "sha256:…"
"summary.html" = "sha256:…"
```

| command | does |
|---|---|
| `theme add <url>[@ref] [--name n]` | shallow-fetch to cache, copy tree in, write lock. Follows `extends = { git = … }` recursively; refuses on `contract` mismatch with a message naming both versions |
| `theme update [name]` | fetch; if every local file hash matches the lock, replace wholesale and re-lock. Else refuse, listing exactly the edited files, and point at `derive` |
| `theme list` | installed themes, chain, origin, clean/dirty per lock |
| `theme new <name> [--extends x]` | scaffold rung 3: `theme.toml` + commented empty `_tokens.scss` listing the contract names |
| `theme derive <name> [new]` | move the *edited* files (per lock hashes) into a new theme extending it, restore pristine parent files, re-lock, print the front-matter/rule change to re-point rows |
| `theme check [name]` | validate fragments + CSS selectors against engine schemas without a site; lint `--…` names against the token contract, always on the resolved chain (§3 back-tested edges: split variant pairs, lost identity slots, vars no ancestor defines) |
| `theme try <url>` | install to cache only (loaded last, never committed); preview via the dev override (§5); `add` keeps it |

`derive` is the load-bearing command: because inheritance is file shadowing,
"the files you edited" *already are* a valid derived theme — the command is
nearly `mv` plus two lines of TOML. It converts the classic SSG failure mode
(hacked vendor theme, updates now scary) into rung 3 mechanically, which is
what makes rung 4 safe to allow.

Update strategy is refuse-and-derive only. Three-way merging vendored files
is git's job; the lock carries the base commit for anyone who wants it.

## 5. Dev mode

- **`?theme=name[:tokens]` query override** in the dev server, gated to the
  dev profile: render any page through any loaded theme, front matter
  untouched. This is both the experimentation loop and the standing test of
  guarantee 2 — flip through every theme on any page.
- Theme edits already invalidate by `Template(...)` key; the new requirement
  is that a child theme's pages invalidate on *ancestor* edits (the chain is
  known at load — invalidate by every chain member's key).
- `theme try` caches load last so it can shadow nothing by accident.

## 6. The vanilla experiment: findings

`vanilla/` (built, this directory) is the "2026 user-agent theme": the design
principle is **delegate every aesthetic decision to the platform; keep only
the decisions the platform refuses to make**. Palette = CSS system colors
(`Canvas`, `CanvasText`, `LinkText`, `GrayText`, `Highlight`); dark mode =
`color-scheme: light dark` and nothing else; type = `system-ui` /
`ui-monospace` at the UA's own heading numbers; radius 0, shadows none,
motion none. The token file binds the **entire gallery contract to platform
values** — the proof the contract has a zero point. The residue — the whole
personality budget — is seven opinions, each passing "what would the UA do if
it knew this element?": the measure (`70ch`), body leading 1.5 (WCAG 1.4.12),
nav as a row, `/` crumb separators, ellipsis truncation, bold
`aria-current`, one `color-mix` hairline. Every selector is engine
vocabulary (`data-kind`, `data-slot`, facts, `aria-current`) — never a
class.

Findings from building it, each of which corrected this design:

1. **"Zero fragments" is falsified.** `canonical()` deliberately renders
   `label` and `url` as separate parts (element choice is a theme decision),
   so a crumb is a label *plus* a literal URL link — complete and navigable,
   not fused. A reasonable default therefore needs the one-line fragments
   that fuse them (`<a data-slot-href="url" data-slot="label">`). Restated:
   **fragments carry the semantic element choices (h1, `<time>`, label-
   wearing links); CSS carries only platform-delegated visuals.** Vanilla
   ships 14 classless fragments, all trivial.
   > **Landed differently.** Trivial *and identical in every theme* — which
   > made them engine material, not theme material. All 14 moved into the
   > compiled-in base; vanilla now ships none. The finding stands, its
   > conclusion inverted: because there is one sensible way to fuse a label
   > and a url, it is written once, in the binary.
2. **The grouped-parts tax is real and immediate.** Vanilla's summary meta
   line groups `date` + `tags` in one `<p>`; when both parts are empty, rule
   2 deletes the slots and leaves the wrapper. The theme that groups pays
   `:not(:has(*))`, direct-child-scoped so content paragraphs are untouched.
   (Same lesson as README's `terminal` note; now demonstrated twice.)
3. **The null-theme shell needs one deliberate selector.** With no shell
   fragment, the body is `<section data-kind="shell">` — and the root shell
   *also* stamps `data-kind="shell"` on `<html>`, so the floor's measure
   rule must be `body > [data-kind="shell"]`, attribute-alone would grab the
   root. Without it, a shell-less site gets full-bleed text and guarantee 2
   fails at the first hurdle.
   > **Landed differently.** The base ships a shell, so the canonical-shell
   > case no longer arises; the selector is `[data-frame]`, stamped by that
   > shell. It answers a question this finding did not ask — not "how does a
   > shell-less site get the measure" but "how does a theme WITH a shell avoid
   > inheriting one it must undo". Ship a shell, own the frame.
4. **`_base.scss` carries one impurity**: the `callout` rule styles the
   custom widget wrapper a *site's* `grackle.toml` declares. If base is
   absorbed into the engine (§7), widget-wrapper defaults need a story —
   either the engine emits a generic rule per declared wrapper, or that
   block stays theme territory.
   > **Resolved by relocation.** `callout` sits in `_type.scss`, the opt-in
   > typography partial, so the always-on reset stays free of site vocabulary
   > and a theme that wants the skin imports it. The cost, stated plainly:
   > **vanilla renders a callout as plain text.** An unstyled `<callout>` is
   > an inline span wrapping its own block, which is readable — so this is a
   > defensible answer rather than a deferred one.
5. **Tooling note**: the theme's SCSS uses inline `//` comments, which are
   fine for the engine's Sass compile but invalid CSS — a naive concat (as
   the demo build did) silently eats the *following* declaration. Any
   "compile" path that isn't real Sass must strip them; the §3 concatenation
   model concatenates *compiled* CSS, never raw SCSS, partly for this reason.

Verified in-browser (demo, below): both OS color schemes with zero authored
hues; placeholder-link rendering of inert crumb tails, disabled pager arrows
and the current page; rule-2 deletion incl. finding 2; the canonical page
getting the measure and element defaults. **Not yet verified**: forced-colors
mode (claimed from spec, untested); the fragments have not been run through
`binder.rs` load validation; the engine has not compiled vanilla's SCSS
(grass). The demo (`/tmp/vanilla-demo`, `vanilla-demo` entry in
`.claude/launch.json`, port 9000) is hand-rendered per the binder's rules,
not engine output — three pages: listing, document, and the same document
through `canonical()` for side-by-side comparison.

> **Now engine output.** Vanilla is a subtree of `theme-preview/` at
> `/vanilla/`, served by the real engine: the fragments (now the base's) pass
> `binder.rs` load validation, grass compiles the sheet, and the whole gallery
> renders through `Themes::load_all` on every `cargo test`. Forced-colors mode
> is still claimed from spec, still untested. One thing the demo could not have
> caught, found at mobile width: `pre { overflow-x: auto }` had landed in the
> opt-in typography partial, so a long code line under vanilla scrolled the
> whole page sideways. It is in the reset now, beside the `img { max-width }`
> rule it matches — a wide code block breaks the *page*, not just its block,
> which is the same class of bug as an oversized image and not a matter of
> taste. The skin (panel, border, padding) stays opt-in.

## 7. The floor: base + canonical as engine layers *(landed 2026-07-24 — see the ledger)*

Element defaults (`_base.scss`) cover the whole markdown vocabulary but style
no chrome; theme sheets style *fragment classes* that don't exist until a
theme invents them. The gap between "readable" and "reasonable" is a third
sheet keyed on markup the engine guarantees:

| layer | selects on | owner |
|---|---|---|
| `base` | elements | engine (candidate) |
| `canonical` | `[data-kind]` / `[data-slot]` / facts | engine (candidate) |
| `theme` | the theme's fragment classes | theme |

Vanilla's stylesheet **is** the base+canonical artifact — every selector
already qualifies. The proposal: the engine emits it as `@layer base,
canonical` beneath every theme (cascade order `reset, base, canonical,
theme, overlay, post`), so partial themes' unarranged kinds and the null
theme fall through to something intentional-looking, and "start from one
fragment and grow" starts from a decent page. Themes disagree by shadowing
or `revert-layer` — document that as the intended escape hatch, because a
theme reusing canonical markup for a kind inherits the floor's choices.

Two conditions before applying: resolve finding 4 (the `callout` impurity),
and keep the canonical layer austere — the review test for every rule is
"would `terminal` and `marginalia` both be happy inheriting this?" This is
severable from §§3–5; ship it last.

> **As built**, four things differ from the proposal above.
>
> 1. **The floor is a THEME, not a layer the engine emits.** The base is a
>    directory of fragments *and* a sheet, compiled into the binary and merged
>    under every theme by `Theme::from_sources`. `Theme::null()` is the base,
>    so the guarantee strengthens from "reasonable with every theme" to
>    "reasonable with **no** theme".
> 2. **Two layers, not three.** `@layer base, theme` — base and canonical
>    turned out to be one artifact, exactly as this section noticed ("vanilla's
>    stylesheet *is* the base+canonical artifact"), so there was nothing to
>    split. The sheet declares the full `reset, base, theme, overlay, post`
>    order anyway: `overlay` (§5b) and `post` (§6c) emit nothing yet, and
>    declaring them costs nothing while making this statement the authority on
>    the order rather than leaving a future implementer to discover that an
>    undeclared layer sorts last by accident.
> 3. **The review test is now re-derive/undo**, which is sharper and
>    falsifiable — see the ledger.
> 4. **Typography is opt-in** (`@import "type";`), because a second heading
>    ladder underneath a theme that has one is invisible and wrong. A site with
>    no theme gets it automatically, having nobody else to ask.
>
> `revert-layer` remains the documented escape hatch and is now real: theme
> rules beat base rules by layer, whatever the selectors say.

## 8. Configuration

`[site] theme = "name[:tokens]"` in `grackle.toml` becomes the root of the
per-row cascade (front matter → rule defaults → site default), replacing the
`default`-directory magic as the *primary* mechanism; the directory name
stays honored as a fallback so existing sites don't move. `theme add` then
never needs a rename, and `theme list` can mark the site default.

## 9. Rejected

- **Submodules / gems / Hugo modules** — the update-fear machine this design
  exists to replace. Vendor + lock.
- **Theme-level config schemas** (`theme.params`): a second config language.
  Knobs that can be CSS custom properties are tokens (rungs 1–2); a knob
  that can't is a fragment (rung 3).
- **Three-way merge on update** — git's job; refuse-and-derive keeps the
  model legible.
- **Runtime chain-walking for fragments** — merged at load instead; render
  paths stay untouched and infallible.

## 10. Implementation order

Each step lands alone; nothing depends on a later step.

1. **`theme.toml` loader + chains** — parse (`name`, `extends`, `contract`);
   resolve chains in `Themes::load_all` with cycle/unknown errors; merged
   fragment union per theme (child wins); identity slots from merged shell.
   Tests: shadowing, cycles, a derived theme with no files behaving as its
   parent, and the three §3 back-tested edges — a split `kind`/`kind--
   variant` pair rendering mixed-lineage, a child shell dropping a parent
   identity slot, a child rule surviving a parent token removal. (`theme.rs`)
2. **CSS concatenation** per §3, including the tokens-only child. Dev-server
   invalidation by every chain member. (`build.rs`, css pipeline)
   > **Half landed.** The tokens-only child compiles — `css_pass` falls back to
   > `_tokens.scss` when there is no `theme.scss`, with a regression test,
   > because it shipped once as a directory whose stylesheet silently never
   > read its own token file. Dev-server invalidation works through the
   > `themes` symlink (`serve.rs` watches the canonicalized target, and
   > `is_content` no longer rejects theme files under `/grackle/`). Chain-member
   > invalidation arrives with chains.
3. **`[site] theme`** default in the config cascade. (`config.rs`)
4. **Vanilla through the engine**: add it to a real preview config, fix
   whatever `binder.rs` validation and grass find (finding: fragments are
   unvalidated), add the gallery README row + the portability falsifier —
   a test rendering the corpus through *every* gallery theme and the null
   theme, asserting build success.
   > **Done.** `/vanilla/` in `theme-preview/`; fragments validate; grass
   > compiles. The falsifier landed stronger than "build success": every theme
   > × every kind must keep the row's name, and the base must drop only what an
   > EXEMPT list declares with reasons. Both mutation-checked.
5. **`grackle theme` subcommand**: `add`/`update`/`list`/`new` + lockfile;
   then `derive`; then `check`; then `try`. (new `cli/theme.rs`)
6. **Dev `?theme=` override**, dev-profile-gated.
7. **The floor** (§7), after finding 4 is resolved and vanilla has survived
   step 4 unchanged for a while.
