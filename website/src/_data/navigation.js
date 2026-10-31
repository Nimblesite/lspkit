// [WEB-CONTENT] Header and footer links. The repo is one click from every page.
const repo = "https://github.com/Nimblesite/lspkit";

const navigation = {
  repo,
  main: [
    { text: "Docs", url: "/docs/" },
    { text: "Journal", url: "/blog/" },
    { text: "GitHub ↗", url: repo, external: true },
  ],
  footer: [{ title: "The foundation for any language's tooling.", items: [
    { text: "GitHub ↗", url: repo },
    { text: `© ${new Date().getFullYear()} Nimblesite ↗`, url: "https://www.nimblesite.co" },
  ] }],
};

export default navigation;
