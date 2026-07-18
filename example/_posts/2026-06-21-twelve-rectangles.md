---
title: Twelve rectangles
tags: [photos, css]
---
The [photo section](/photos/) holds twelve generated rectangles in six
aspect ratios. That is not much of a gallery, but it is exactly the shape
masonry layout cares about: mixed ratios, known dimensions.

{% image photos/p03.png %}

A masonry layout needs each image's aspect ratio at build time — the
engine knows it (it made the thumbnails) and should say so on the tag.
Until then, this note is a placeholder for the feature it will test.
