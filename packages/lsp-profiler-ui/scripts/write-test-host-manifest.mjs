// Writes src/test/host/package.json: the manifest of the extension that hosts
// the library under test, generated from the same contributions a real host
// uses ([PROF-UI-IDS]).
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { profilerContributions } from "./contributions.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const host = { prefix: "lspui", brand: "LspUi", container: "lspui-explorer", debugType: "lspui-debug", restartCommand: "lspui.restartServer" };
const contributes = profilerContributions(host);
const manifest = {
  name: "lsp-profiler-ui-test-host",
  displayName: "LspUi",
  publisher: "lspui",
  version: "0.0.1",
  engines: { vscode: "^1.99.0" },
  main: "../../../out/src/test/host/extension.js",
  activationEvents: ["onStartupFinished"],
  contributes: {
    commands: contributes.commands,
    viewsContainers: { activitybar: [{ id: host.container, title: "LspUi", icon: "$(pulse)" }] },
    views: contributes.views,
    viewsWelcome: contributes.viewsWelcome,
    menus: contributes.menus,
    keybindings: contributes.keybindings,
    configuration: { title: "LspUi", properties: contributes.configuration },
  },
};
const target = path.join(here, "..", "src", "test", "host", "package.json");
fs.mkdirSync(path.dirname(target), { recursive: true });
fs.writeFileSync(target, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`wrote ${target}`);
