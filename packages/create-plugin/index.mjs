#!/usr/bin/env node
// `pnpm create caskade-plugin` / `just plugin-new` (#130).
//
//   create-caskade-plugin [folder] [--id dev.example.hello] [--name "Hello"]
//                         [--ui | --no-ui] [--sdk <caskade checkout>] [--yes]
//
// Every plugin has a Rust backend; --ui (the default) adds a view it draws
// with Caskade's components (plugin API 0.2, #109). Missing answers are
// asked for on a terminal; --yes takes the defaults. --sdk links the
// unpublished Rust SDK from a Caskade checkout.

import { basename, resolve } from "node:path";
import { stdin, stdout } from "node:process";
import { createInterface } from "node:readline/promises";
import { generate, validId } from "./generate.mjs";

function parse(argv) {
  const options = { yes: false };
  const rest = [];
  for (let at = 0; at < argv.length; at++) {
    const arg = argv[at];
    const value = () => {
      const next = argv[++at];
      if (next === undefined) throw new Error(`${arg} needs a value`);
      return next;
    };
    if (arg === "--id") options.id = value();
    else if (arg === "--name") options.name = value();
    else if (arg === "--sdk") options.sdk = value();
    else if (arg === "--ui") options.ui = true;
    else if (arg === "--no-ui") options.ui = false;
    else if (arg === "--backend") options.backend = true;
    else if (arg === "--no-backend") options.backend = false;
    else if (arg === "--yes" || arg === "-y") options.yes = true;
    else if (arg === "--help" || arg === "-h") options.help = true;
    else if (arg.startsWith("-")) throw new Error(`unknown option ${arg}`);
    else rest.push(arg);
  }
  options.dir = rest[0];
  return options;
}

const slug = (text) =>
  text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "") || "my-plugin";

async function main() {
  const options = parse(process.argv.slice(2));
  if (options.help) {
    console.log(
      "Usage: create-caskade-plugin [folder] [--id <reverse-dns>] [--name <name>] [--ui|--no-ui] [--sdk <caskade checkout>] [--yes]",
    );
    return;
  }
  const ask = !options.yes && stdin.isTTY;
  const prompt = ask ? createInterface({ input: stdin, output: stdout }) : null;
  const question = async (text, fallback) => {
    if (!prompt) return fallback;
    const answer = (await prompt.question(`${text} (${fallback}): `)).trim();
    return answer || fallback;
  };
  const yesNo = async (text, fallback) =>
    /^y/i.test(await question(`${text} [y/n]`, fallback ? "y" : "n"));
  try {
    const name =
      options.name ??
      (await question("Plugin name", options.dir ? basename(options.dir) : "My Plugin"));
    let id = options.id ?? (await question("Plugin id", `dev.example.${slug(name)}`));
    while (!validId(id) && prompt)
      id = await question(
        "Use reverse-DNS, lowercase (e.g. dev.example.hello)",
        `dev.example.${slug(name)}`,
      );
    const ui = options.ui ?? (await yesNo("A view, drawn by the backend", true));
    const dir = resolve(options.dir ?? slug(name));
    const files = generate({ id, name, ui, backend: options.backend, dir, sdk: options.sdk });
    console.log(`Created ${id} in ${dir}:`);
    for (const file of files) console.log(`  ${file}`);
    console.log(`\nNext: cd ${dir} && npm run build && caskade dev .`);
  } finally {
    prompt?.close();
  }
}

main().catch((error) => {
  console.error(`create-caskade-plugin: ${error.message}`);
  process.exitCode = 1;
});
