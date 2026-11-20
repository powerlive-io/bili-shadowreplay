import { readFile } from "node:fs/promises";
import vm from "node:vm";
import ts from "typescript";

export function syntheticModule(exports) {
  return new vm.SyntheticModule(Object.keys(exports), function () {
    for (const [key, value] of Object.entries(exports)) this.setExport(key, value);
  });
}

// Match the app's ESM output without relying on Node's native TS support.
export async function loadLibModule(name, packages = []) {
  const source = await readFile(new URL(`../../src/lib/${name}.ts`, import.meta.url), "utf8");
  const module = new vm.SourceTextModule(ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
  }).outputText);
  await module.link(async specifier => {
    if (!packages.includes(specifier)) throw new Error(`Unexpected ${name} dependency: ${specifier}`);
    return syntheticModule(await import(specifier));
  });
  await module.evaluate();
  return module.namespace;
}
