import { defineConfig } from '@vscode/test-cli';
import crypto from 'node:crypto';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
// Short user-data dir: VS Code's IPC socket path must stay under macOS's 104-byte limit.
const userDataDir = path.join(os.tmpdir(), `lspui-${crypto.createHash('sha256').update(here).digest('hex').slice(0, 8)}`);

export default defineConfig({
  tests: [{
    label: 'ui-suite',
    files: 'out/src/test/suite/**/*.test.js',
    extensionDevelopmentPath: path.join(here, 'src', 'test', 'host'),
    workspaceFolder: path.join(here, 'src', 'test', 'fixtures'),
    launchArgs: ['--disable-extensions', '--user-data-dir', userDataDir],
    mocha: { ui: "tdd", timeout: 60_000, bail: false },
  }],
});
