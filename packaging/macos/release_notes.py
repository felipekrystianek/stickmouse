#!/usr/bin/env python3
"""Writes the update feeds and the changelog page from CHANGELOG.md.

    release_notes.py <version> <dmg file name> <dmg size> <EdDSA signature>

- website/appcast.xml: the Sparkle feed that Stickmouse 1.1 and later read. Its release notes list
  the newest few versions, so someone skipping releases still sees what changed.
- website/version.json: the file Stickmouse 1.0 reads, so those users hear about the update too.
- website/changelog.html: every release, for the website.
"""

import html
import json
import re
import sys
from datetime import datetime, timezone
from email.utils import format_datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SITE = "https://stickmouse.pages.dev"
NOTES_IN_FEED = 5


def releases():
    """[(version, date, [change, ...])], newest first, as written in CHANGELOG.md."""
    found = []
    for line in (ROOT / "CHANGELOG.md").read_text().splitlines():
        heading = re.match(r"^## (\S+) - (\d{4}-\d{2}-\d{2})\s*$", line)
        if heading:
            found.append((heading[1], heading[2], []))
        elif found and line.startswith("- "):
            found[-1][2].append(line[2:].strip())
        elif found and line.startswith("  ") and found[-1][2]:
            found[-1][2][-1] += " " + line.strip()
    return found


def notes_html(entries, heading_tag):
    parts = []
    for version, date, changes in entries:
        items = "".join(f"<li>{html.escape(change)}</li>" for change in changes)
        parts.append(f"<{heading_tag}>Stickmouse {html.escape(version)} <small>{date}</small></{heading_tag}><ul>{items}</ul>")
    return "\n".join(parts)


def main():
    version, dmg_name, size, signature = sys.argv[1:5]
    entries = releases()
    if not entries or entries[0][0] != version:
        sys.exit(f"CHANGELOG.md must start with a '## {version} - YYYY-MM-DD' entry")
    date = format_datetime(datetime.strptime(entries[0][1], "%Y-%m-%d").replace(hour=12, tzinfo=timezone.utc))
    url = f"{SITE}/downloads/{dmg_name}"

    feed_notes = notes_html(entries[:NOTES_IN_FEED], "h3")
    appcast = f"""<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>Stickmouse</title>
    <link>{SITE}/</link>
    <item>
      <title>Stickmouse {version}</title>
      <pubDate>{date}</pubDate>
      <sparkle:version>{version}</sparkle:version>
      <sparkle:shortVersionString>{version}</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>14.0</sparkle:minimumSystemVersion>
      <sparkle:fullReleaseNotesLink>{SITE}/changelog.html</sparkle:fullReleaseNotesLink>
      <description><![CDATA[
<style>body {{ font: 13px -apple-system, sans-serif; }} h3 {{ margin: 12px 0 4px; }} small {{ color: gray; font-weight: normal; }} ul {{ margin: 0; padding-left: 18px; }}</style>
{feed_notes}
      ]]></description>
      <enclosure url="{url}" length="{size}" type="application/octet-stream" sparkle:edSignature="{signature}"/>
    </item>
  </channel>
</rss>
"""
    (ROOT / "website/appcast.xml").write_text(appcast)

    summary = " ".join(entries[0][2])
    (ROOT / "website/version.json").write_text(
        json.dumps({"version": version, "url": f"{SITE}/downloads/Stickmouse.dmg", "notes": summary}) + "\n"
    )

    page = (ROOT / "packaging/macos/changelog.html").read_text()
    (ROOT / "website/changelog.html").write_text(page.replace("<!-- RELEASES -->", notes_html(entries, "h2")))


if __name__ == "__main__":
    main()
