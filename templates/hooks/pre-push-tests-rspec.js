#!/usr/bin/env node
// Run tests before push
// Author: https://github.com/fredericrous

const runTests = require('./common/run-tests');
const filesToDetect = /\.rb$/;

(async function () {
  try {
    const run = await runTests();
    run({
      rootFile: 'Rakefile',
      testCommand: 'RAILS_ENV=test rails db:schema:load && rails db:migrate && bundle exec rspec',
      filesToDetect,
    });
  } catch (e) {
    process.exit(1);
  }
})();
