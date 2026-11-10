import DOMPurify from "dompurify";
import { marked } from "marked";

/** The AI's light Markdown (bold, lists, small tables) as sanitized HTML. */
export function renderMarkdown(text: string): string {
  // The tutor never needs images, and an image tag could make the app fetch a URL.
  return DOMPurify.sanitize(marked.parse(text, { async: false, gfm: true, breaks: true }), {
    FORBID_TAGS: ["img", "picture", "video", "audio", "iframe", "form"],
  });
}
