#!/usr/bin/env node
// Run tests before push
// Author: https://github.com/fredericrous

const runTests = require('./common/run-tests');
const filesToDetect = /\.py$/;

(async function () {
  try {
    const run = await runTests();
    run({
      rootFile: 'manage.py',
      testCommand:
        './manage.py migrate && ./manage.py collectstatic --no-input && ' +
        './manage.py check && ./manage.py test',
      filesToDetect,
    });
  } catch (e) {
    process.exit(1);
  }
})();
