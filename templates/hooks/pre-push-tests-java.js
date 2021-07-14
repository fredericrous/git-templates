#!/usr/bin/env node
// Run tests before push
// Author: https://github.com/fredericrous

const runTests = require('./common/run-tests');
const filesToDetect = /\.(java|kt|groovy)$/;

(async function () {
  try {
    const run = await runTests();
    let retValue = await run({
      rootFile: 'build.gradle',
      testCommand: 'gradle test',
      filesToDetect,
    });
    if (retValue !== 2) return;
    retValue = await run({
      rootFile: 'pom.xml',
      testCommand: 'mvn test',
      filesToDetect,
    });
  } catch (e) {
    process.exit(1);
  }
})();
