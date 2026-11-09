import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

// A dependency-free static build. All URLs are relative so the same output
// works at /caskade/, at a custom domain's root, and in local previews.
const root = new URL("./", import.meta.url);
const output = new URL("dist/", root);
const languages = ["en", "sv"];
const template = await readFile(new URL("index.html", root), "utf8");
const catalogues = await Promise.all(
  languages.map(async (language) =>
    JSON.parse(await readFile(new URL(`locales/${language}.json`, root), "utf8")),
  ),
);
const keys = Object.keys(catalogues[0]).sort();
const usedKeys = new Set([...template.matchAll(/\{\{(\w+)\}\}/g)].map((match) => match[1]));
const builtins = [
  "language",
  "assets",
  "englishPage",
  "swedishPage",
  "englishCurrent",
  "swedishCurrent",
  "canonical",
  "english",
  "swedish",
];
const escapeHtml = (value) =>
  value
    .replaceAll("&", "&amp;")
    .replaceAll('"', "&quot;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");

// Validate every translation before replacing the last successful build.
for (const [index, catalogue] of catalogues.entries()) {
  if (JSON.stringify(Object.keys(catalogue).sort()) !== JSON.stringify(keys)) {
    throw new Error(`Translation keys differ in ${languages[index]}.json`);
  }
  for (const [key, value] of Object.entries(catalogue)) {
    if (typeof value !== "string" || !value.trim()) throw new Error(`Empty translation: ${key}`);
    if (!usedKeys.has(key)) throw new Error(`Unused translation: ${key}`);
  }
  for (const key of usedKeys) {
    if (!builtins.includes(key) && !(key in catalogue))
      throw new Error(`Missing translation: ${key}`);
  }
}

await rm(output, { recursive: true, force: true });
await mkdir(new URL("sv/", output), { recursive: true });
await cp(new URL("assets/", root), new URL("assets/", output), { recursive: true });
await cp(new URL("site.css", root), new URL("assets/site.css", output));
await cp(new URL("../../packages/tokens/tokens.css", root), new URL("assets/tokens.css", output));
await cp(new URL("../desktop/public/caskade.svg", root), new URL("assets/caskade.svg", output));

for (const [index, language] of languages.entries()) {
  const prefix = language === "en" ? "./" : "../";
  const values = {
    ...catalogues[index],
    language,
    assets: `${prefix}assets/`,
    englishPage: language === "en" ? "./" : "../",
    swedishPage: language === "en" ? "./sv/" : "./",
    englishCurrent: language === "en" ? "page" : "false",
    swedishCurrent: language === "sv" ? "page" : "false",
    canonical: `https://krazyjakee.github.io/caskade/${language === "sv" ? "sv/" : ""}`,
    english: "https://krazyjakee.github.io/caskade/",
    swedish: "https://krazyjakee.github.io/caskade/sv/",
  };
  const html = template.replace(/\{\{(\w+)\}\}/g, (_, key) => escapeHtml(values[key]));
  const page = new URL(language === "en" ? "index.html" : "sv/index.html", output);
  await writeFile(page, html);
}
await writeFile(new URL(".nojekyll", output), "");
console.log(`Built English and Swedish pages in ${fileURLToPath(output)}`);
