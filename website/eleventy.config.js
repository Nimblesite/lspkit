import techdoc from "eleventy-plugin-techdoc";

// [WEB-CONTENT] Keep structural layouts and generated indexes in techdoc.
export default function configure(eleventyConfig) {
  eleventyConfig.addPlugin(techdoc, {
    features: { blog: true, docs: true, darkMode: true, i18n: false },
  });
  eleventyConfig.addPassthroughCopy("src/assets");
  return { dir: { input: "src", output: "_site" }, markdownTemplateEngine: "njk" };
}
