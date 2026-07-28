#!/usr/bin/env node
// # Run tests before push
// # Author: https://github.com/fredericrous

const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const readline = require('readline');
const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout,
  terminal: false,
});

const gitCommand = `git hash-object --stdin </dev/null | tr '[0-9a-f]' '0'`;
const zero = execSync(gitCommand).toString().trim();

function executeNpmPerProject(line) {
  const [, /* localRef */ localOid, , /* remoteRef */ remoteOid] = line.split(' ');

  if (localOid === zero) return; // handle delete
  const range = remoteOid === zero ? localOid : `${remoteOid}..${localOid}`;
  const modifiedFiles = execSync(`git diff-tree --no-commit-id --name-only -r "${range}"`)
    .toString()
    .split('\n');
  const jsFiles = modifiedFiles
    .filter((file) => /\.(js|jsx|ts|tsx|vue)$/.test(file))
    .map(path.dirname);

  if (!jsFiles.length) return;
  const gitRoot = execSync('git rev-parse --show-toplevel').toString().trim();
  const allPkgJson = execSync(`fd package.json ${gitRoot}`)
    .toString()
    .split('\n')
    .filter((x) => !!x)
    .map(path.dirname)
    .map((x) => x.replace(new RegExp(`^${gitRoot}/`), ''));

  const execTests = (folder) => {
    // Skip dirs without any gate script (e.g. GitOps repos, tooling pkgs) —
    // `npm test` there fails with "Missing script: test" and blocks the push.
    let pkg;
    try {
      pkg = JSON.parse(fs.readFileSync(`${gitRoot}/${folder}/package.json`, 'utf8'));
    } catch {
      return;
    }
    const scripts = (pkg && pkg.scripts) || {};
    // Whichever of these the package defines, cheapest first, stopping at the
    // first failure — so a type error costs seconds, not a full suite run.
    //
    // Scoped by script presence, like pre-commit-pyright scopes on a
    // [tool.pyright] table: a repo that defines none is skipped entirely, so
    // this stays a no-op where it doesn't apply.
    //
    // `lint` is deliberately absent: pre-commit-lint-js already lints staged
    // files with the repo's pinned eslint, so repeating it here costs time and
    // catches nothing new. Everything else CI runs belongs here — the gate
    // exists so CI is never the first to see a failure.
    const gate = ['typecheck', 'test:unit', 'test'].filter((s) => scripts[s]);
    for (const script of gate)
      execSync(`cd "${gitRoot}/${folder}"; npm run ${script} || exit 1`, {
        stdio: 'inherit',
      });
  };
  // .some, not .filter: only run tests in package dirs that actually contain a
  // modified JS/TS file (the old .filter returned a truthy array for every pkg,
  // so it ran npm test everywhere).
  const foldersToExecTests = allPkgJson.filter((pkg) =>
    jsFiles.some((file) => file.startsWith(pkg))
  );
  foldersToExecTests.forEach((folder) => execTests(folder));
}

rl.on('line', (line) => {
  try {
    executeNpmPerProject(line);
  } catch (e) {
    process.exit(1);
  }
});
