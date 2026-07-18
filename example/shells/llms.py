#!/usr/bin/env python3
# The first script shell (DESIGN §5g): /llms.txt prototyped on the
# experimental bench before the md shell exists as a built-in. Reads the
# grackle-shell/0 JSON payload on stdin, prints a markdown listing.
import json
import re
import sys

payload = json.load(sys.stdin)
assert payload["schema"] == "grackle-shell/0", payload["schema"]

site = payload["site"]
out = [f"# {site['title']}", "", f"> Notes from {site['author']}.", ""]
for row in payload["rows"]:
    # First sentence of the body, tags stripped, as the summary.
    text = re.sub(r"<[^>]+>", " ", row["html"])
    text = re.sub(r"\s+", " ", text).strip()
    first = text.split(". ")[0].rstrip(".") + "." if text else ""
    when = f" ({row['date']})" if row.get("date") else ""
    out.append(f"- [{row['title']}]({site['url']}{row['url']}){when}: {first}")
out.append("")
sys.stdout.write("\n".join(out))
