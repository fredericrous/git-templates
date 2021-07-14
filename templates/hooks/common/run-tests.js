const { execSync } = require('child_process');
const path = require('path');
const readline = require('readline');

const gitCommand = `git hash-object --stdin </dev/null | tr '[0-9a-f]' '0'`;
const zero = execSync(gitCommand).toString().trim();

function executePerProject(localOid, remoteOid, { rootFile, testCommand, filesToDetect }) {
  const range = remoteOid === zero ? localOid : `${remoteOid}..${localOid}`;
  const jsFiles = [
    ...new Set(
      execSync(`git diff-tree --no-commit-id --name-only -r "${range}"`)
        .toString()
        .split('\n')
        .filter((file) => filesToDetect.test(file))
        .map(path.dirname)
    ),
  ];
  if (!jsFiles.length) return 0;
  const gitRoot = execSync('git rev-parse --show-toplevel').toString().trim();
  const allPkgJson = execSync(`fd ${rootFile} ${gitRoot}`)
    .toString()
    .split('\n')
    .filter((x) => !!x)
    .map(path.dirname)
    .map((x) => x.replace(new RegExp(`^${gitRoot}/`), ''));

  const execTests = (folder) =>
    execSync(`cd ${gitRoot}/${folder}; ${testCommand} || exit 1`, { stdio: 'inherit' });
  const foldersToExecTests = allPkgJson.filter((pkg) =>
    jsFiles.filter((file) => file.startsWith(pkg))
  );

  if (!foldersToExecTests.length) return 2;
  foldersToExecTests.forEach((folder) => execTests(folder));
  return 0;
}

function handleExit(code) {
  execSync('git stash pop || true');
  process.exit(code);
}

module.exports = async function main() {
  const line = await new Promise((resolve) => {
    readline
      .createInterface({
        input: process.stdin,
        output: process.stdout,
        terminal: false,
      })
      .on('line', (line) => resolve(line.split(' ')));
  });
  /**
   * if filesToDetect are to be commited,
   * run testCommand wherever there is a rootFile in a top folder
   * @param {Object[]} config
   * @param {string} config[].rootFile
   * @param {string} config[].testCommand
   * @param {string} config[].filesToDetect
   * @returns {number} success=0 error=1(reject) nofiles=0 noRunner=2
   */
  return (config) =>
    new Promise((resolve, reject) => {
      const [, /* localRef */ localOid, , /* remoteRef */ remoteOid] = line;
      if (localOid === zero) return; // handle delete
      let shouldPop = true;
      try {
        const stdout = execSync('git stash').toString().trim();
        if (stdout === 'No local changes to save') {
          throw '';
        }
        process.once('SIGINT', handleExit);
        process.once('SIGTERM', handleExit);
      } catch (e) {
        shouldPop = false;
      }
      let retValue;
      try {
        retValue = executePerProject(localOid, remoteOid, config);
      } catch (e) {
        retValue = 1;
      } finally {
        shouldPop && execSync('git stash pop &> /dev/null || true');
        retValue === 1 ? reject() : resolve(retValue);
      }
    });
};
