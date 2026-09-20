import importlib.util
from pathlib import Path
import os
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from html.parser import HTMLParser

SCRIPTS = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("release_notes", SCRIPTS / "render-release-notes.py")
notes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notes)


class VisibleText(HTMLParser):
    def __init__(self):
        super().__init__()
        self.items = []
        self.in_item = False

    def handle_starttag(self, tag, attrs):
        if tag == "li":
            self.items.append("")
            self.in_item = True

    def handle_endtag(self, tag):
        if tag == "li":
            self.in_item = False

    def handle_data(self, data):
        if self.in_item:
            self.items[-1] += data


class ReleaseNotesTests(unittest.TestCase):
    def test_exact_release_only_and_visible_formatting(self):
        markdown = "## v2.0.0\n- Future one.\n- Future two.\n## v1.13.0\n**Date:** today\n- Add **Start at Login**.\n- Save `settings.start_at_login`.\n## v1.12.0\n- Old one.\n- Old two.\n"
        rendered = notes.render(markdown, "1.13.0")
        parser = VisibleText()
        parser.feed(rendered)
        self.assertEqual(parser.items, ["Add Start at Login.", "Save settings.start_at_login."])
        self.assertIn("<strong>Start at Login</strong>", rendered)
        self.assertIn("<code>settings.start_at_login</code>", rendered)

    def test_html_and_cdata_are_escaped_without_changing_visible_text(self):
        bullet = 'Keep <script> & "quotes" and ]]> safe.'
        rendered = notes.render(f"## v1.0.0\n- {bullet}\n- Use **bold** and `a < b`.", "1.0.0")
        root = ET.fromstring(f"<description><![CDATA[{rendered}]]></description>")
        self.assertNotIn("<script>", root.text)
        parser = VisibleText()
        parser.feed(root.text)
        self.assertEqual(parser.items, [bullet, "Use bold and a < b."])

    def test_single_fix_release_preserves_the_approved_bullet(self):
        parser = VisibleText()
        parser.feed(notes.render("## v1.0.0\n- Fix the Dock icon at login.\n", "1.0.0"))
        self.assertEqual(parser.items, ["Fix the Dock icon at login."])

    def test_missing_empty_duplicate_and_wrong_version_sections_fail(self):
        for markdown in ["", "## v1.0.0\nNo bullets.", "## v1.0.0\n- ", "## v1.0.0\n" + "- Fix.\n" * 6, "## v1.0.01\n- One.\n- Two.", "## v1.0.0\n- One.\n- Two.\n## v1.0.0\n- Three.\n- Four."]:
            with self.subTest(markdown=markdown), self.assertRaises(ValueError):
                notes.render(markdown, "1.0.0")

    @unittest.skipUnless(sys.platform == "darwin", "Appcast script uses macOS PlistBuddy and stat")
    def test_appcast_embeds_notes_before_signing_and_missing_notes_fail_first(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "scripts").mkdir()
            for name in ["prepare-sparkle-appcast.sh", "render-release-notes.py"]:
                shutil.copy2(SCRIPTS / name, root / "scripts" / name)
            bundle = root / "Test.app"
            (bundle / "Contents").mkdir(parents=True)
            with (bundle / "Contents/Info.plist").open("wb") as plist:
                plistlib.dump({"CFBundleShortVersionString": "1.13.0", "CFBundleVersion": "11300"}, plist)
            archive = root / "update.zip"
            archive.write_bytes(b"fixture")
            (root / "RELEASE_NOTES.md").write_text("## v1.13.0\n- Add **Start at Login**.\n- Save `settings.start_at_login`.\n")
            signer = root / "sign_update"
            signer.write_text('''#!/bin/bash
set -eu
if [[ "$1" == --verify ]]; then exit 0; fi
printf 'called\\n' >> "$(dirname "$0")/signer-calls"
file="${!#}"
if [[ "$file" == *.xml ]]; then
    grep -q '<strong>Start at Login</strong>' "$file"
    printf '\\n<!-- sparkle-signatures: test-fixture -->\\n' >> "$file"
else
    printf 'sparkle:edSignature="fixture-signature"\\n'
fi
''')
            signer.chmod(0o755)
            env = {key: value for key, value in os.environ.items() if not key.startswith("SOMETHING_BG_")}
            env["SOMETHING_BG_SPARKLE_SIGN_UPDATE"] = str(signer)
            output = root / "appcast.xml"
            command = ["bash", str(root / "scripts/prepare-sparkle-appcast.sh"), str(bundle), str(archive), str(output)]
            result = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            description = ET.parse(output).findtext("./channel/item/description")
            parser = VisibleText()
            parser.feed(description)
            self.assertEqual(parser.items, ["Add Start at Login.", "Save settings.start_at_login."])
            self.assertEqual((root / "signer-calls").read_text().splitlines(), ["called", "called"])
            (root / "signer-calls").unlink()
            output.unlink()
            (root / "RELEASE_NOTES.md").write_text("## v0.0.0\n- Old one.\n- Old two.")
            result = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(output.exists())
            self.assertFalse((root / "signer-calls").exists())


if __name__ == "__main__":
    unittest.main()
