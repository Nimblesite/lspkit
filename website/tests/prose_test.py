"""[WEB-ACCESS-PROSE] Exercise the real Eleventy build and its public pages."""

from html.parser import HTMLParser
from pathlib import Path
import shutil
import subprocess
import unittest
from urllib.parse import unquote, urlsplit

SITE = Path(__file__).resolve().parents[1]
OUTPUT = SITE / "_site"
DOCS = ("docs/getting-started", "docs/crates", "docs/profiler", "docs/examples")


class Document(HTMLParser):
    def __init__(self, path):
        super().__init__()
        self.elements = []
        self.feed(path.read_text())

    def handle_starttag(self, tag, attrs):
        self.elements.append((tag, dict(attrs)))

    def attributes(self, tag):
        return [attrs for name, attrs in self.elements if name == tag]


class ProsePages(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        shutil.rmtree(OUTPUT, ignore_errors=True)
        subprocess.run(["npm", "run", "build"], cwd=SITE, check=True)

    def check_page(self, path):
        page = Document(path)
        self.assertEqual(len(page.attributes("h1")), 1, path)
        styles = [a.get("href") for a in page.attributes("link") if a.get("rel") == "stylesheet"]
        self.assertIn("/assets/css/styles.css", styles, path)
        viewport = [a for a in page.attributes("meta") if a.get("name") == "viewport"]
        self.assertEqual(viewport[0]["content"], "width=device-width, initial-scale=1.0")
        for image in page.attributes("img"):
            self.assertTrue(image.get("alt"), path)
        return page

    def check_links(self, path, page):
        for tag, attrs in page.elements:
            value = attrs.get("href") if tag in ("a", "link") else attrs.get("src")
            if not value:
                continue
            url = urlsplit(value)
            if url.scheme or url.netloc:
                continue
            target = OUTPUT / unquote(url.path).lstrip("/") if url.path.startswith("/") else path.parent / unquote(url.path)
            target = path if not url.path else target
            target = target / "index.html" if target.is_dir() else target
            self.assertTrue(target.exists(), (path, value))
            if url.fragment and target.suffix == ".html":
                ids = [attrs.get("id") for _, attrs in Document(target).elements]
                self.assertIn(unquote(url.fragment), ids, (path, value))

    def test_docs_and_blog_share_a_complete_reading_surface(self):
        docs = self.check_page(OUTPUT / "docs/index.html")
        links = [a.get("href") for a in docs.attributes("a")]
        for route in DOCS:
            self.assertIn(f"/{route}/", links)
        for route in ("docs/prose", "prose"):
            self.assertFalse((OUTPUT / route).exists(), route)
        for route, tags in (("docs/getting-started", ("h2", "ul", "pre", "code")), ("docs/crates", ("h2", "table", "pre")), ("docs/profiler", ("h2", "ul", "ol", "pre"))):
            page = self.check_page(OUTPUT / route / "index.html")
            for tag in tags:
                self.assertTrue(page.attributes(tag), (route, tag))
        examples = self.check_page(OUTPUT / "docs/examples/index.html")
        videos = examples.attributes("video")
        self.assertEqual(len(videos), 3)
        for video in videos:
            self.assertTrue(video.get("poster", "").startswith("https://"))
            self.assertTrue(video.get("aria-label"))
        self.assertEqual(len(examples.attributes("track")), 3)
        self.assertGreaterEqual(len(examples.attributes("img")), 6)
        hrefs = [a.get("href") for a in examples.attributes("a")]
        for link in ("https://sharplsp.dev/docs/profiler/", "https://www.ospreylang.dev/docs/tutorials/profiler/"):
            self.assertIn(link, hrefs)
        for route in ("docs", *DOCS, "blog/one-engine"):
            page = Document(OUTPUT / route / "index.html")
            for video in page.attributes("video"):
                self.assertTrue(video.get("poster", "").startswith("https://"), route)
                self.assertTrue(video.get("aria-label"), route)
            for source in page.attributes("source"):
                self.assertTrue(source["src"].startswith("https://"), route)
            media = [a for a in page.attributes("img") if a.get("src") != "/assets/images/engine.svg"]
            self.assertTrue(page.attributes("video") or media, (route, "needs a video or screenshot"))
            self.assertTrue(media, (route, "needs a screenshot"))
        for path in OUTPUT.rglob("*.html"):
            self.check_links(path, self.check_page(path))
        blog = Document(OUTPUT / "blog/index.html")
        self.assertIn("/blog/one-engine/", [a.get("href") for a in blog.attributes("a")])

    def test_reading_pages_have_header_images_and_explicit_prose_classes(self):
        for route in ("docs", *DOCS, "blog/one-engine"):
            page = Document(OUTPUT / route / "index.html")
            classes = [attrs.get("class", "").split() for _, attrs in page.elements]
            self.assertTrue(any("prose" in names for names in classes), route)
            self.assertTrue(any("page-illustration" in names for names in classes), route)
            image = page.attributes("img")[0]
            self.assertEqual(image["src"], "/assets/images/engine.svg")
            self.assertEqual((image["width"], image["height"]), ("1440", "600"))
        css = OUTPUT / "assets/css/styles.css"
        self.assertLessEqual(len(css.read_text().splitlines()), 2000)

    def test_journal_has_illustrated_posts_and_only_docs_and_journal_navigation(self):
        for path in OUTPUT.rglob("*.html"):
            page = Document(path)
            navigation = [a["href"] for a in page.attributes("a") if "nav-link" in a.get("class", "").split()]
            self.assertEqual(navigation, ["/docs/", "/blog/"], path)
        journal = self.check_page(OUTPUT / "blog/index.html")
        self.assertTrue(journal.attributes("img"), "Journal needs a real header image")
        self.assertTrue(journal.attributes("h2"), "Post titles need a heading hierarchy")
        self.assertIn("/blog/one-engine/", [a.get("href") for a in journal.attributes("a")])


if __name__ == "__main__":
    unittest.main()
