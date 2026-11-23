import { describe, expect, it } from "vitest";
import { ALLOWED_TAGS, renderMarkdown } from "../app/utils/markdown";

/** Every tag and attribute in the output, to check against what may come out. */
function tagsOf(html: string): { name: string; attrs: string }[] {
  return [...html.matchAll(/<\/?([A-Za-z][A-Za-z0-9]*)([^>]*)>/g)].map((m) => ({ name: (m[1] ?? "").toLowerCase(), attrs: (m[2] ?? "").trim() }));
}

function expectSafe(html: string) {
  for (const tag of tagsOf(html)) {
    expect(ALLOWED_TAGS as readonly string[], `<${tag.name}> in ${html}`).toContain(tag.name);
    // The only attribute that may appear is a class made by the renderer itself.
    expect(tag.attrs.replace(/^class="md-(link|image)"$/, "").replace(/\/$/, "").trim(), `attrs of <${tag.name}> in ${html}`).toBe("");
  }
}

describe("renderMarkdown", () => {
  it("formats the plain syntax", () => {
    const html = renderMarkdown("# Title\n\n- one\n- two\n\n**bold** and `code`\n\n```\nblock\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |");
    expect(html).toContain("<h1>Title</h1>");
    expect(html).toContain("<li>one</li>");
    expect(html).toContain("<strong>bold</strong>");
    expect(html).toContain("<code>code</code>");
    expect(html).toContain("<pre>");
    expect(html).toContain("<table>");
    expectSafe(html);
  });

  it("does not pass a script tag: it is shown as text", () => {
    const html = renderMarkdown("<script>alert(1)</script>\n\nafter");
    expect(html).not.toMatch(/<script/i);
    expect(html).toContain("&lt;script&gt;");
    expectSafe(html);
  });

  it("does not pass raw HTML with an event handler", () => {
    for (const source of [
      '<img src=x onerror="alert(1)">',
      '<a href="javascript:alert(1)" onclick="x()">x</a>',
      '<svg onload="alert(1)"></svg>',
      "<iframe src='http://example.invalid/'></iframe>",
      "<div style=\"background:url(http://example.invalid/x)\">x</div>",
      "<style>@import url(http://example.invalid/x.css)</style>",
      "<details open ontoggle=alert(1)>x</details>",
    ]) {
      const html = renderMarkdown(source);
      expect(html, source).not.toMatch(/<(img|a|svg|iframe|div|style|details)\b/i);
      expectSafe(html);
    }
  });

  it("does not load an image, from outside or from a data address", () => {
    for (const source of ["![logo](https://example.invalid/a.png)", "![x](data:image/svg+xml;base64,AAAA)", "![](//example.invalid/a.png)", '![x](http://example.invalid/a.png "t")', "![x][ref]\n\n[ref]: http://example.invalid/a.png"]) {
      const html = renderMarkdown(source);
      expect(html, source).not.toMatch(/<img|src=|example\.invalid/i);
      expectSafe(html);
    }
    expect(renderMarkdown("![logo](https://example.invalid/a.png)")).toContain("logo");
  });

  it("does not make a link: no anchor, no destination", () => {
    for (const source of [
      "[click](javascript:alert(1))",
      "[click](https://example.invalid/)",
      "[click](  JaVaScRiPt:alert(1) )",
      "[click](data:text/html;base64,AAAA)",
      "<https://example.invalid/>",
      "https://example.invalid/ and www.example.invalid",
      "[ref]\n\n[ref]: https://example.invalid/",
    ]) {
      const html = renderMarkdown(source);
      expect(html, source).not.toMatch(/<a\b|href=/i);
      expectSafe(html);
    }
    // The text of a link stays.
    expect(renderMarkdown("[click](https://example.invalid/)")).toContain("click");
    expect(renderMarkdown("[click](https://example.invalid/)")).not.toContain("example.invalid");
  });

  it("keeps quotes and angle brackets in text from becoming markup", () => {
    const html = renderMarkdown('a "quoted" <b>x</b> & `<i>` y');
    expect(html).not.toMatch(/<b>|<i>/);
    expectSafe(html);
  });

  it("is safe on empty and odd input", () => {
    expect(renderMarkdown("")).toBe("");
    expectSafe(renderMarkdown("\u0000<scr\u0000ipt>x</scr\u0000ipt>\n\n[a](<javascript:alert(1)>)\n\n![](javascript:alert(1))"));
  });
});
