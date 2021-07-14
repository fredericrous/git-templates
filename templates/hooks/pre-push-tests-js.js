#!/usr/bin/env node
// Run tests before push
// Author: https://github.com/fredericrous

const runTests = require('./common/run-tests');
const filesToDetect = /\.(js|jsx|ts|tsx|vue)$/;

(async function () {
  try {
    const run = await runTests();
    let retValue = await run({
      rootFile: 'pnpm-lock.yaml',
      testCommand: 'pnpm test && pnpm audit',
      filesToDetect,
    });
    if (retValue !== 2) return;
    retValue = await run({
      rootFile: 'yarn.lock',
      testCommand: 'yarn test && yarn audit',
      filesToDetect,
    });
    if (retValue !== 2) return;
    retValue = await run({
      rootFile: 'package.json',
      testCommand: 'npm test && npm audit',
      filesToDetect,
    });
  } catch (e) {
    process.exit(1);
  }
})();
