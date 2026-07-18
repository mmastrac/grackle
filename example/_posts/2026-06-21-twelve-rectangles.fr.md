---
title: Douze rectangles
tags: [photos, css]
---
La [section photos](view:gallery) contient douze rectangles générés selon six
proportions. Ce n'est pas une grande galerie, mais c'est exactement la forme
qui intéresse une mise en page masonry : des proportions variées, des
dimensions connues.

{% image photos/p03.png %}

Une telle mise en page a besoin de la proportion de chaque image au moment du
build — le moteur la connaît, c'est lui qui a fabriqué les vignettes, et il
devrait la déclarer sur la balise. En attendant, cette note tient la place de
la fonctionnalité qu'elle servira à tester.
