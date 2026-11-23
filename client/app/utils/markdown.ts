// Markdown to safe HTML, for a request's body.
//
// The body is text written by a person or an agent, so it is never trusted:
// - raw HTML is not passed through (`html: false`: it is shown as text);
// - nothing is loaded from outside: an image is shown as its description, never as `<img>`;
// - a link is shown as its text, never as `<a>`: it opens nothing and goes nowhere;
// - `linkify` and `typographer` are off, so no text turns into a link on its own.
// - a table column's alignment (`:--`, `:-:`, `--:`) comes out as a fixed class, never as a `style` attribute.
// The result is the only thing that goes into `v-html`; the test in tests/markdown.test.ts
// checks that no tag outside the allowed list, and no attribute but `class`, ever comes out.
import MarkdownIt from "markdown-it";

const md = new MarkdownIt({ html: false, linkify: false, typographer: false, breaks: true });

// Links: the text stays, the destination is dropped.
md.renderer.rules.link_open = () => '<span class="md-link">';
md.renderer.rules.link_close = () => "</span>";

// Images: the description stays, as plain text marked as an image.
md.renderer.rules.image = (tokens, index) => {
  const token = tokens[index];
  const alt = token ? token.content : "";
  return `<span class="md-image">［画像${alt ? `: ${md.utils.escapeHtml(alt)}` : ""}］</span>`;
};

// Table alignment: markdown-it writes `style="text-align:..."` on th/td. Only the three known values are
// kept, as a class of ours; the style attribute itself never comes out.
const ALIGN_CLASS: Record<string, string> = { left: "md-left", center: "md-center", right: "md-right" };
md.core.ruler.push("align_to_class", (state) => {
  for (const token of state.tokens) {
    if ((token.type !== "th_open" && token.type !== "td_open") || !token.attrs) continue;
    const align = /^text-align:(left|center|right)$/.exec(token.attrGet("style") ?? "")?.[1];
    token.attrs = token.attrs.filter(([name]) => name !== "style");
    if (align) token.attrSet("class", ALIGN_CLASS[align]!);
    if (token.attrs.length === 0) token.attrs = null;
  }
  return true;
});

/** The HTML tags `renderMarkdown` can produce (what markdown-it makes of the plain syntax). */
export const ALLOWED_TAGS = [
  "p", "br", "hr", "h1", "h2", "h3", "h4", "h5", "h6",
  "ul", "ol", "li", "blockquote", "pre", "code", "em", "strong", "s",
  "table", "thead", "tbody", "tr", "th", "td", "span",
] as const;

export function renderMarkdown(source: string): string {
  return md.render(source);
}
