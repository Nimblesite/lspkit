// [WEB-CONTENT] Set SITE_URL to the real origin when publishing.
const site = {
  name: "lspkit",
  title: "lspkit — One engine. Every surface.",
  description: "lspkit is the language server toolkit: write your language analysis once in Rust, serve it over LSP and as an MCP server, and profile it with a CPU and memory profiler built into VS Code.",
  url: process.env.SITE_URL || "http://localhost:8080",
  stylesheet: "/assets/css/styles.css",
  themeColor: "#f4f1e9",
  favicon: "/assets/images/favicon.ico",
  appleTouchIcon: "/assets/images/apple-touch-icon.png",
  logo: "/assets/images/logo.png",
};

export default site;
