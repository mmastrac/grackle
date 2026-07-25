# Themes: distribution, inheritance, and the floor

**Status: the base theme landed 2026-07-24; this file is the plan for making
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

## 0. Platform facts

- **Themes are partial**: any kind a theme declines to arrange falls through to
  canonical (generic semantic markup: `data-kind`, `data-slot`, flags as
  `data-<fact>` attributes). Already an inheritance mechanism with one parent.
- **Variant misses degrade**: a row asking for `listing--cards` without that
  fragment falls back to `listing`, then canonical. Row variants are requests.
- **Subthemes**: `theme: "ledger:dark:wide"` renders through `ledger` with
  `data-subtheme="dark wide"` on `<html>`; CSS subselects via
  `[data-subtheme~="…"]`.
- **Identity slots** come from `.slots/` files, not theme files.
- **Token contract**: shared `--bg`, `--size`, `--space`, etc.; all gallery
  themes written entirely in `var(--…)`.
- **`theme.toml`** is the new per-theme config (not yet live; this design
  document makes it real in §3).

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
| 0 | a look | `cp -r` a gallery theme, then `[site] theme = "name"` (§8, built); `grackle theme add <url>` is still specced | none |
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

## 6. The vanilla principle

`vanilla/` (the "2026 user-agent theme") demonstrates the design principle:
**delegate every aesthetic decision to the platform; keep only the decisions
it refuses to make.** Palette = CSS system colors; dark mode = `color-scheme:
light dark`; type = `system-ui` / `ui-monospace`. The engine base fragments
fuse labels and links (semantic element choices); CSS carries only
platform-delegated visuals. Two working rules: **a rule belongs in the base if
a theme would have to re-derive it; it does not if a theme would have to undo
it.** **Ship a shell, own the frame** — a theme's own shell inherits none of
the base's page geometry, because a sticky header or sidebar would need to
undo a centred measure.

## 7. The floor: base as a compiled-in theme

The base is a directory of fragments and stylesheets, compiled into the binary
and merged under every theme. **Every site renders reasonably with no theme at
all** — the guarantee strengthens from "reasonable with every theme" to
"reasonable with nothing."

**Cascade layers**: `@layer reset, base, theme, overlay, post`. Base and theme
are declared; `overlay` (site-owned `.style.scss`) and `post` (per-post CSS)
remain planned. A theme rule beats the base's regardless of selector specificity
(no arms race). The review test: **a rule belongs in the base if a theme would
have to re-derive it; it does not if a theme would have to undo it.** This is
backed by measurement, not taste.

**Three tiers, and the boundary between them is measured.** The reset and the
type **ladder** — heading scale, weight, block rhythm — are unconditional. Only
the **skins** are opt-in (`@import "skin";`): the blockquote rule, the code
panel, table borders, the callout.

The measurement is the answer to "could the base just always apply?" — it can,
for half of itself. Under a theme with a complete type sheet of its own the
ladder is **inert**: the theme's reset zeroes everything the ladder sets, so the
theme wins every conflict and the ladder only fills gaps. The skins are not
inert — on a listing they move a paragraph 19px and the page 61px, because a
blockquote gains a left border and a code block gains a panel. What makes the
ladder safe to impose is that it reads *only tokens*: a theme retunes the entire
hierarchy through `--size`/`--scale` without restating a rule, which is a
stronger sense of "overridable" than the cascade alone provides.

Shipping a `theme.scss` is the only thing that declines the skins. A
tokens-only theme, and a site with no theme at all, get them automatically —
they have nobody else to ask. So writing your first `theme.scss` costs you the
code panel and the blockquote rule, whose absence is legible, rather than the
heading hierarchy, whose absence would be alarming.

## 8. Configuration *(built 2026-07-25)*

`[site] theme = "name[:tokens]"` in `grackle.toml` is the root of the per-row
cascade (front matter → rule defaults → site default), replacing the
`default`-directory magic as the *primary* mechanism; the directory name
stays honored as a fallback so existing sites don't move. `theme add` then
never needs a rename, and `theme list` can mark the site default.

**As built**, it is one rewrite rather than a new resolution path.
`Themes::resolve(row_spec)` spends the key in the single place the five render
paths all pass through: a row that named nothing becomes a row that named the
site's theme, and everything downstream — `get`, `css_of`, the stylesheet pass
— sees a name it already knew how to handle. Absent, `resolve` returns `None`
and every byte is what it was, which is what let this land under URL parity.

Three consequences worth stating, each of which is a decision:

- **The site default is a full spec, so its tokens apply.**
  `theme = "ledger:dark"` is a one-line site-wide dark mode — rung 2 reached
  from rung 0, with no `themes/` edit. A row that names its *own* theme states
  its own tokens; the site's do not follow it, because a subtheme is a dress
  and the row changed clothes.
- **A listing wears it too.** `unanimous_theme` (§5h) still wins when a
  listing's members agree, and their tokens still do not lift; a listing they
  do not claim now takes the site's spec, tokens included, exactly as a
  themeless row does. Before this it took the default theme with no tokens.
- **`default` is spellable with no `themes/` directory**, because `default`
  *is* the base theme (§7). So `theme = "default"` is a legal way to say "the
  floor, explicitly", and only some other name with nothing behind it is a
  load error — listing the knowns, at load, rather than on the first themeless
  page to render.

**One honest edge.** `/css/main.css` is the `default` theme's sheet, and it is
still emitted when `[site] theme` names another theme — referenced by nothing
unless a row says `theme: default`. Naming sheets after the theme that renders
them would fix it, except that `default` → `main.css` exists for URL parity
with the reference build; the real answer is emitting only the sheets a build
actually referenced, which is a pass this change did not want to invent.

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

