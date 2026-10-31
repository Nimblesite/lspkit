import techdoc from "eleventy-plugin-techdoc";
import media from "./src/_data/media.json" with { type: "json" };

// [WEB-EXAMPLES] Every docs page shows the profiler running in SharpLsp or
// Osprey. Media lives on each project's own site; one data file names it.

/** Names the project the media comes from, so a reader knows what SharpLsp or Osprey is. */
function caption(item, more) {
  const project = media.projects[item.project];
  return `<figcaption><a class="example-project" href="${project.url}"><strong>${project.name}</strong> · ${project.blurb} ↗</a>${item.caption}${more}</figcaption>`;
}

function video(key) {
  const clip = media.videos[key];
  const poster = clip.poster ?? `${clip.base}-poster.webp`;
  const webm = clip.webm ? `<source src="${clip.base}.webm" type="video/webm">` : "";
  const track = clip.captions ? `<track kind="captions" src="${clip.base}.en.vtt" srclang="en" label="English">` : "";
  return `<figure class="example-video"><video controls preload="metadata" playsinline width="${clip.width}" height="${clip.height}" poster="${poster}" aria-label="${clip.label}">${webm}<source src="${clip.base}.mp4" type="video/mp4">${track}</video>${caption(clip, ` <a href="${clip.page}">Watch the walkthrough ↗</a>`)}</figure>`;
}

function shot(key) {
  const image = media.shots[key];
  return `<figure><img src="${image.src}" width="${image.width}" height="${image.height}" loading="lazy" alt="${image.alt}">${caption(image, "")}</figure>`;
}

function shots(...keys) {
  return `<div class="example-shots">${keys.map((key) => shot(key)).join("")}</div>`;
}

// [WEB-CONTENT-JOURNAL] Techdoc 0.2 registers its index virtually; replace its
// content after registration to retain the plugin's collections and routes.
function journalIndex(config) {
  const template = config.virtualTemplates["blog/index.njk"];
  template.content = '{% include "journal.njk" %}';
  template.data = {
    layout: "layouts/base.njk",
    title: "Journal — lspkit",
    description: "Building language servers and MCP servers in Rust: notes from the lspkit workbench.",
    permalink: "/blog/",
  };
}

// [WEB-CONTENT] Keep structural layouts and generated indexes in techdoc.
export default function configure(eleventyConfig) {
  eleventyConfig.addPlugin(techdoc, {
    features: { blog: true, docs: true, darkMode: true, i18n: false },
  });
  eleventyConfig.addPlugin(journalIndex);
  eleventyConfig.addShortcode("video", video);
  eleventyConfig.addShortcode("shots", shots);
  eleventyConfig.addPassthroughCopy("src/assets");
  return { dir: { input: "src", output: "_site" }, markdownTemplateEngine: "njk" };
}
