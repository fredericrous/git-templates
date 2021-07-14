#!/usr/bin/env node
// Run tests before push
// Author: https://github.com/fredericrous

const runTests = require('./common/run-tests');
const filesToDetect = /\.go$/;

(async function () {
  try {
    const run = await runTests();
    run({
      rootFile: 'go.md',
      testCommand: 'go test -race ./...',
      filesToDetect,
    });
  } catch (e) {
    process.exit(1);
  }
})();
