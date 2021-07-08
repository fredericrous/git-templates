#!/usr/bin/env node
/**
 * Prevent commit of forbidden terms
 *
 * Author: fredericrous
 */
const { execSync } = require('child_process');
const { EOL } = require('os');
const path = require('path');

const FILES_TO_SEARCH_IN = /\.(jsx?|tsx?|vue)$/;
const TERMS = {
  fit: '\\s*fit\\(',
  fdescribe: '\\s*fdescribe\\(',
  debugger: 'debugger;?',
  skipOnly: '(describe|context|it)\\.(skip|only)',
};

const message = (function formatMsg() {
  const COLORS = {
    green: '112',
    orange: '208',
    red: '160',
  };
  const mark = {
    ok: '\u2713',
    err: '\u2717',
  };

  const colorMsg = (msg, color) => `\u001b[38;5;${color}m${msg}\u001b[0m`;
  return {
    valid: (msg) => console.info(`  ${colorMsg(mark.ok, COLORS.green)} ${msg}`),
    error: (msg) => console.error(`  ${colorMsg(mark.err, COLORS.red)} ${msg}`),
    orange: (msg) => colorMsg(msg, COLORS.orange),
  };
})();

const scriptPath = path.relative(path.resolve(__dirname, '..'), process.argv[1]);
function validStagedFilesPerTerm(status, [term, matchRegex]) {
  const gitCommand = `git diff --cached -G"${matchRegex}" --diff-filter=d --name-only`;
  const filesArray = execSync(gitCommand).toString().split(EOL);
  const matches = filesArray
    .filter((file) => FILES_TO_SEARCH_IN.test(file.trim()))
    .filter((file) => file !== scriptPath);
  if (matches.length) {
    if (status === 0) {
      message.error('Unwanted terms found');
    }
    status = 1;
    console.info(`    The following files contains '${message.orange(term)}' in them:`);
    matches.map((line) => EOL + console.info('    - ' + message.orange(line)));
  }
  return status;
}
const status = Object.entries(TERMS).reduce(validStagedFilesPerTerm, 0);
if (status === 0) {
  message.valid('No unwanted terms where found');
}
process.exit(status);
