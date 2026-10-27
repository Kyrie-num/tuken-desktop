// Run the cold-start routing function from the official installed Desktop.
// This catches changes to the private sidebar preference used by Tuken.
const fs = require("node:fs");
const vm = require("node:vm");
const assert = require("node:assert/strict");

const archive = fs.readFileSync(process.argv[2]);
const headerSize = archive.readUInt32LE(4);
const jsonSize = archive.readUInt32LE(12);
const tree = JSON.parse(archive.subarray(16, 16 + jsonSize));
function* files(node, prefix = "") {
  for (const [name, value] of Object.entries(node.files ?? {})) {
    const path = prefix + name;
    if (value.files) yield* files(value, path + "/");
    else yield [path, value];
  }
}
let checked = false;
for (const [name, info] of files(tree)) {
  if (!name.startsWith(".vite/build/index.") || !name.endsWith(".js") || info.unpacked) continue;
  const start = 8 + headerSize + Number(info.offset);
  const source = archive.subarray(start, start + info.size).toString("utf8");
  const match = source.match(/var ([\w$]+)="coldLaunch";(function ([\w$]+)\([^)]*storedSidebarMode[^)]*\)\{[^}]*\})/);
  if (!match) continue;
  for (const isCustom3p of [true, false]) {
    const result = vm.runInNewContext(
      `var ${match[1]} = "coldLaunch"; ${match[2]}; ${match[3]}("https://claude.ai/", {storedSidebarMode:"epitaxy", isCustom3p:${isCustom3p}}).pathname`,
      { URL },
      { timeout: 1000 },
    );
    assert.equal(result, "/epitaxy", `Desktop ignored Code preference (3p=${isCustom3p})`);
  }
  console.log(`Official Desktop cold start enters Code: ${name}`);
  checked = true;
  break;
}
assert(checked, "Desktop changed its startup routing; review the preference before shipping");
