---
title: An imported artifact
tags: [meta, css]
---
Some pages arrive finished. A demo written in 2003, a one-file toy with its
own layout assumptions, a page rescued from an archive — it already knows
what it looks like, and wrapping it in this site's chrome would only argue
with it.

The [glass pane](/demos/pane.html) is one of those. It brings its own
document: its own `<title>`, its own gradient, its own opinion about
`backdrop-filter`. The site serves it exactly as written.

What it is *not* is invisible. It carries front matter, so it is a row like
any other — it has a title the database can see, it can be queried, and it
can be linked to by source path from a note like this one. Previously those
two properties were mutually exclusive: a file with front matter got wrapped
in a second document, and a file without one was not a row at all.

The shell is the seam. A row says `shell: none` and the body becomes the
whole output; say nothing and it wears the theme like everything else.
