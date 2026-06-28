import { copyFileSync, mkdirSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const publicDir = resolve(__dirname, "..", "docs", "public");
mkdirSync(publicDir, { recursive: true });

const assets = [
  ["../../scripts/install.sh", "install.sh"],
  ["../../scripts/install.ps1", "install.ps1"],
  ["../../assets/logo.svg", "logo.svg"],
];

for (const [src, dest] of assets) {
  copyFileSync(resolve(__dirname, src), resolve(publicDir, dest));
  console.log(`  copied ${dest}`);
}
