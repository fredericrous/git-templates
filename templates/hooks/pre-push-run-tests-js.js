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
    // Skip dirs without a "test" script (e.g. GitOps repos, tooling pkgs) —
    // `npm test` there fails with "Missing script: test" and blocks the push.
    let pkg;
    try {
      pkg = JSON.parse(fs.readFileSync(`${gitRoot}/${folder}/package.json`, 'utf8'));
    } catch {
      return;
    }
    if (!pkg.scripts || !pkg.scripts.test) return;
    execSync(`cd ${gitRoot}/${folder}; npm test || exit 1`, { stdio: 'inherit' });
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
