import techdoc from "eleventy-plugin-techdoc";

// [WEB-CONTENT-JOURNAL] Techdoc 0.2 registers its index virtually; replace its
// content after registration to retain the plugin's collections and routes.
function journalIndex(config) {
  const template = config.virtualTemplates["blog/index.njk"];
  template.content = '{% include "journal.njk" %}';
  template.data = {
    layout: "layouts/base.njk",
    title: "Journal — lspkit",
    description: "Notes on building better language servers and developer tools.",
    permalink: "/blog/",
  };
}

// [WEB-CONTENT] Keep structural layouts and generated indexes in techdoc.
export default function configure(eleventyConfig) {
  eleventyConfig.addPlugin(techdoc, {
    features: { blog: true, docs: true, darkMode: true, i18n: false },
  });
  eleventyConfig.addPlugin(journalIndex);
  eleventyConfig.addPassthroughCopy("src/assets");
  return { dir: { input: "src", output: "_site" }, markdownTemplateEngine: "njk" };
}
