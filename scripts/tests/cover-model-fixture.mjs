import { readFile } from "node:fs/promises";
import vm from "node:vm";
import ts from "typescript";

export async function coverModelModule(context) {
  const url = new URL("../../src/lib/cover-elements.ts", import.meta.url);
  const source = await readFile(url, "utf8");
  const { outputText } = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
  });
  const module = new vm.SourceTextModule(outputText, { context });
  await module.link(async specifier => {
    if (!specifier.endsWith("/cover-styles.json")) throw Error(`Unexpected import ${specifier}`);
    const presets = JSON.parse(await readFile(new URL(specifier, url), "utf8"));
    return new vm.SyntheticModule(["default"], function() { this.setExport("default", presets); }, { context });
  });
  await module.evaluate();
  return module;
}
